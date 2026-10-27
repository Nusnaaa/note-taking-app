mod common;
use std::{io::Cursor, time::Duration};
use stem_core::{
    processing::*,
    provisioning::{resource_digest, Transfer},
    security::{Access, Scope},
    store::*,
    Error, Id,
};
fn descriptor(bytes: &[u8]) -> Descriptor {
    Descriptor {
        id: "synthetic-import".into(),
        version: "1".into(),
        adapter_version: "1".into(),
        family: Family::Recogniser,
        input: InputContract::OrderedStrokesV1,
        tasks: vec![Task::Prose],
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some("app-owned synthetic import fixture; no real model".into()),
        redistributable_resources: false,
        resource_sha256: Some(resource_digest(bytes)),
        runtime_contract: "stem-processing-v1".into(),
    }
}
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(30)).unwrap()
}

fn imported(s: &Store, root: &std::path::Path, complete: bool) -> Id {
    let mut transfer = s
        .engine_import_plan(descriptor(b"model"), 5, true, &Access::public())
        .unwrap()
        .begin(root, &control())
        .unwrap();
    let id = transfer.id();
    transfer
        .append(0, if complete { b"model" } else { b"mo" }, &control())
        .unwrap();
    if complete {
        drop(transfer.finish(&control()).unwrap());
    }
    id
}
#[test]
fn discard_records_original_consent_before_delete_and_preserves_notebook_content() {
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let note = s
        .create(Kind::Note, None, "independent notebook", &a)
        .unwrap();
    let block = s.append_ink(note.id, vec![common::stroke(0)], &a).unwrap();
    let original_ink = s.source_ink(block.id, &a).unwrap();
    let id = imported(&s, root.path(), true);
    let ledger: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.path().join(format!("{id}.install.json"))).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        s.engine_discard_plan(id, false, None, &a),
        Err(Error::Denied)
    ));
    let prepared = s
        .engine_discard_plan(id, true, None, &a)
        .unwrap()
        .prepare(root.path(), &control())
        .unwrap();
    let action = prepared.action().id;
    assert!(matches!(
        Transfer::resume(root.path(), id),
        Err(Error::ResourceLimit)
    ));
    let permit = s
        .authorise_engine_discard(&prepared, &control(), &a)
        .unwrap();
    let pending = s.engine_resource_action(action, &a).unwrap();
    assert_eq!(pending.state, ResourceActionState::Pending);
    assert_eq!(
        Some(pending.import_consent_ms),
        ledger["binding"]["consent_ms"].as_i64()
    );
    assert!(root.path().join(format!("{id}.resource")).exists());
    let removed = prepared.remove(permit, &control()).unwrap();
    for suffix in ["part", "resource", "install.json"] {
        assert!(!root.path().join(format!("{id}.{suffix}")).exists());
    }
    assert!(root.path().join(format!("{id}.owner.lock")).exists());
    assert_eq!(
        s.complete_engine_discard(&removed, &a).unwrap().state,
        ResourceActionState::Completed
    );
    assert_eq!(
        s.engine_resource_action(action, &a).unwrap().descriptor,
        descriptor(b"model")
    );
    assert_eq!(s.entity(note.id, &a).unwrap().title, "independent notebook");
    assert!(s.engine_catalogue(None, 8, &a).unwrap().versions.is_empty());
    assert_eq!(s.source_ink(block.id, &a).unwrap(), original_ink);
}
#[test]
fn publication_and_sql_failure_prevent_deletion_and_keep_history_atomic() {
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let id = imported(&s, root.path(), false);
    let prepared = s
        .engine_discard_plan(id, true, None, &a)
        .unwrap()
        .prepare(root.path(), &control())
        .unwrap();
    let action = prepared.action().id;
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s
        .authorise_engine_discard(&prepared, &control(), &a)
        .is_err());
    assert!(matches!(
        s.engine_resource_action(action, &a),
        Err(Error::Unavailable)
    ));
    assert!(root.path().join(format!("{id}.part")).exists());
    drop(prepared);
    let transfer = s
        .engine_import_plan(descriptor(b"model"), 5, true, &a)
        .unwrap()
        .resume(root.path(), id, &control())
        .unwrap();
    assert_eq!(transfer.committed_bytes(), 2);
    drop(transfer);
    let plan = s.engine_discard_plan(id, true, None, &a).unwrap();
    let before_publication = s.engine_discard_plan(id, true, None, &a).unwrap();
    let mut transfer = s
        .engine_import_plan(descriptor(b"model"), 5, true, &a)
        .unwrap()
        .resume(root.path(), id, &control())
        .unwrap();
    transfer
        .copy_remaining(&mut Cursor::new(b"del"), &control())
        .unwrap();
    let publication = transfer.finish(&control()).unwrap();
    assert!(matches!(
        plan.prepare(root.path(), &control()),
        Err(Error::ResourceLimit)
    ));
    s.commit_engine_import(&publication, &a).unwrap();
    drop(publication);
    let stale = before_publication.prepare(root.path(), &control()).unwrap();
    assert!(matches!(
        s.authorise_engine_discard(&stale, &control(), &a),
        Err(Error::Conflict)
    ));
    assert!(s
        .engine_resource_actions(None, 8, &a)
        .unwrap()
        .actions
        .is_empty());
    drop(stale);
    assert!(matches!(
        s.engine_discard_plan(id, true, None, &a),
        Err(Error::Conflict)
    ));
    assert!(root.path().join(format!("{id}.resource")).exists());
}
#[test]
fn cancelled_and_foreign_authorisation_never_delete_and_retry_can_finish_after_lost_completion() {
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let id = imported(&s, root.path(), false);
    let prepared = s
        .engine_discard_plan(id, true, None, &a)
        .unwrap()
        .prepare(root.path(), &control())
        .unwrap();
    let action = prepared.action().id;
    let c = control();
    c.cancel();
    assert!(matches!(
        s.authorise_engine_discard(&prepared, &c, &a),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        Store::memory()
            .unwrap()
            .authorise_engine_discard(&prepared, &control(), &a),
        Err(Error::Conflict)
    ));
    let permit = s
        .authorise_engine_discard(&prepared, &control(), &a)
        .unwrap();
    let removed = prepared.remove(permit, &control()).unwrap();
    drop(removed);
    assert_eq!(
        s.engine_resource_action(action, &a).unwrap().state,
        ResourceActionState::Pending
    );
    let resumed = s
        .engine_discard_plan(id, true, Some(action), &a)
        .unwrap()
        .prepare(root.path(), &control())
        .unwrap();
    let permit = s
        .authorise_engine_discard(&resumed, &control(), &a)
        .unwrap();
    let removed = resumed.remove(permit, &control()).unwrap();
    assert_eq!(s.complete_engine_discard(&removed, &a).unwrap().id, action);
}
#[test]
fn discard_rejects_legacy_foreign_and_pending_version_cannot_publish_again() {
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let foreign = Store::memory().unwrap();
    let id = imported(&foreign, root.path(), false);
    assert!(matches!(
        s.engine_discard_plan(id, true, None, &a)
            .unwrap()
            .prepare(root.path(), &control()),
        Err(Error::Conflict)
    ));
    let legacy = Transfer::begin(root.path(), descriptor(b"model"), 5, true).unwrap();
    let legacy_id = legacy.id();
    drop(legacy);
    assert!(matches!(
        s.engine_discard_plan(legacy_id, true, None, &a)
            .unwrap()
            .prepare(root.path(), &control()),
        Err(Error::ReviewRequired)
    ));
    let id = imported(&s, root.path(), false);
    let prepared = s
        .engine_discard_plan(id, true, None, &a)
        .unwrap()
        .prepare(root.path(), &control())
        .unwrap();
    let _permit = s
        .authorise_engine_discard(&prepared, &control(), &a)
        .unwrap();
    drop(prepared);
    let mut transfer = s
        .engine_import_plan(descriptor(b"model"), 5, true, &a)
        .unwrap()
        .resume(root.path(), id, &control())
        .unwrap();
    transfer
        .copy_remaining(&mut Cursor::new(b"del"), &control())
        .unwrap();
    let publication = transfer.finish(&control()).unwrap();
    assert!(matches!(
        s.commit_engine_import(&publication, &a),
        Err(Error::Conflict)
    ));
    assert!(root
        .path()
        .join(format!("{legacy_id}.install.json"))
        .exists());
}
#[test]
fn resource_actions_require_fresh_app_access_and_page_without_paths() {
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    for _ in 0..3 {
        let id = imported(&s, root.path(), false);
        let prepared = s
            .engine_discard_plan(id, true, None, &a)
            .unwrap()
            .prepare(root.path(), &control())
            .unwrap();
        let _permit = s
            .authorise_engine_discard(&prepared, &control(), &a)
            .unwrap();
    }
    let first = s.engine_resource_actions(None, 2, &a).unwrap();
    assert_eq!(first.actions.len(), 2);
    assert!(first.next.is_some());
    let second = s.engine_resource_actions(first.next, 2, &a).unwrap();
    assert_eq!(second.actions.len(), 1);
    assert!(second.next.is_none());
    assert!(!serde_json::to_string(&first)
        .unwrap()
        .contains(&root.path().display().to_string()));
    let app = Access::authenticate(
        &mut common::TestAuth(true),
        [Scope::App].into_iter().collect(),
    )
    .unwrap();
    s.set_app_lock(true, &app).unwrap();
    assert!(matches!(
        s.engine_resource_actions(None, 2, &a),
        Err(Error::Denied)
    ));
    assert!(matches!(
        s.engine_resource_action(first.actions[0].id, &a),
        Err(Error::Denied)
    ));
}
#[cfg(unix)]
#[test]
fn symlink_payload_refuses_the_entire_discard_before_any_file_is_deleted() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let s = Store::memory().unwrap();
    let a = Access::public();
    let id = imported(&s, root.path(), false);
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(outside.path(), b"outside bytes").unwrap();
    symlink(outside.path(), root.path().join(format!("{id}.resource"))).unwrap();
    assert!(matches!(
        s.engine_discard_plan(id, true, None, &a)
            .unwrap()
            .prepare(root.path(), &control()),
        Err(Error::ReviewRequired)
    ));
    assert_eq!(std::fs::read(outside.path()).unwrap(), b"outside bytes");
    assert_eq!(
        std::fs::read(root.path().join(format!("{id}.part"))).unwrap(),
        b"mo"
    );
    assert!(s
        .engine_resource_actions(None, 8, &a)
        .unwrap()
        .actions
        .is_empty());
}
#[test]
fn cancellation_after_authorisation_preserves_files_and_retry_survives_reopen() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    let models = root.path().join("models");
    let mut s = Store::open(&path).unwrap();
    let a = Access::public();
    let id = imported(&s, &models, false);
    let prepared = s
        .engine_discard_plan(id, true, None, &a)
        .unwrap()
        .prepare(&models, &control())
        .unwrap();
    let action = prepared.action().id;
    let permit = s
        .authorise_engine_discard(&prepared, &control(), &a)
        .unwrap();
    let c = control();
    c.cancel();
    assert!(matches!(prepared.remove(permit, &c), Err(Error::Cancelled)));
    assert!(models.join(format!("{id}.part")).exists());
    drop(s);
    let mut s = Store::open(&path).unwrap();
    let prepared = s
        .engine_discard_plan(id, true, Some(action), &a)
        .unwrap()
        .prepare(&models, &control())
        .unwrap();
    let permit = s
        .authorise_engine_discard(&prepared, &control(), &a)
        .unwrap();
    let removed = prepared.remove(permit, &control()).unwrap();
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.complete_engine_discard(&removed, &a).is_err());
    drop(removed);
    drop(s);
    let mut s = Store::open(&path).unwrap();
    assert_eq!(
        s.engine_resource_action(action, &a).unwrap().state,
        ResourceActionState::Pending
    );
    let prepared = s
        .engine_discard_plan(id, true, Some(action), &a)
        .unwrap()
        .prepare(&models, &control())
        .unwrap();
    let permit = s
        .authorise_engine_discard(&prepared, &control(), &a)
        .unwrap();
    let removed = prepared.remove(permit, &control()).unwrap();
    assert_eq!(
        s.complete_engine_discard(&removed, &a).unwrap().state,
        ResourceActionState::Completed
    );
}
#[test]
fn schema16_migrates_without_changing_notebooks_and_both_backups_preserve_action_history() {
    use stem_core::{
        backup::BackupOptions,
        crypto::{self, Credential},
    };
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    let models = root.path().join("models");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let note = s
        .create(Kind::Note, None, "retained schema16 note", &a)
        .unwrap();
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM engine_reclamations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    common::remove_linked_history_extension(&db);
    db.execute_batch("DROP TABLE engine_reclamations; DROP TABLE engine_resource_actions; PRAGMA user_version=16;")
        .unwrap();
    drop(db);
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 23);
    assert_eq!(
        s.entity(note.id, &a).unwrap().title,
        "retained schema16 note"
    );
    let id = imported(&s, &models, false);
    let prepared = s
        .engine_discard_plan(id, true, None, &a)
        .unwrap()
        .prepare(&models, &control())
        .unwrap();
    let action = prepared.action().id;
    let permit = s
        .authorise_engine_discard(&prepared, &control(), &a)
        .unwrap();
    drop(permit);
    drop(prepared);
    let expected = s.engine_resource_action(action, &a).unwrap();
    let password = "Explicit synthetic resource audit backup passphrase";
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
    let restored = Store::restore_backup_password(&backup.bytes, password).unwrap();
    assert_eq!(
        restored.engine_resource_action(action, &a).unwrap(),
        expected
    );
    assert!(matches!(
        restored.engine_discard_plan(id, true, Some(action), &a),
        Err(Error::Conflict)
    ));
    let plain = crypto::decrypt(&backup.bytes, Credential::Password(password)).unwrap();
    let mut document: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    document["tables"]["engine_resource_actions"][0][2]["value"] = "{bad action".into();
    let corrupt = crypto::encrypt(&serde_json::to_vec(&document).unwrap(), password).unwrap();
    assert!(Store::restore_backup_password(&corrupt.bytes, password).is_err());
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
        restored.store().engine_resource_action(action, &a).unwrap(),
        expected
    );
}
#[test]
fn authorisation_permit_cannot_be_rebound_to_a_different_preparation_of_the_same_record() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let id = imported(&s, first.path(), false);
    let prepared = s
        .engine_discard_plan(id, true, None, &a)
        .unwrap()
        .prepare(first.path(), &control())
        .unwrap();
    let action = prepared.action().id;
    let permit = s
        .authorise_engine_discard(&prepared, &control(), &a)
        .unwrap();
    for suffix in ["part", "install.json"] {
        std::fs::copy(
            first.path().join(format!("{id}.{suffix}")),
            second.path().join(format!("{id}.{suffix}")),
        )
        .unwrap();
    }
    let other = s
        .engine_discard_plan(id, true, Some(action), &a)
        .unwrap()
        .prepare(second.path(), &control())
        .unwrap();
    assert_eq!(other.action(), prepared.action());
    assert!(matches!(
        other.remove(permit, &control()),
        Err(Error::Conflict)
    ));
    assert_eq!(
        std::fs::read(second.path().join(format!("{id}.part"))).unwrap(),
        b"mo"
    );
    assert!(first.path().join(format!("{id}.install.json")).exists());
}
