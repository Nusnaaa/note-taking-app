use std::{sync::Arc, time::Duration};
use stem_core::{
    host::{Command, Host, Request},
    processing::*,
    provisioning::{resource_digest, ResourceLeases},
    security::Access,
    store::*,
    Error, Id,
};
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(30)).unwrap()
}
fn consent() -> ReclamationConsent {
    ReclamationConsent {
        confirmed: true,
        reviewed_unknown_binding: false,
        reviewed_failed_retirement: false,
        reviewed_symlinks: false,
    }
}
fn descriptor() -> Descriptor {
    Descriptor {
        id: "synthetic-host-reclamation".into(),
        version: "1".into(),
        adapter_version: "1".into(),
        family: Family::Recogniser,
        input: InputContract::OrderedStrokesV1,
        tasks: vec![Task::Prose],
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some("app-owned Host lifecycle fixture; no real SDK".into()),
        redistributable_resources: false,
        resource_sha256: Some(resource_digest(b"model")),
        runtime_contract: "stem-processing-v1".into(),
    }
}
fn fixture(capacity: usize) -> (tempfile::TempDir, Arc<Host>, Id) {
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::open(root.path().join("library.db")).unwrap();
    let models = root.path().join("models");
    let a = Access::public();
    let mut transfer = s
        .engine_import_plan(descriptor(), 5, true, &a)
        .unwrap()
        .begin(&models, &control())
        .unwrap();
    transfer.append(0, b"model", &control()).unwrap();
    let prepared = transfer.finish(&control()).unwrap();
    let v = s.commit_engine_import(&prepared, &a).unwrap();
    drop(prepared);
    let plan = s.engine_removal_plan(v.id, &a).unwrap();
    drop(
        s.commit_engine_removal(&plan, &mut Manager::new(), &control(), &a)
            .unwrap(),
    );
    drop(s);
    let h = Arc::new(Host::open(root.path().join("library.db"), capacity).unwrap());
    (root, h, v.id)
}
fn call(h: &Host, command: Command) {
    let r = h
        .request(
            &serde_json::to_string(&Request {
                version: 1,
                command,
            })
            .unwrap(),
        )
        .unwrap();
    assert!(r.ok, "{:?}", r.error);
}
#[test]
fn review_defers_exclusivity_and_writer_consent_precedes_deletion_with_idempotent_completion() {
    let (root, h, id) = fixture(8);
    let models = root.path().join("models");
    let path = models.join(format!("{id}.resource"));
    let plan = h.engine_reclamation_plan(id, None).unwrap();
    let action = plan.action_id();
    let review = plan.review(&models, &control()).unwrap();
    assert_eq!(review.summary().action, action);
    let reader = ResourceLeases::default()
        .acquire(&path, &resource_digest(b"model"))
        .unwrap();
    drop(reader);
    call(
        &h,
        Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "authoring during review".into(),
        },
    );
    let prepared = review.prepare(consent(), &control()).unwrap();
    assert_eq!(prepared.action_id(), action);
    assert!(path.exists());
    let pending = h
        .authorise_engine_reclamation(&prepared, &control())
        .unwrap();
    assert_eq!(pending.state, ResourceActionState::Pending);
    assert_eq!(h.engine_reclamation(action).unwrap(), pending);
    assert!(path.exists());
    let removed = prepared.remove(&control()).unwrap();
    assert_eq!(removed.action_id(), action);
    assert!(!path.exists());
    let done = h.complete_engine_reclamation(&removed, &control()).unwrap();
    assert_eq!(done.state, ResourceActionState::Completed);
    assert_eq!(
        h.complete_engine_reclamation(&removed, &control()).unwrap(),
        done
    );
    assert_eq!(h.engine_reclamations(None, 8).unwrap().actions, vec![done]);
    drop(removed);
    h.close().unwrap();
}
#[test]
fn capture_invalidates_a_waiting_review_and_never_revives_it_at_pen_up() {
    let (root, h, id) = fixture(8);
    let models = root.path().join("models");
    let review = h
        .engine_reclamation_plan(id, None)
        .unwrap()
        .review(&models, &control())
        .unwrap();
    let pen = Id::new();
    h.set_capture_priority(pen, true).unwrap();
    assert!(matches!(
        h.engine_reclamation_plan(id, None),
        Err(Error::ResourceLimit)
    ));
    h.set_capture_priority(pen, false).unwrap();
    assert!(matches!(
        review.prepare(consent(), &control()),
        Err(Error::Cancelled)
    ));
    assert!(models.join(format!("{id}.resource")).exists());
    assert!(h.engine_reclamations(None, 8).unwrap().actions.is_empty());
    h.close().unwrap();
}
#[test]
fn relock_after_pending_cancels_deletion_and_explicit_retry_appends_new_consent() {
    let (root, h, id) = fixture(8);
    let models = root.path().join("models");
    let prepared = h
        .engine_reclamation_plan(id, None)
        .unwrap()
        .review(&models, &control())
        .unwrap()
        .prepare(consent(), &control())
        .unwrap();
    let pending = h
        .authorise_engine_reclamation(&prepared, &control())
        .unwrap();
    call(&h, Command::Relock {});
    assert!(matches!(prepared.remove(&control()), Err(Error::Cancelled)));
    assert!(models.join(format!("{id}.resource")).exists());
    h.close().unwrap();
    let h = Host::open(root.path().join("library.db"), 8).unwrap();
    let prepared = h
        .engine_reclamation_plan(id, Some(pending.id))
        .unwrap()
        .review(&models, &control())
        .unwrap()
        .prepare(consent(), &control())
        .unwrap();
    let updated = h
        .authorise_engine_reclamation(&prepared, &control())
        .unwrap();
    assert_eq!(updated.id, pending.id);
    assert_eq!(updated.reviews.len(), 2);
    let removed = prepared.remove(&control()).unwrap();
    assert_eq!(
        h.complete_engine_reclamation(&removed, &control())
            .unwrap()
            .state,
        ResourceActionState::Completed
    );
    drop(removed);
    h.close().unwrap();
}
#[test]
fn lost_completion_recovers_after_close_while_old_result_handle_survives() {
    let (root, h, id) = fixture(8);
    let models = root.path().join("models");
    let prepared = h
        .engine_reclamation_plan(id, None)
        .unwrap()
        .review(&models, &control())
        .unwrap()
        .prepare(consent(), &control())
        .unwrap();
    let pending = h
        .authorise_engine_reclamation(&prepared, &control())
        .unwrap();
    let removed = prepared.remove(&control()).unwrap();
    let cancelled = control();
    cancelled.cancel();
    assert!(matches!(
        h.complete_engine_reclamation(&removed, &cancelled),
        Err(Error::Cancelled)
    ));
    assert_eq!(
        h.engine_reclamation(pending.id).unwrap().state,
        ResourceActionState::Pending
    );
    h.close().unwrap();
    let next = Host::open(root.path().join("library.db"), 8).unwrap();
    assert!(h.complete_engine_reclamation(&removed, &control()).is_err());
    let review = next
        .engine_reclamation_plan(id, Some(pending.id))
        .unwrap()
        .review(&models, &control())
        .unwrap();
    assert_eq!(
        review.summary().facts.consent_provenance,
        ConsentProvenance::RecordedDecision
    );
    let prepared = review.prepare(consent(), &control()).unwrap();
    next.authorise_engine_reclamation(&prepared, &control())
        .unwrap();
    let retry = prepared.remove(&control()).unwrap();
    assert_eq!(
        next.complete_engine_reclamation(&retry, &control())
            .unwrap()
            .reviews
            .len(),
        2
    );
    drop(retry);
    drop(removed);
    next.close().unwrap();
}
#[test]
fn close_retires_review_directory_ownership_even_when_caller_retains_the_handle() {
    let (root, h, id) = fixture(8);
    let models = root.path().join("models");
    let review = h
        .engine_reclamation_plan(id, None)
        .unwrap()
        .review(&models, &control())
        .unwrap();
    h.close().unwrap();
    let next = Host::open(root.path().join("library.db"), 8).unwrap();
    assert!(matches!(
        review.prepare(consent(), &control()),
        Err(Error::Cancelled)
    ));
    let review = next
        .engine_reclamation_plan(id, None)
        .unwrap()
        .review(&models, &control())
        .unwrap();
    drop(review);
    next.close().unwrap();
    assert!(models.join(format!("{id}.resource")).exists());
}
#[test]
fn consent_parent_cancellation_foreign_host_and_capacity_fail_without_deletion() {
    let (root, h, id) = fixture(1);
    let models = root.path().join("models");
    let plan = h.engine_reclamation_plan(id, None).unwrap();
    assert!(matches!(
        h.engine_reclamation_plan(id, None),
        Err(Error::ResourceLimit)
    ));
    let review = plan.review(&models, &control()).unwrap();
    let mut denied = consent();
    denied.confirmed = false;
    assert!(matches!(
        review.prepare(denied, &control()),
        Err(Error::Denied)
    ));
    h.close().unwrap();
    let h = Host::open(root.path().join("library.db"), 8).unwrap();
    let review = h
        .engine_reclamation_plan(id, None)
        .unwrap()
        .review(&models, &control())
        .unwrap();
    let cancelled = control();
    cancelled.cancel();
    assert!(matches!(
        review.prepare(consent(), &cancelled),
        Err(Error::Cancelled)
    ));
    let prepared = h
        .engine_reclamation_plan(id, None)
        .unwrap()
        .review(&models, &control())
        .unwrap()
        .prepare(consent(), &control())
        .unwrap();
    let (_other_root, other, _) = fixture(8);
    assert!(other
        .authorise_engine_reclamation(&prepared, &control())
        .is_err());
    assert!(h.engine_reclamations(None, 8).unwrap().actions.is_empty());
    assert!(matches!(
        h.engine_reclamations(None, 0),
        Err(Error::ResourceLimit)
    ));
    drop(prepared);
    h.close().unwrap();
    other.close().unwrap();
    assert!(models.join(format!("{id}.resource")).exists());
}
#[test]
fn logical_removal_does_not_acknowledge_an_independently_retained_sdk_owner() {
    struct Model;
    impl Recogniser for Model {
        fn probe(&self) -> stem_core::Result<()> {
            Ok(())
        }
        fn recognise(
            &self,
            _: &InkSnapshot,
            _: Task,
            _: &JobControl,
        ) -> stem_core::Result<RecognitionOutput> {
            Err(Error::EngineUnavailable)
        }
    }
    let root = tempfile::tempdir().unwrap();
    let models = root.path().join("models");
    let mut s = Store::open(root.path().join("library.db")).unwrap();
    let a = Access::public();
    let v = s
        .stage_engine(
            descriptor(),
            Some(&mut std::io::Cursor::new(b"model")),
            &models,
            5,
            true,
            &a,
        )
        .unwrap();
    let mut manager = Manager::new();
    s.activate_engine(v.id, &mut manager, Box::new(Model), 5, &a)
        .unwrap();
    let plan = s.engine_removal_plan(v.id, &a).unwrap();
    let detached = s
        .commit_engine_removal(&plan, &mut manager, &control(), &a)
        .unwrap();
    drop(s);
    let h = Host::open(root.path().join("library.db"), 8).unwrap();
    let mut all = consent();
    all.reviewed_unknown_binding = true;
    all.reviewed_failed_retirement = true;
    all.reviewed_symlinks = true;
    let review = h
        .engine_reclamation_plan(v.id, None)
        .unwrap()
        .review(&models, &control())
        .unwrap();
    assert!(matches!(
        review.prepare(all, &control()),
        Err(Error::ResourceLimit)
    ));
    assert!(h.engine_reclamations(None, 8).unwrap().actions.is_empty());
    drop(detached);
    let prepared = h
        .engine_reclamation_plan(v.id, None)
        .unwrap()
        .review(&models, &control())
        .unwrap()
        .prepare(all, &control())
        .unwrap();
    h.authorise_engine_reclamation(&prepared, &control())
        .unwrap();
    let removed = prepared.remove(&control()).unwrap();
    h.complete_engine_reclamation(&removed, &control()).unwrap();
    drop(removed);
    h.close().unwrap();
}
#[test]
fn fresh_app_authority_gates_history_and_review_creation() {
    let (root, h, id) = fixture(8);
    let models = root.path().join("models");
    let review = h
        .engine_reclamation_plan(id, None)
        .unwrap()
        .review(&models, &control())
        .unwrap();
    struct Auth;
    impl stem_core::security::Authenticator for Auth {
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
        Box::new(Auth),
    )
    .unwrap();
    call(&h, Command::SetAppLock { enabled: true });
    call(&h, Command::Relock {});
    assert!(h.engine_reclamations(None, 8).is_err());
    assert!(h.engine_reclamation_plan(id, None).is_err());
    assert!(matches!(
        review.prepare(consent(), &control()),
        Err(Error::Cancelled)
    ));
    h.close().unwrap();
}
