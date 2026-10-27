mod common;
use std::{path::PathBuf, time::Duration};
use stem_core::backup::BackupOptions;
use stem_core::{
    processing::*, provisioning::resource_digest, security::Access, store::*, Error, Id,
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
        id: "synthetic-reclamation".into(),
        version: "1".into(),
        adapter_version: "1".into(),
        family: Family::Recogniser,
        input: InputContract::OrderedStrokesV1,
        tasks: vec![Task::Prose],
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some("app-owned synthetic reclamation; no real SDK".into()),
        redistributable_resources: false,
        resource_sha256: Some(resource_digest(b"model")),
        runtime_contract: "stem-processing-v1".into(),
    }
}
fn fixture(bound: bool) -> (tempfile::TempDir, Store, Id, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let v = if bound {
        let mut transfer = s
            .engine_import_plan(descriptor(), 5, true, &a)
            .unwrap()
            .begin(root.path(), &control())
            .unwrap();
        transfer.append(0, b"model", &control()).unwrap();
        let prepared = transfer.finish(&control()).unwrap();
        s.commit_engine_import(&prepared, &a).unwrap()
    } else {
        s.stage_engine(
            descriptor(),
            Some(&mut std::io::Cursor::new(b"model")),
            root.path(),
            5,
            true,
            &a,
        )
        .unwrap()
    };
    let plan = s.engine_removal_plan(v.id, &a).unwrap();
    drop(
        s.commit_engine_removal(&plan, &mut Manager::new(), &control(), &a)
            .unwrap(),
    );
    let path = PathBuf::from(v.local_resource.unwrap());
    (root, s, v.id, path)
}
#[test]
fn reviewed_consent_is_durable_before_known_files_are_removed_and_ink_is_independent() {
    let (root, mut s, id, path) = fixture(true);
    let a = Access::public();
    let note = s
        .create(Kind::Note, None, "independent source", &a)
        .unwrap();
    let block = s.append_ink(note.id, vec![common::stroke(0)], &a).unwrap();
    let ink = s.source_ink(block.id, &a).unwrap();
    let plan = s.engine_reclamation_plan(id, None, &a).unwrap();
    let action = plan.id();
    let review = plan.review(root.path(), &control()).unwrap();
    assert_eq!(
        review.summary().facts.consent_provenance,
        ConsentProvenance::BoundLedger
    );
    assert!(!review.summary().facts.binding_review_required);
    assert!(review.summary().facts.original_import_consent_ms.is_some());
    let prepared = review.prepare(consent(), &control()).unwrap();
    let permit = s
        .authorise_engine_reclamation(&prepared, &control(), &a)
        .unwrap();
    assert!(path.exists());
    let pending = s.engine_reclamation(action, &a).unwrap();
    assert_eq!(pending.state, ResourceActionState::Pending);
    let removed = prepared.remove(permit, &control()).unwrap();
    assert!(!path.exists());
    assert!(!root.path().join(format!("{id}.part")).exists());
    assert!(!root.path().join(format!("{id}.install.json")).exists());
    assert!(root.path().join(format!("{id}.owner.lock")).exists());
    let done = s.complete_engine_reclamation(&removed, &a).unwrap();
    assert_eq!(done.state, ResourceActionState::Completed);
    assert_eq!(s.complete_engine_reclamation(&removed, &a).unwrap(), done);
    assert_eq!(s.entity(note.id, &a).unwrap().title, "independent source");
    assert_eq!(s.source_ink(block.id, &a).unwrap(), ink);
    assert_eq!(
        s.engine_version(id, &a).unwrap().state,
        Installation::Removed
    );
    assert!(!serde_json::to_string(&done)
        .unwrap()
        .contains(root.path().to_str().unwrap()));
}
#[test]
fn legacy_binding_is_reviewed_without_inventing_original_consent() {
    let (root, mut s, id, path) = fixture(false);
    let a = Access::public();
    let review = s
        .engine_reclamation_plan(id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap();
    assert!(review.summary().facts.binding_review_required);
    assert!(review.summary().facts.original_import_consent_ms.is_none());
    assert!(matches!(
        review.prepare(consent(), &control()),
        Err(Error::ReviewRequired)
    ));
    assert!(path.exists());
    assert!(s
        .engine_reclamations(None, 8, &a)
        .unwrap()
        .actions
        .is_empty());
    let mut confirm = consent();
    confirm.reviewed_unknown_binding = true;
    let prepared = s
        .engine_reclamation_plan(id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap()
        .prepare(confirm, &control())
        .unwrap();
    let permit = s
        .authorise_engine_reclamation(&prepared, &control(), &a)
        .unwrap();
    let removed = prepared.remove(permit, &control()).unwrap();
    let done = s.complete_engine_reclamation(&removed, &a).unwrap();
    assert!(done.reviews[0].facts.original_import_consent_ms.is_none());
    assert!(done.reviews[0].consent.reviewed_unknown_binding);
}
#[test]
fn lost_completion_is_explicitly_retried_with_an_additional_durable_review() {
    let (root, mut s, id, path) = fixture(true);
    let a = Access::public();
    let prepared = s
        .engine_reclamation_plan(id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap()
        .prepare(consent(), &control())
        .unwrap();
    let action = prepared.action().id;
    let original = prepared.action().reviews[0]
        .facts
        .original_import_consent_ms;
    let permit = s
        .authorise_engine_reclamation(&prepared, &control(), &a)
        .unwrap();
    drop(prepared.remove(permit, &control()).unwrap());
    assert!(!path.exists());
    assert_eq!(
        s.engine_reclamation(action, &a).unwrap().state,
        ResourceActionState::Pending
    );
    let review = s
        .engine_reclamation_plan(id, Some(action), &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap();
    assert_eq!(
        review.summary().facts.consent_provenance,
        ConsentProvenance::RecordedDecision
    );
    assert_eq!(review.summary().facts.original_import_consent_ms, original);
    let prepared = review.prepare(consent(), &control()).unwrap();
    let permit = s
        .authorise_engine_reclamation(&prepared, &control(), &a)
        .unwrap();
    assert_eq!(s.engine_reclamation(action, &a).unwrap().reviews.len(), 2);
    let removed = prepared.remove(permit, &control()).unwrap();
    assert_eq!(
        s.complete_engine_reclamation(&removed, &a)
            .unwrap()
            .reviews
            .len(),
        2
    );
}
#[test]
fn sql_failure_and_cancellation_do_not_delete_files_and_preparation_keys_cannot_be_rebound() {
    let (root, mut s, id, path) = fixture(true);
    let a = Access::public();
    let prepared = s
        .engine_reclamation_plan(id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap()
        .prepare(consent(), &control())
        .unwrap();
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s
        .authorise_engine_reclamation(&prepared, &control(), &a)
        .is_err());
    assert!(path.exists());
    assert!(s
        .engine_reclamations(None, 8, &a)
        .unwrap()
        .actions
        .is_empty());
    let permit = s
        .authorise_engine_reclamation(&prepared, &control(), &a)
        .unwrap();
    drop(prepared);
    let second = s
        .engine_reclamation_plan(id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap()
        .prepare(consent(), &control())
        .unwrap();
    assert!(matches!(
        second.remove(permit, &control()),
        Err(Error::Conflict)
    ));
    assert!(path.exists());
    let prepared = s
        .engine_reclamation_plan(id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap()
        .prepare(consent(), &control())
        .unwrap();
    let permit = s
        .authorise_engine_reclamation(&prepared, &control(), &a)
        .unwrap();
    let cancel = control();
    cancel.cancel();
    assert!(matches!(
        prepared.remove(permit, &cancel),
        Err(Error::Cancelled)
    ));
    assert!(path.exists());
}
#[test]
fn review_directory_replacement_and_file_changes_require_fresh_review() {
    let (root, s, id, path) = fixture(true);
    let a = Access::public();
    let review = s
        .engine_reclamation_plan(id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap();
    std::fs::write(&path, b"changed bytes").unwrap();
    assert!(matches!(
        review.prepare(consent(), &control()),
        Err(Error::Conflict)
    ));
    let parent = tempfile::tempdir().unwrap();
    let moved = parent.path().join("moved");
    let review = s
        .engine_reclamation_plan(id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap();
    std::fs::rename(root.path(), &moved).unwrap();
    std::fs::create_dir(root.path()).unwrap();
    assert!(matches!(
        review.prepare(consent(), &control()),
        Err(Error::Conflict)
    ));
    assert_eq!(
        std::fs::read(moved.join(format!("{id}.resource"))).unwrap(),
        b"changed bytes"
    );
}
#[cfg(unix)]
#[test]
fn explicit_symlink_review_unlinks_only_the_known_link_and_preserves_its_target() {
    let (root, mut s, id, path) = fixture(true);
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(outside.path(), b"independent private bytes").unwrap();
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(outside.path(), &path).unwrap();
    let a = Access::public();
    let review = s
        .engine_reclamation_plan(id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap();
    assert_eq!(
        review.summary().facts.resource.kind,
        ResourceEntryKind::Symlink
    );
    assert!(matches!(
        review.prepare(consent(), &control()),
        Err(Error::ReviewRequired)
    ));
    let mut confirm = consent();
    confirm.reviewed_symlinks = true;
    let prepared = s
        .engine_reclamation_plan(id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap()
        .prepare(confirm, &control())
        .unwrap();
    let permit = s
        .authorise_engine_reclamation(&prepared, &control(), &a)
        .unwrap();
    let removed = prepared.remove(permit, &control()).unwrap();
    s.complete_engine_reclamation(&removed, &a).unwrap();
    assert_eq!(
        std::fs::read(outside.path()).unwrap(),
        b"independent private bytes"
    );
    assert!(std::fs::symlink_metadata(&path).is_err());
}

#[test]
fn installed_or_staged_versions_refuse_reclamation_and_review_flags_never_bypass_a_live_sdk_pin() {
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
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let v = s
        .stage_engine(
            descriptor(),
            Some(&mut std::io::Cursor::new(b"model")),
            root.path(),
            5,
            true,
            &a,
        )
        .unwrap();
    assert!(s.engine_reclamation_plan(v.id, None, &a).is_err());
    let mut manager = Manager::new();
    s.activate_engine(v.id, &mut manager, Box::new(Model), 5, &a)
        .unwrap();
    assert!(s.engine_reclamation_plan(v.id, None, &a).is_err());
    let removal = s.engine_removal_plan(v.id, &a).unwrap();
    let detached = s
        .commit_engine_removal(&removal, &mut manager, &control(), &a)
        .unwrap();
    assert!(detached.is_some());
    let mut confirm = consent();
    confirm.reviewed_unknown_binding = true;
    confirm.reviewed_failed_retirement = true;
    confirm.reviewed_symlinks = true;
    let review = s
        .engine_reclamation_plan(v.id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap();
    assert!(matches!(
        review.prepare(confirm, &control()),
        Err(Error::ResourceLimit)
    ));
    assert!(s
        .engine_reclamations(None, 8, &a)
        .unwrap()
        .actions
        .is_empty());
    drop(detached);
    let prepared = s
        .engine_reclamation_plan(v.id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap()
        .prepare(confirm, &control())
        .unwrap();
    let permit = s
        .authorise_engine_reclamation(&prepared, &control(), &a)
        .unwrap();
    let removed = prepared.remove(permit, &control()).unwrap();
    s.complete_engine_reclamation(&removed, &a).unwrap();
}
#[test]
fn persisted_uncertain_retirement_requires_explicit_review_and_is_never_silently_reset() {
    let (root, mut s, id, path) = fixture(true);
    let a = Access::public();
    let marker = root.path().join(format!(
        ".stem-resource-{}.lock",
        resource_digest(path.file_name().unwrap().as_encoded_bytes())
    ));
    std::fs::write(&marker, b"prior process retirement uncertain").unwrap();
    let review = s
        .engine_reclamation_plan(id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap();
    assert!(review.summary().facts.retirement_review_required);
    assert!(matches!(
        review.prepare(consent(), &control()),
        Err(Error::ReviewRequired)
    ));
    assert!(path.exists());
    let mut confirm = consent();
    confirm.reviewed_failed_retirement = true;
    let prepared = s
        .engine_reclamation_plan(id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap()
        .prepare(confirm, &control())
        .unwrap();
    let permit = s
        .authorise_engine_reclamation(&prepared, &control(), &a)
        .unwrap();
    let removed = prepared.remove(permit, &control()).unwrap();
    let done = s.complete_engine_reclamation(&removed, &a).unwrap();
    assert!(done.reviews[0].facts.retirement_review_required);
    assert!(done.reviews[0].consent.reviewed_failed_retirement);
    assert_eq!(
        std::fs::read(marker).unwrap(),
        b"prior process retirement uncertain"
    );
}
#[test]
fn foreign_ledger_and_fresh_app_access_cannot_be_overridden_by_review_flags() {
    let (root, mut s, id, path) = fixture(true);
    let a = Access::public();
    let ledger = root.path().join(format!("{id}.install.json"));
    let before = std::fs::read(&ledger).unwrap();
    let mut state: serde_json::Value = serde_json::from_slice(&before).unwrap();
    state["binding"]["library"] = Id::new().to_string().into();
    std::fs::write(&ledger, serde_json::to_vec(&state).unwrap()).unwrap();
    assert!(matches!(
        s.engine_reclamation_plan(id, None, &a)
            .unwrap()
            .review(root.path(), &control()),
        Err(Error::Conflict)
    ));
    assert!(path.exists());
    std::fs::write(&ledger, &before).unwrap();
    let prepared = s
        .engine_reclamation_plan(id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap()
        .prepare(consent(), &control())
        .unwrap();
    let auth = Access::authenticate(
        &mut common::TestAuth(true),
        [stem_core::security::Scope::App].into_iter().collect(),
    )
    .unwrap();
    s.set_app_lock(true, &auth).unwrap();
    assert!(s
        .authorise_engine_reclamation(&prepared, &control(), &a)
        .is_err());
    assert!(s.engine_reclamations(None, 8, &a).is_err());
    assert!(path.exists());
    assert!(s
        .engine_reclamations(None, 8, &auth)
        .unwrap()
        .actions
        .is_empty());
}
#[test]
fn schema18_classic_and_stream_backups_preserve_decisions_without_paths_or_deletion_authority() {
    use stem_core::crypto::{self, Credential};
    let (root, mut s, id, _) = fixture(true);
    let a = Access::public();
    let prepared = s
        .engine_reclamation_plan(id, None, &a)
        .unwrap()
        .review(root.path(), &control())
        .unwrap()
        .prepare(consent(), &control())
        .unwrap();
    let action = prepared.action().id;
    let permit = s
        .authorise_engine_reclamation(&prepared, &control(), &a)
        .unwrap();
    drop(permit);
    drop(prepared);
    let expected = s.engine_reclamation(action, &a).unwrap();
    let password = "Explicit synthetic model reclamation backup passphrase";
    let options = BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    };
    let backup = s
        .finish_backup(
            s.prepare_backup(options, &a).unwrap(),
            password,
            &mut common::TestAuth(true),
            true,
        )
        .unwrap();
    let plain = crypto::decrypt(&backup.bytes, Credential::Password(password)).unwrap();
    assert!(!String::from_utf8_lossy(&plain).contains(root.path().to_str().unwrap()));
    let mut json: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    assert_eq!(json["schema"], 23);
    let restored = Store::restore_backup_password(&backup.bytes, password).unwrap();
    assert_eq!(restored.engine_reclamation(action, &a).unwrap(), expected);
    assert!(restored
        .engine_reclamation_plan(id, Some(action), &a)
        .is_err());
    json["tables"]["engine_reclamations"][0][2]["value"] = "{invalid reclamation".into();
    let bad = crypto::encrypt(&serde_json::to_vec(&json).unwrap(), password).unwrap();
    assert!(Store::restore_backup_password(&bad.bytes, password).is_err());
    let c = JobControl::new(Duration::from_secs(300)).unwrap();
    let (stream, _) = s
        .finish_stream_backup(
            s.prepare_stream_backup(options, &a).unwrap(),
            password,
            &mut common::TestAuth(true),
            true,
            Vec::new(),
            &c,
        )
        .unwrap();
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let stage = builder.tempdir().unwrap();
    let restored = Store::restore_stream_backup(
        stream.as_slice(),
        Credential::Password(password),
        100_000_000,
        stage.path(),
        &c,
    )
    .unwrap();
    assert_eq!(
        restored.store().engine_reclamation(action, &a).unwrap(),
        expected
    );
}
#[test]
fn real_schema17_layout_migrates_and_legacy_backup_tag_cannot_smuggle_reclamations() {
    use stem_core::crypto::{self, Credential};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    let mut s = Store::open(&path).unwrap();
    let a = Access::public();
    let note = s.create(Kind::Note, None, "schema17 source", &a).unwrap();
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM engine_reclamations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    common::remove_linked_history_extension(&db);
    db.execute_batch("DROP TABLE engine_reclamations; PRAGMA user_version=17;")
        .unwrap();
    drop(db);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 23);
    assert_eq!(s.entity(note.id, &a).unwrap().title, "schema17 source");
    let password = "Explicit schema17 compatibility backup passphrase";
    let options = BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    };
    let backup = s
        .finish_backup(
            s.prepare_backup(options, &a).unwrap(),
            password,
            &mut common::TestAuth(true),
            true,
        )
        .unwrap();
    let plain = crypto::decrypt(&backup.bytes, Credential::Password(password)).unwrap();
    let mut old: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    old["schema"] = 17.into();
    let smuggled = crypto::encrypt(&serde_json::to_vec(&old).unwrap(), password).unwrap();
    assert!(Store::restore_backup_password(&smuggled.bytes, password).is_err());
    old["tables"]
        .as_object_mut()
        .unwrap()
        .remove("engine_reclamations");
    let valid = crypto::encrypt(&serde_json::to_vec(&old).unwrap(), password).unwrap();
    let restored = Store::restore_backup_password(&valid.bytes, password).unwrap();
    assert_eq!(
        restored.entity(note.id, &a).unwrap().title,
        "schema17 source"
    );
}
