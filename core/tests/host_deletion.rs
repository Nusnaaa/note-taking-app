mod common;
use common::*;
use serde_json::Value;
use stem_core::{
    host::{Command, Host, Request, Response},
    security::Scope,
    store::Kind,
    Id,
};
fn call(h: &Host, command: Command) -> Response {
    h.request(
        &serde_json::to_string(&Request {
            version: 1,
            command,
        })
        .unwrap(),
    )
    .unwrap()
}
fn ok(h: &Host, command: Command) -> Value {
    let response = call(h, command);
    assert!(response.ok, "{:?}", response.error);
    response.data.unwrap()
}
fn note(h: &Host, title: &str) -> Id {
    serde_json::from_value(
        ok(
            h,
            Command::Create {
                kind: Kind::Note,
                parent: None,
                title: title.into(),
            },
        )["id"]
            .clone(),
    )
    .unwrap()
}
fn prepare(h: &Host, entity: Id, permanent: bool) -> Id {
    serde_json::from_value(ok(h, Command::PrepareDeletion { entity, permanent })["review"].clone())
        .unwrap()
}
#[test]
fn host_trash_restore_and_reviewed_purge_preserve_then_retire_source_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let h = Host::open(path.clone(), 16).unwrap();
    let n = note(&h, "source");
    let original = stroke(0);
    let b: Id = serde_json::from_value(
        ok(
            &h,
            Command::Capture {
                ticket: Id::new(),
                entity: n,
                strokes: vec![original.clone()],
            },
        )["id"]
            .clone(),
    )
    .unwrap();
    let review = prepare(&h, n, false);
    assert!(
        !call(
            &h,
            Command::ConfirmDeletion {
                review,
                confirmed: false
            }
        )
        .ok
    );
    ok(
        &h,
        Command::ConfirmDeletion {
            review,
            confirmed: true,
        },
    );
    assert!(!call(&h, Command::Read { id: n }).ok);
    let entries = ok(
        &h,
        Command::TrashEntries {
            offset: 0,
            limit: 10,
        },
    );
    assert_eq!(entries[0]["id"], n.to_string());
    let revision = entries[0]["revision"].as_u64().unwrap();
    assert!(
        !call(
            &h,
            Command::RestoreEntity {
                entity: n,
                revision: revision - 1
            }
        )
        .ok
    );
    ok(
        &h,
        Command::RestoreEntity {
            entity: n,
            revision,
        },
    );
    let source: Vec<stem_core::ink::Stroke> =
        serde_json::from_value(ok(&h, Command::SourceInk { block: b })).unwrap();
    assert_eq!(source, vec![original]);
    let review = prepare(&h, n, false);
    ok(
        &h,
        Command::ConfirmDeletion {
            review,
            confirmed: true,
        },
    );
    let purge = prepare(&h, n, true);
    h.close().unwrap();
    let h = Host::open(path, 16).unwrap();
    assert!(
        !call(
            &h,
            Command::ConfirmDeletion {
                review: purge,
                confirmed: true
            }
        )
        .ok
    );
    let purge = prepare(&h, n, true);
    ok(
        &h,
        Command::ConfirmDeletion {
            review: purge,
            confirmed: true,
        },
    );
    assert!(
        !call(
            &h,
            Command::ConfirmDeletion {
                review: purge,
                confirmed: true
            }
        )
        .ok
    );
    assert!(ok(
        &h,
        Command::TrashEntries {
            offset: 0,
            limit: 10
        }
    )
    .as_array()
    .unwrap()
    .is_empty());
    assert!(!call(&h, Command::SourceInk { block: b }).ok);
    assert!(
        !call(
            &h,
            Command::RestoreEntity {
                entity: n,
                revision
            }
        )
        .ok
    );
}
#[test]
fn host_dependency_review_redacts_private_ids_and_stale_generation_cannot_delete() {
    let dir = tempfile::tempdir().unwrap();
    let h = Host::open(dir.path().join("library.db"), 16).unwrap();
    let n = note(&h, "target");
    let public = note(&h, "visible");
    let private = note(&h, "private title");
    for origin in [public, private] {
        ok(
            &h,
            Command::AddLink {
                origin,
                target: n,
                anchor: None,
            },
        );
    }
    h.authenticate(
        [Scope::Entity(private)].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    ok(
        &h,
        Command::SetLock {
            entity: private,
            revision: 0,
            locked: true,
        },
    );
    ok(&h, Command::Relock {});
    let review = ok(
        &h,
        Command::PrepareDeletion {
            entity: n,
            permanent: false,
        },
    );
    assert_eq!(review["has_dependencies"], true);
    assert_eq!(review["visible_dependants"], serde_json::json!([public]));
    assert!(!review.to_string().contains(&private.to_string()));
    assert!(!review.to_string().contains("private title"));
    let token = serde_json::from_value(review["review"].clone()).unwrap();
    let newer = note(&h, "new dependency");
    ok(
        &h,
        Command::AddLink {
            origin: newer,
            target: n,
            anchor: None,
        },
    );
    assert!(
        !call(
            &h,
            Command::ConfirmDeletion {
                review: token,
                confirmed: true
            }
        )
        .ok
    );
    assert!(call(&h, Command::Read { id: n }).ok);
    let review = prepare(&h, n, false);
    ok(&h, Command::ReleaseDeletion { review });
    assert!(
        !call(
            &h,
            Command::ConfirmDeletion {
                review,
                confirmed: true
            }
        )
        .ok
    );
    let forged = h.request(&format!(r#"{{"version":1,"command":{{"op":"ConfirmDeletion","review":"{}","confirmed":true,"permanent":true}}}}"#, Id::new()));
    assert!(forged.is_err());
}
#[test]
fn host_reviews_are_local_and_relock_retires_capabilities_without_declassifying_trash() {
    let dir = tempfile::tempdir().unwrap();
    let h = Host::open(dir.path().join("one.db"), 16).unwrap();
    let other = Host::open(dir.path().join("two.db"), 16).unwrap();
    let n = note(&h, "locked");
    let proof = || [Scope::Entity(n)].into_iter().collect();
    h.authenticate(proof(), Box::new(TestAuth(true))).unwrap();
    ok(
        &h,
        Command::SetLock {
            entity: n,
            revision: 0,
            locked: true,
        },
    );
    let review = prepare(&h, n, false);
    assert!(
        !call(
            &other,
            Command::ConfirmDeletion {
                review,
                confirmed: true
            }
        )
        .ok
    );
    ok(&h, Command::Relock {});
    assert!(
        !call(
            &h,
            Command::ConfirmDeletion {
                review,
                confirmed: true
            }
        )
        .ok
    );
    h.authenticate(proof(), Box::new(TestAuth(true))).unwrap();
    let review = prepare(&h, n, false);
    ok(
        &h,
        Command::ConfirmDeletion {
            review,
            confirmed: true,
        },
    );
    ok(&h, Command::Relock {});
    assert!(ok(
        &h,
        Command::TrashEntries {
            offset: 0,
            limit: 10
        }
    )
    .as_array()
    .unwrap()
    .is_empty());
    assert!(
        !call(
            &h,
            Command::PrepareDeletion {
                entity: n,
                permanent: true
            }
        )
        .ok
    );
    h.authenticate(proof(), Box::new(TestAuth(true))).unwrap();
    let entries = ok(
        &h,
        Command::TrashEntries {
            offset: 0,
            limit: 10,
        },
    );
    assert_eq!(entries[0]["id"], n.to_string());
    ok(
        &h,
        Command::RestoreEntity {
            entity: n,
            revision: entries[0]["revision"].as_u64().unwrap(),
        },
    );
    assert!(call(&h, Command::Read { id: n }).ok);
}

#[test]
fn held_review_validation_is_non_consuming_generation_bound_and_authorises_capture_targets() {
    let dir = tempfile::tempdir().unwrap();
    let h = Host::open(dir.path().join("review.db"), 16).unwrap();
    let root: Id = serde_json::from_value(
        ok(
            &h,
            Command::Create {
                kind: Kind::Notebook,
                parent: None,
                title: "root".into(),
            },
        )["id"]
            .clone(),
    )
    .unwrap();
    let child: Id = serde_json::from_value(
        ok(
            &h,
            Command::Create {
                kind: Kind::Note,
                parent: Some(root),
                title: "child".into(),
            },
        )["id"]
            .clone(),
    )
    .unwrap();
    let value = ok(
        &h,
        Command::PrepareDeletion {
            entity: root,
            permanent: false,
        },
    );
    assert_eq!(value["capture_targets"], serde_json::json!([child]));
    let review = serde_json::from_value(value["review"].clone()).unwrap();
    ok(&h, Command::ValidateDeletionReview { review });
    ok(&h, Command::ValidateDeletionReview { review });
    assert!(call(&h, Command::Read { id: root }).ok);
    note(&h, "changed generation");
    assert!(!call(&h, Command::ValidateDeletionReview { review }).ok);
    assert!(
        !call(
            &h,
            Command::ConfirmDeletion {
                review,
                confirmed: true
            }
        )
        .ok
    );
    let review = prepare(&h, root, false);
    ok(&h, Command::ValidateDeletionReview { review });
    ok(
        &h,
        Command::ConfirmDeletion {
            review,
            confirmed: true,
        },
    );
    assert!(!call(&h, Command::Read { id: child }).ok);
}
