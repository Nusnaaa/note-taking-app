use std::io::Cursor;
use std::time::Duration;
use stem_core::{
    processing::*,
    provisioning::{resource_digest, Transfer},
    security::Access,
    store::*,
    Error,
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
#[test]
fn plan_requires_consent_and_typed_eligibility_without_creating_files_or_metadata() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("not-created");
    let s = Store::memory().unwrap();
    let a = Access::public();
    let bytes = b"synthetic resource";
    assert!(matches!(
        s.engine_import_plan(descriptor(bytes), bytes.len() as u64, false, &a),
        Err(Error::Denied)
    ));
    let mut d = descriptor(bytes);
    d.input = InputContract::MandatoryRaster;
    assert!(matches!(
        s.engine_import_plan(d, bytes.len() as u64, true, &a),
        Err(Error::Unsupported)
    ));
    let _plan = s
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true, &a)
        .unwrap();
    assert!(!root.exists());
}
#[test]
fn import_records_bound_consent_and_stages_verified_resource_with_stable_retry() {
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let bytes = b"synthetic verified model fixture";
    let mut transfer = s
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true, &a)
        .unwrap()
        .begin(root.path(), &control())
        .unwrap();
    let id = transfer.id();
    transfer
        .copy_remaining(&mut Cursor::new(bytes), &control())
        .unwrap();
    assert_eq!(transfer.committed_bytes(), bytes.len() as u64);
    assert!(matches!(s.engine_version(id, &a), Err(Error::Unavailable)));
    let prepared = transfer.finish(&control()).unwrap();
    let ledger: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.path().join(format!("{id}.install.json"))).unwrap(),
    )
    .unwrap();
    assert!(!ledger["binding"]["library"].as_str().unwrap().is_empty());
    assert_eq!(
        ledger["binding"]["consent_ms"].as_i64(),
        Some(prepared.consent_ms())
    );
    let staged = s.commit_engine_import(&prepared, &a).unwrap();
    assert_eq!(staged.id, id);
    assert_eq!(staged.state, Installation::Staged);
    assert_eq!(
        std::fs::read(staged.local_resource.as_ref().unwrap()).unwrap(),
        bytes
    );
    assert!(s.active_engine("synthetic-import", &a).unwrap().is_none());
    let generation = s
        .prepare_stream_backup(
            stem_core::backup::BackupOptions {
                include_original_ink: false,
                include_authorised_source_assets: false,
            },
            &a,
        )
        .unwrap()
        .generation()
        .to_owned();
    assert_eq!(s.commit_engine_import(&prepared, &a).unwrap(), staged);
    assert_eq!(
        s.prepare_stream_backup(
            stem_core::backup::BackupOptions {
                include_original_ink: false,
                include_authorised_source_assets: false
            },
            &a
        )
        .unwrap()
        .generation()
        .to_owned(),
        generation
    );
}
#[test]
fn foreign_or_changed_resume_cannot_truncate_another_imports_unacknowledged_tail() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let bytes = b"synthetic resource for explicit resume";
    let mut transfer = s
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true, &a)
        .unwrap()
        .begin(root.path(), &control())
        .unwrap();
    let id = transfer.id();
    transfer.append(0, &bytes[..5], &control()).unwrap();
    drop(transfer);
    let path = root.path().join(format!("{id}.part"));
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"unacknowledged")
        .unwrap();
    let length = std::fs::metadata(&path).unwrap().len();
    let foreign = Store::memory().unwrap();
    assert!(matches!(
        foreign
            .engine_import_plan(descriptor(bytes), bytes.len() as u64, true, &a)
            .unwrap()
            .resume(root.path(), id, &control()),
        Err(Error::Conflict)
    ));
    assert_eq!(std::fs::metadata(&path).unwrap().len(), length);
    let mut changed = descriptor(bytes);
    changed.version = "other".into();
    assert!(matches!(
        s.engine_import_plan(changed, bytes.len() as u64, true, &a)
            .unwrap()
            .resume(root.path(), id, &control()),
        Err(Error::Conflict)
    ));
    assert_eq!(std::fs::metadata(&path).unwrap().len(), length);
    let mut resumed = s
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true, &a)
        .unwrap()
        .resume(root.path(), id, &control())
        .unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 5);
    resumed
        .copy_remaining(&mut Cursor::new(&bytes[5..]), &control())
        .unwrap();
    let prepared = resumed.finish(&control()).unwrap();
    assert_eq!(s.commit_engine_import(&prepared, &a).unwrap().id, id);
}
#[test]
fn legacy_transfer_requires_review_before_managed_resume_and_preserves_its_bytes() {
    let root = tempfile::tempdir().unwrap();
    let s = Store::memory().unwrap();
    let a = Access::public();
    let bytes = b"legacy synthetic resource";
    let mut legacy =
        Transfer::begin(root.path(), descriptor(bytes), bytes.len() as u64, true).unwrap();
    let id = legacy.id();
    legacy.append(0, &bytes[..4]).unwrap();
    drop(legacy);
    assert!(matches!(
        s.engine_import_plan(descriptor(bytes), bytes.len() as u64, true, &a)
            .unwrap()
            .resume(root.path(), id, &control()),
        Err(Error::ReviewRequired)
    ));
    assert_eq!(
        std::fs::read(root.path().join(format!("{id}.part"))).unwrap(),
        &bytes[..4]
    );
    assert_eq!(
        Transfer::resume(root.path(), id).unwrap().committed_bytes(),
        4
    );
}
#[test]
fn truncated_trailing_corrupt_and_cancelled_inputs_never_stage_metadata() {
    let root = tempfile::tempdir().unwrap();
    let s = Store::memory().unwrap();
    let a = Access::public();
    let bytes = b"expected resource";
    for input in [
        &bytes[..3],
        b"expected resource extra".as_slice(),
        b"different data!!!".as_slice(),
    ] {
        let mut transfer = s
            .engine_import_plan(descriptor(bytes), bytes.len() as u64, true, &a)
            .unwrap()
            .begin(root.path(), &control())
            .unwrap();
        let id = transfer.id();
        let copied = transfer.copy_remaining(&mut Cursor::new(input), &control());
        if copied.is_ok() {
            assert!(transfer.finish(&control()).is_err());
        }
        assert!(matches!(s.engine_version(id, &a), Err(Error::Unavailable)));
    }
    let mut transfer = s
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true, &a)
        .unwrap()
        .begin(root.path(), &control())
        .unwrap();
    let id = transfer.id();
    let c = control();
    c.cancel();
    assert!(matches!(
        transfer.copy_remaining(&mut Cursor::new(bytes), &c),
        Err(Error::Cancelled)
    ));
    assert_eq!(transfer.committed_bytes(), 0);
    assert!(root.path().join(format!("{id}.install.json")).exists());
}
#[test]
fn stale_generation_or_cancelled_commit_preserves_resumable_verified_resource() {
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let bytes = b"verified resumable resource";
    let mut transfer = s
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true, &a)
        .unwrap()
        .begin(root.path(), &control())
        .unwrap();
    let id = transfer.id();
    transfer
        .copy_remaining(&mut Cursor::new(bytes), &control())
        .unwrap();
    let prepared = transfer.finish(&control()).unwrap();
    s.create(Kind::Note, None, "concurrent authoring", &a)
        .unwrap();
    assert!(matches!(
        s.commit_engine_import(&prepared, &a),
        Err(Error::Conflict)
    ));
    assert!(std::path::Path::new(prepared.version().local_resource.as_ref().unwrap()).exists());
    drop(prepared); // Release the previous publication owner before resuming.
    let resumed = s
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true, &a)
        .unwrap()
        .resume(root.path(), id, &control())
        .unwrap();
    let c = control();
    let fresh = resumed.finish(&c).unwrap();
    c.cancel();
    assert!(matches!(
        s.commit_engine_import(&fresh, &a),
        Err(Error::Cancelled)
    ));
    drop(fresh); // A cancelled prepared publication still owns its transfer.
    let retry = s
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true, &a)
        .unwrap()
        .resume(root.path(), id, &control())
        .unwrap()
        .finish(&control())
        .unwrap();
    assert_eq!(s.commit_engine_import(&retry, &a).unwrap().id, id);
}

#[test]
fn staged_retry_reports_installed_metadata_but_never_revives_a_removed_version() {
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
    let bytes = b"idempotent resource fixture";
    let mut transfer = s
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true, &a)
        .unwrap()
        .begin(root.path(), &control())
        .unwrap();
    transfer
        .copy_remaining(&mut Cursor::new(bytes), &control())
        .unwrap();
    let prepared = transfer.finish(&control()).unwrap();
    let staged = s.commit_engine_import(&prepared, &a).unwrap();
    let mut manager = Manager::new();
    s.activate_engine(staged.id, &mut manager, Box::new(Model), 100, &a)
        .unwrap();
    assert_eq!(
        s.commit_engine_import(&prepared, &a).unwrap().state,
        Installation::Installed
    );
    s.remove_engine(staged.id, &mut manager, &a).unwrap();
    assert!(matches!(
        s.commit_engine_import(&prepared, &a),
        Err(Error::Conflict)
    ));
    assert_eq!(
        s.engine_version(staged.id, &a).unwrap().state,
        Installation::Removed
    );
}

#[test]
fn interrupted_reader_retries_data_and_final_eof_without_losing_durable_offset() {
    use std::io::{self, Read};
    struct Interrupted {
        input: Cursor<Vec<u8>>,
        calls: usize,
    }
    impl Read for Interrupted {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.calls += 1;
            if self.calls == 1 || self.calls == 3 {
                Err(io::ErrorKind::Interrupted.into())
            } else {
                self.input.read(buf)
            }
        }
    }
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let bytes = b"interrupted source read fixture";
    let mut transfer = s
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true, &a)
        .unwrap()
        .begin(root.path(), &control())
        .unwrap();
    let id = transfer.id();
    transfer
        .copy_remaining(
            &mut Interrupted {
                input: Cursor::new(bytes.to_vec()),
                calls: 0,
            },
            &control(),
        )
        .unwrap();
    assert_eq!(transfer.committed_bytes(), bytes.len() as u64);
    let prepared = transfer.finish(&control()).unwrap();
    assert_eq!(s.commit_engine_import(&prepared, &a).unwrap().id, id);
}

#[test]
fn prepared_import_keeps_exclusive_ownership_through_sql_publication() {
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let bytes = b"model";
    let mut transfer = s
        .engine_import_plan(descriptor(bytes), 5, true, &a)
        .unwrap()
        .begin(root.path(), &control())
        .unwrap();
    let id = transfer.id();
    transfer
        .copy_remaining(&mut Cursor::new(bytes), &control())
        .unwrap();
    let prepared = transfer.finish(&control()).unwrap();
    assert!(matches!(
        Transfer::resume(root.path(), id),
        Err(Error::ResourceLimit)
    ));
    s.commit_engine_import(&prepared, &a).unwrap();
    assert!(matches!(
        Transfer::resume(root.path(), id),
        Err(Error::ResourceLimit)
    ));
    drop(prepared);
    assert_eq!(
        Transfer::resume(root.path(), id).unwrap().committed_bytes(),
        5
    );
}
