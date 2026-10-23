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

fn encode(command: Command) -> String {
    serde_json::to_string(&Request {
        version: 1,
        command,
    })
    .unwrap()
}

struct PausedAuth {
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
impl stem_core::security::Authenticator for PausedAuth {
    fn verify(&mut self, _: stem_core::Id, _: &std::collections::BTreeSet<Scope>) -> bool {
        self.entered.send(()).unwrap();
        self.release.recv().unwrap();
        true
    }
}

#[test]
fn query_saturation_reserves_edit_admission_and_preserves_edit_undo_order() {
    use std::{
        sync::{mpsc, Arc},
        time::Duration,
    };
    use stem_core::{content::Content, host::WriterLimits, store::Kind, Id};
    let d = tempfile::tempdir().unwrap();
    let h = Arc::new(
        Host::open_with_limits(
            d.path().join("priority.db"),
            WriterLimits {
                authoring_count: 4,
                authoring_bytes: 4096,
                query_count: 1,
                query_bytes: 1024,
            },
        )
        .unwrap(),
    );
    let note = request(
        &h,
        Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "private fixture title".into(),
        },
    );
    let id = Id::parse(note["id"].as_str().unwrap()).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let blocked = Arc::clone(&h);
    let auth = std::thread::spawn(move || {
        blocked.authenticate(
            [Scope::App].into_iter().collect(),
            Box::new(PausedAuth {
                entered: entered_tx,
                release: release_rx,
            }),
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    // The query was admitted first, but queued authoring takes precedence.
    let read = h
        .submit(&encode(Command::Blocks {
            entity: id,
            offset: 0,
            limit: 10,
        }))
        .unwrap();
    assert!(matches!(
        h.submit(&encode(Command::Status {})),
        Err(Error::ResourceLimit)
    ));
    let append = h
        .submit(&encode(Command::Append {
            entity: id,
            content: Content::Text("private fixture content".into()),
        }))
        .unwrap();
    let undo = h.submit(&encode(Command::Undo { entity: id })).unwrap();
    let redo = h.submit(&encode(Command::Redo { entity: id })).unwrap();
    assert!(matches!(
        h.submit(&encode(Command::Undo { entity: id })),
        Err(Error::ResourceLimit)
    ));
    let mut status = h.subscribe_status();
    let pending = status.next_timeout(Duration::ZERO).unwrap().unwrap();
    assert_eq!(pending.pending_authoring, 4); // includes running verifier
    assert_eq!(pending.pending_queries, 1);
    let json = serde_json::to_string(&pending).unwrap();
    assert!(!json.contains(&id.to_string()) && !json.contains("private fixture"));
    release_tx.send(()).unwrap();
    auth.join().unwrap().unwrap();
    assert!(append.wait().unwrap().ok);
    assert!(undo.wait().unwrap().ok);
    assert!(redo.wait().unwrap().ok);
    let blocks = read.wait().unwrap().data.unwrap();
    assert_eq!(blocks.as_array().unwrap().len(), 1);
    assert_eq!(blocks[0]["content"]["value"], "private fixture content");
    let idle = status.next_timeout(Duration::ZERO).unwrap().unwrap();
    assert_eq!((idle.pending_authoring, idle.pending_queries), (0, 0));
    assert!(!idle.needs_attention && !idle.closed);
    assert!(status.next_timeout(Duration::ZERO).unwrap().is_none());
    drop(h);
    let closed = status.next_timeout(Duration::ZERO).unwrap().unwrap();
    assert!(closed.closed);
    assert!(status.next_timeout(Duration::ZERO).unwrap().is_none());
}

#[test]
fn byte_budgets_are_independent_and_rejection_does_not_acknowledge_a_save() {
    use std::time::Duration;
    use stem_core::{host::WriterLimits, store::Kind};
    let d = tempfile::tempdir().unwrap();
    let h = Host::open_with_limits(
        d.path().join("bytes.db"),
        WriterLimits {
            authoring_count: 4,
            authoring_bytes: 256,
            query_count: 4,
            query_bytes: 20,
        },
    )
    .unwrap();
    assert!(matches!(
        h.submit(&encode(Command::Status {})),
        Err(Error::ResourceLimit)
    ));
    let mut status = h.subscribe_status();
    let initial = status.next_timeout(Duration::ZERO).unwrap().unwrap();
    assert_eq!((initial.pending_queries, initial.pending_authoring), (0, 0));
    assert!(matches!(
        h.submit(&encode(Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "x".repeat(1024)
        })),
        Err(Error::ResourceLimit)
    ));
    let create = h
        .submit(&encode(Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "fits".into(),
        }))
        .unwrap();
    assert!(create.wait().unwrap().ok);
    let bad = h
        .submit(&encode(Command::Undo {
            entity: stem_core::Id::new(),
        }))
        .unwrap();
    assert!(!bad.wait().unwrap().ok);
    let failed = status.next_timeout(Duration::ZERO).unwrap().unwrap();
    assert!(failed.needs_attention);
    assert_eq!(failed.pending_authoring, 0);
}

#[test]
fn dropped_receipts_still_drain_on_close_and_observers_see_final_closed_state() {
    use std::time::Duration;
    use stem_core::store::Kind;
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("drain.db");
    let h = Host::open(path.clone(), 32).unwrap();
    let mut observer = h.subscribe_status();
    for i in 0..16 {
        drop(
            h.submit(&encode(Command::Create {
                kind: Kind::Note,
                parent: None,
                title: i.to_string(),
            }))
            .unwrap(),
        );
    }
    drop(h);
    let closed = observer.next_timeout(Duration::ZERO).unwrap().unwrap();
    assert!(closed.closed);
    assert_eq!(closed.pending_authoring, 0);
    let h = Host::open(path, 8).unwrap();
    assert_eq!(
        request(&h, Command::List { parent: None })
            .as_array()
            .unwrap()
            .len(),
        16
    );
}

#[test]
fn verifier_panic_fails_closed_without_killing_the_writer_or_retaining_access() {
    use stem_core::{security::Authenticator, store::Kind, Id};
    struct Panics;
    impl Authenticator for Panics {
        fn verify(&mut self, _: Id, _: &std::collections::BTreeSet<Scope>) -> bool {
            panic!("synthetic verifier failure")
        }
    }
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("auth.db"), 8).unwrap();
    let n = request(
        &h,
        Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "locked".into(),
        },
    );
    let id = Id::parse(n["id"].as_str().unwrap()).unwrap();
    h.authenticate(
        [Scope::Entity(id)].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    request(
        &h,
        Command::SetLock {
            entity: id,
            revision: n["revision"].as_u64().unwrap(),
            locked: true,
        },
    );
    assert!(matches!(
        h.authenticate([Scope::App].into_iter().collect(), Box::new(Panics)),
        Err(Error::WorkerFailed)
    ));
    assert!(!h.request(&encode(Command::Read { id })).unwrap().ok);
    assert!(h.request(&encode(Command::Status {})).unwrap().ok);
}
