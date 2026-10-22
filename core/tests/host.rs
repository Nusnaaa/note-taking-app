mod common;
use common::*;
use serde_json::Value;
use stem_core::{
    host::{Command, Host, Request},
    security::Scope,
    Error,
};
fn request(h: &Host, c: Command) -> Value {
    let r = h
        .request(
            &serde_json::to_string(&Request {
                version: 1,
                command: c,
            })
            .unwrap(),
        )
        .unwrap();
    assert!(r.ok, "{:?}", r.error);
    r.data.unwrap()
}
#[test]
fn host_exercise_edits_share_identity_with_master_and_layout_undo_survives_restart() {
    use stem_core::{
        content::Content,
        objects::Rect,
        store::{Kind, WritingTarget},
        Id,
    };
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("library.db");
    let h = Host::open(path.clone(), 8).unwrap();
    let note = request(
        &h,
        Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "master".into(),
        },
    );
    let master = Id::parse(note["id"].as_str().unwrap()).unwrap();
    let exercise = request(
        &h,
        Command::CreateExercise {
            master,
            title: "exercise".into(),
            blocks: vec![],
        },
    );
    let exercise = Id::parse(exercise["id"].as_str().unwrap()).unwrap();
    let b = request(
        &h,
        Command::AppendTo {
            target: WritingTarget::Exercise(exercise),
            content: Content::Text("same working".into()),
        },
    );
    let block = Id::parse(b["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        request(
            &h,
            Command::Blocks {
                entity: master,
                offset: 0,
                limit: 10
            }
        )[0],
        b
    );
    assert_eq!(
        request(
            &h,
            Command::OpenInMaster {
                exercise,
                block: Some(block)
            }
        )["block"],
        b["id"]
    );
    request(
        &h,
        Command::SetLayout {
            block,
            expected: None,
            bounds: Rect {
                x: 1.,
                y: 2.,
                width: 3.,
                height: 4.,
            },
            z: 5,
        },
    );
    drop(h);
    let h = Host::open(path, 8).unwrap();
    assert_eq!(request(&h, Command::Layout { block })["z"], 5);
    request(&h, Command::Undo { entity: master });
    assert!(request(&h, Command::Layout { block }).is_null());
    assert_eq!(
        request(
            &h,
            Command::ExerciseBlocks {
                exercise,
                offset: 0,
                limit: 10
            }
        )[0],
        b
    );
}
#[test]
fn threaded_host_commits_reopens_and_rejects_unknown_version_and_fields() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("host.db");
    let h = Host::open(p.clone(), 8).unwrap();
    let n = request(
        &h,
        Command::Create {
            kind: stem_core::store::Kind::Note,
            parent: None,
            title: "Unicode λ ∂ 日本".into(),
        },
    );
    let id = stem_core::Id::parse(n["id"].as_str().unwrap()).unwrap();
    assert!(
        !h.request(r#"{"version":2,"command":{"op":"Status"}}"#)
            .unwrap()
            .ok
    );
    assert!(h
        .request(r#"{"version":1,"command":{"op":"Status","authenticate":true}}"#)
        .is_err());
    drop(h);
    let h = Host::open(p, 8).unwrap();
    assert_eq!(request(&h, Command::Read { id })["title"], n["title"]);
}
#[test]
fn asynchronous_writer_retry_tickets_and_lock_scopes_are_enforced() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("host.db"), 4).unwrap();
    let n = request(
        &h,
        Command::Create {
            kind: stem_core::store::Kind::Note,
            parent: None,
            title: "n".into(),
        },
    );
    let id = stem_core::Id::parse(n["id"].as_str().unwrap()).unwrap();
    let ticket = stem_core::Id::new();
    let strokes = vec![stroke(0)];
    let a = request(
        &h,
        Command::Capture {
            entity: id,
            ticket,
            strokes: strokes.clone(),
        },
    );
    let b = request(
        &h,
        Command::Capture {
            entity: id,
            ticket,
            strokes,
        },
    );
    assert_eq!(a, b);
    h.authenticate(
        [Scope::Entity(id)].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    let rev = request(&h, Command::Read { id })["revision"]
        .as_u64()
        .unwrap();
    request(
        &h,
        Command::SetLock {
            entity: id,
            revision: rev,
            locked: true,
        },
    );
    request(&h, Command::Relock {});
    let response = h
        .request(
            &serde_json::to_string(&Request {
                version: 1,
                command: Command::Read { id },
            })
            .unwrap(),
        )
        .unwrap();
    assert!(!response.ok);
    assert!(response.data.is_none());
    assert!(response.generation.is_none());
    assert!(matches!(
        h.authenticate(
            [Scope::Entity(id)].into_iter().collect(),
            Box::new(TestAuth(false))
        ),
        Err(Error::Denied)
    ));
}
#[test]
fn concurrent_host_calls_return_distinct_committed_content() {
    let d = tempfile::tempdir().unwrap();
    let h = std::sync::Arc::new(Host::open(d.path().join("host.db"), 32).unwrap());
    let mut workers = Vec::new();
    for i in 0..16 {
        let h = h.clone();
        workers.push(std::thread::spawn(move || {
            request(
                &h,
                Command::Create {
                    kind: stem_core::store::Kind::Note,
                    parent: None,
                    title: format!("{i}"),
                },
            )
        }));
    }
    let ids: std::collections::BTreeSet<_> = workers
        .into_iter()
        .map(|w| w.join().unwrap()["id"].to_string())
        .collect();
    assert_eq!(ids.len(), 16);
    assert_eq!(
        request(&h, Command::List { parent: None })
            .as_array()
            .unwrap()
            .len(),
        16
    );
}
