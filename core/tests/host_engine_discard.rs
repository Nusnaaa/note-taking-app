use std::{sync::Arc, time::Duration};
use stem_core::{
    host::{Command, Host, Request},
    processing::*,
    provisioning::{resource_digest, Transfer},
    security::Access,
    store::*,
    Error, Id,
};
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(30)).unwrap()
}
fn descriptor() -> Descriptor {
    Descriptor {
        id: "synthetic-discard".into(),
        version: "1".into(),
        adapter_version: "1".into(),
        family: Family::Recogniser,
        input: InputContract::OrderedStrokesV1,
        tasks: vec![Task::Prose],
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some("app-owned synthetic discard; no real SDK/model".into()),
        redistributable_resources: false,
        resource_sha256: Some(resource_digest(b"model")),
        runtime_contract: "stem-processing-v1".into(),
    }
}
fn fixture(capacity: usize) -> (tempfile::TempDir, Arc<Host>, Id) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path().join("library.db")).unwrap();
    let mut transfer = store
        .engine_import_plan(descriptor(), 5, true, &Access::public())
        .unwrap()
        .begin(&root.path().join("models"), &control())
        .unwrap();
    let id = transfer.id();
    transfer.append(0, b"mo", &control()).unwrap();
    drop(transfer);
    drop(store);
    let h = Arc::new(Host::open(root.path().join("library.db"), capacity).unwrap());
    (root, h, id)
}
fn call(h: &Host, command: Command) {
    let reply = h
        .request(
            &serde_json::to_string(&Request {
                version: 1,
                command,
            })
            .unwrap(),
        )
        .unwrap();
    assert!(reply.ok, "{:?}", reply.error);
}
#[test]
fn durable_pending_precedes_deletion_and_completed_receipt_is_idempotent() {
    let (root, h, id) = fixture(8);
    assert!(matches!(
        h.engine_discard_plan(id, false, None),
        Err(Error::Denied)
    ));
    let plan = h.engine_discard_plan(id, true, None).unwrap();
    let action = plan.action_id();
    let prepared = plan
        .prepare(&root.path().join("models"), &control())
        .unwrap();
    assert_eq!(prepared.action_id(), action);
    assert!(matches!(
        Transfer::resume(&root.path().join("models"), id),
        Err(Error::ResourceLimit)
    ));
    let pending = h.authorise_engine_discard(&prepared, &control()).unwrap();
    assert_eq!(pending.id, action);
    assert_eq!(pending.state, ResourceActionState::Pending);
    assert!(root.path().join(format!("models/{id}.part")).exists());
    assert_eq!(h.engine_resource_action(action).unwrap(), pending);
    let removed = prepared.remove(&control()).unwrap();
    assert_eq!(removed.action_id(), action);
    assert!(!root.path().join(format!("models/{id}.part")).exists());
    let complete = h.complete_engine_discard(&removed, &control()).unwrap();
    assert_eq!(complete.state, ResourceActionState::Completed);
    assert_eq!(
        h.complete_engine_discard(&removed, &control()).unwrap(),
        complete
    );
    assert_eq!(
        h.engine_resource_actions(None, 8).unwrap().actions,
        vec![complete]
    );
    call(
        &h,
        Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "authoring remains available".into(),
        },
    );
    drop(removed);
    h.close().unwrap();
}
#[test]
fn capture_after_consent_cancels_file_work_and_explicit_retry_keeps_original_action() {
    let (root, h, id) = fixture(8);
    let models = root.path().join("models");
    let prepared = h
        .engine_discard_plan(id, true, None)
        .unwrap()
        .prepare(&models, &control())
        .unwrap();
    let pending = h.authorise_engine_discard(&prepared, &control()).unwrap();
    let pen = Id::new();
    h.set_capture_priority(pen, true).unwrap();
    assert!(matches!(
        h.engine_discard_plan(id, true, Some(pending.id)),
        Err(Error::ResourceLimit)
    ));
    h.set_capture_priority(pen, false).unwrap();
    assert!(matches!(prepared.remove(&control()), Err(Error::Cancelled)));
    assert!(models.join(format!("{id}.part")).exists());
    h.close().unwrap();
    let h = Host::open(root.path().join("library.db"), 8).unwrap();
    let prepared = h
        .engine_discard_plan(id, true, Some(pending.id))
        .unwrap()
        .prepare(&models, &control())
        .unwrap();
    assert_eq!(
        h.authorise_engine_discard(&prepared, &control()).unwrap(),
        pending
    );
    let removed = prepared.remove(&control()).unwrap();
    assert_eq!(
        h.complete_engine_discard(&removed, &control()).unwrap().id,
        pending.id
    );
    drop(removed);
    h.close().unwrap();
}
#[test]
fn deleted_files_with_lost_completion_recover_after_close_without_rollback_claim() {
    let (root, h, id) = fixture(8);
    let models = root.path().join("models");
    let prepared = h
        .engine_discard_plan(id, true, None)
        .unwrap()
        .prepare(&models, &control())
        .unwrap();
    let pending = h.authorise_engine_discard(&prepared, &control()).unwrap();
    let removed = prepared.remove(&control()).unwrap();
    let cancelled = control();
    cancelled.cancel();
    assert!(matches!(
        h.complete_engine_discard(&removed, &cancelled),
        Err(Error::Cancelled)
    ));
    assert_eq!(
        h.engine_resource_action(pending.id).unwrap().state,
        ResourceActionState::Pending
    );
    assert!(!models.join(format!("{id}.install.json")).exists());
    // Close retires the actual guard even while a completed-work handle survives.
    h.close().unwrap();
    let next = Host::open(root.path().join("library.db"), 8).unwrap();
    assert!(h.complete_engine_discard(&removed, &control()).is_err());
    let prepared = next
        .engine_discard_plan(id, true, Some(pending.id))
        .unwrap()
        .prepare(&models, &control())
        .unwrap();
    next.authorise_engine_discard(&prepared, &control())
        .unwrap();
    let retry = prepared.remove(&control()).unwrap();
    assert_eq!(
        next.complete_engine_discard(&retry, &control())
            .unwrap()
            .state,
        ResourceActionState::Completed
    );
    drop(retry);
    drop(removed);
    next.close().unwrap();
}
#[test]
fn relock_and_parent_cancellation_never_grant_file_deletion() {
    let (root, h, id) = fixture(8);
    let models = root.path().join("models");
    let prepared = h
        .engine_discard_plan(id, true, None)
        .unwrap()
        .prepare(&models, &control())
        .unwrap();
    let cancelled = control();
    cancelled.cancel();
    assert!(matches!(
        h.authorise_engine_discard(&prepared, &cancelled),
        Err(Error::Cancelled)
    ));
    assert!(h
        .engine_resource_actions(None, 8)
        .unwrap()
        .actions
        .is_empty());
    h.authorise_engine_discard(&prepared, &control()).unwrap();
    call(&h, Command::Relock {});
    assert!(matches!(prepared.remove(&control()), Err(Error::Cancelled)));
    assert!(models.join(format!("{id}.part")).exists());
    h.close().unwrap();
}
#[test]
fn admission_and_foreign_host_publication_are_bounded_and_file_neutral() {
    let (root, h, id) = fixture(1);
    let models = root.path().join("models");
    let plan = h.engine_discard_plan(id, true, None).unwrap();
    assert!(matches!(
        h.engine_discard_plan(id, true, None),
        Err(Error::ResourceLimit)
    ));
    let prepared = plan.prepare(&models, &control()).unwrap();
    let (other_root, other, _) = fixture(8);
    assert!(other
        .authorise_engine_discard(&prepared, &control())
        .is_err());
    assert!(h
        .engine_resource_actions(None, 8)
        .unwrap()
        .actions
        .is_empty());
    assert!(matches!(
        h.engine_resource_actions(None, 0),
        Err(Error::ResourceLimit)
    ));
    assert!(matches!(prepared.remove(&control()), Err(Error::Denied)));
    assert!(models.join(format!("{id}.part")).exists());
    h.close().unwrap();
    other.close().unwrap();
    drop(other_root);
}

#[test]
fn action_history_and_new_plans_require_current_app_authority() {
    let (root, h, id) = fixture(8);
    let prepared = h
        .engine_discard_plan(id, true, None)
        .unwrap()
        .prepare(&root.path().join("models"), &control())
        .unwrap();
    let action = h.authorise_engine_discard(&prepared, &control()).unwrap();
    struct SyntheticAuth;
    impl stem_core::security::Authenticator for SyntheticAuth {
        fn verify(
            &mut self,
            _: Id,
            _: &std::collections::BTreeSet<stem_core::security::Scope>,
        ) -> bool {
            true
        }
    }
    h.authenticate(
        [stem_core::security::Scope::App].into_iter().collect(),
        Box::new(SyntheticAuth),
    )
    .unwrap();
    call(&h, Command::SetAppLock { enabled: true });
    call(&h, Command::Relock {});
    assert!(h.engine_resource_action(action.id).is_err());
    assert!(h.engine_resource_actions(None, 8).is_err());
    assert!(h.engine_discard_plan(id, true, Some(action.id)).is_err());
    assert!(matches!(prepared.remove(&control()), Err(Error::Cancelled)));
    assert!(root.path().join(format!("models/{id}.part")).exists());
    h.close().unwrap();
}
