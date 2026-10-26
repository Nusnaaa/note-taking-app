use std::{
    io::Cursor,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    time::Duration,
};
use stem_core::{
    content::Content, converters::TextConverter, processing::*, provisioning::resource_digest,
    security::Access, store::*, Error, Result,
};
fn descriptor(version: &str) -> Descriptor {
    Descriptor {
        id: "synthetic".into(),
        version: version.into(),
        adapter_version: "1".into(),
        family: Family::Recogniser,
        input: InputContract::OrderedStrokesV1,
        tasks: vec![Task::Prose],
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some("app-owned synthetic installation-plan fixture".into()),
        redistributable_resources: false,
        resource_sha256: None,
        runtime_contract: "stem-processing-v1".into(),
    }
}
struct Model {
    fail: bool,
    dropped: Arc<AtomicBool>,
    registry: Option<Weak<Mutex<Manager>>>,
}
impl Drop for Model {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}
impl Recogniser for Model {
    fn probe(&self) -> Result<()> {
        if let Some(registry) = &self.registry {
            assert_eq!(
                registry
                    .upgrade()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .descriptor("synthetic")
                    .unwrap()
                    .version,
                "old"
            );
        }
        if self.fail {
            Err(Error::WorkerFailed)
        } else {
            Ok(())
        }
    }
    fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
        Ok(RecognitionOutput {
            candidates: vec![Content::Text("unchanged".into())],
            raw_output: "unchanged".into(),
            confidence: None,
            diagnostics: vec![],
        })
    }
}
fn adapter(fail: bool) -> Box<dyn Recogniser> {
    Box::new(Model {
        fail,
        dropped: Arc::new(AtomicBool::new(false)),
        registry: None,
    })
}
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(30)).unwrap()
}
fn setup() -> (tempfile::TempDir, Store, Manager, EngineVersion) {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let old = s
        .stage_engine(descriptor("old"), None, dir.path(), 100, true, &a)
        .unwrap();
    let mut m = Manager::new();
    s.activate_engine(old.id, &mut m, adapter(false), 100, &a)
        .unwrap();
    (dir, s, m, old)
}
#[test]
fn preparation_probes_without_live_manager_borrow_and_commit_publishes_durable_version() {
    let (dir, mut s, m, _) = setup();
    let a = Access::public();
    let next = s
        .stage_engine(descriptor("new"), None, dir.path(), 100, true, &a)
        .unwrap();
    let shared = Arc::new(Mutex::new(m));
    let plan: EngineActivationPlan = s.engine_activation_plan(next.id, &a).unwrap();
    let model = Model {
        fail: false,
        dropped: Arc::new(AtomicBool::new(false)),
        registry: Some(Arc::downgrade(&shared)),
    };
    let mut prepared =
        std::thread::spawn(move || plan.prepare_recogniser(Box::new(model), 100, &control()))
            .join()
            .unwrap()
            .unwrap();
    assert_eq!(
        shared
            .lock()
            .unwrap()
            .descriptor("synthetic")
            .unwrap()
            .version,
        "old"
    );
    let detached = {
        s.commit_engine_activation(&mut prepared, &mut shared.lock().unwrap(), &a)
            .unwrap()
    };
    assert!(prepared.version().is_none());
    assert_eq!(
        s.active_engine("synthetic", &a).unwrap().unwrap().id,
        next.id
    );
    assert_eq!(
        shared
            .lock()
            .unwrap()
            .descriptor("synthetic")
            .unwrap()
            .version,
        "new"
    );
    drop(detached);
    assert!(s
        .commit_engine_activation(&mut prepared, &mut shared.lock().unwrap(), &a)
        .is_err());
}
#[test]
fn failed_probe_and_commit_keep_previous_runtime_and_retry_preserves_candidate_holder() {
    let (dir, mut s, mut m, old) = setup();
    let a = Access::public();
    let next = s
        .stage_engine(descriptor("new"), None, dir.path(), 100, true, &a)
        .unwrap();
    assert!(s
        .engine_activation_plan(next.id, &a)
        .unwrap()
        .prepare_recogniser(adapter(true), 100, &control())
        .is_err());
    assert_eq!(m.descriptor("synthetic").unwrap().version, "old");
    let dropped = Arc::new(AtomicBool::new(false));
    let mut prepared = s
        .engine_activation_plan(next.id, &a)
        .unwrap()
        .prepare_recogniser(
            Box::new(Model {
                fail: false,
                dropped: Arc::clone(&dropped),
                registry: None,
            }),
            100,
            &control(),
        )
        .unwrap();
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s
        .commit_engine_activation(&mut prepared, &mut m, &a)
        .is_err());
    assert_eq!(m.descriptor("synthetic").unwrap().version, "old");
    assert_eq!(
        s.active_engine("synthetic", &a).unwrap().unwrap().id,
        old.id
    );
    assert!(prepared.version().is_some());
    assert!(!dropped.load(Ordering::Acquire));
    drop(
        s.commit_engine_activation(&mut prepared, &mut m, &a)
            .unwrap(),
    );
    assert_eq!(m.descriptor("synthetic").unwrap().version, "new");
}
#[test]
fn stale_generation_and_foreign_library_cannot_commit_prepared_candidate() {
    let (dir, mut s, mut m, old) = setup();
    let a = Access::public();
    let next = s
        .stage_engine(descriptor("new"), None, dir.path(), 100, true, &a)
        .unwrap();
    let mut prepared = s
        .engine_activation_plan(next.id, &a)
        .unwrap()
        .prepare_recogniser(adapter(false), 100, &control())
        .unwrap();
    let mut foreign = Store::memory().unwrap();
    assert!(matches!(
        foreign.commit_engine_activation(&mut prepared, &mut m, &a),
        Err(Error::Conflict)
    ));
    s.create(Kind::Note, None, "concurrent authored work", &a)
        .unwrap();
    assert!(matches!(
        s.commit_engine_activation(&mut prepared, &mut m, &a),
        Err(Error::Conflict)
    ));
    assert_eq!(
        s.active_engine("synthetic", &a).unwrap().unwrap().id,
        old.id
    );
    assert_eq!(m.descriptor("synthetic").unwrap().version, "old");
    assert!(prepared.version().is_some());
}
#[test]
fn changed_resource_and_load_limit_reject_before_replacing_live_engine() {
    let (dir, mut s, m, _) = setup();
    let a = Access::public();
    let bytes = b"synthetic local resource";
    let mut d = descriptor("resource");
    d.resource_sha256 = Some(resource_digest(bytes));
    let next = s
        .stage_engine(d, Some(&mut Cursor::new(bytes)), dir.path(), 100, true, &a)
        .unwrap();
    assert!(matches!(
        s.engine_activation_plan(next.id, &a)
            .unwrap()
            .prepare_recogniser(adapter(false), 1, &control()),
        Err(Error::ResourceLimit)
    ));
    std::fs::write(next.local_resource.as_ref().unwrap(), b"changed").unwrap();
    assert!(matches!(
        s.engine_activation_plan(next.id, &a)
            .unwrap()
            .prepare_recogniser(adapter(false), 100, &control()),
        Err(Error::Integrity)
    ));
    assert_eq!(m.descriptor("synthetic").unwrap().version, "old");
}
#[test]
fn cancelled_converter_commit_preserves_staged_state_and_fresh_converter_flow_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let mut d = descriptor("converter");
    d.family = Family::Converter;
    d.input = InputContract::EditableV1;
    d.tasks.clear();
    let v = s.stage_engine(d, None, dir.path(), 100, true, &a).unwrap();
    let c = control();
    let mut prepared = s
        .engine_activation_plan(v.id, &a)
        .unwrap()
        .prepare_converter(Box::new(TextConverter), 100, &c)
        .unwrap();
    c.cancel();
    let mut m = Manager::new();
    assert!(matches!(
        s.commit_engine_activation(&mut prepared, &mut m, &a),
        Err(Error::Cancelled)
    ));
    assert_eq!(
        s.engine_version(v.id, &a).unwrap().state,
        Installation::Staged
    );
    assert_eq!(m.readiness("synthetic"), Readiness::Missing);
    let mut fresh = s
        .engine_activation_plan(v.id, &a)
        .unwrap()
        .prepare_converter(Box::new(TextConverter), 100, &control())
        .unwrap();
    assert!(s
        .commit_engine_activation(&mut fresh, &mut m, &a)
        .unwrap()
        .is_none());
    assert_eq!(m.readiness("synthetic"), Readiness::Ready);
}

#[test]
fn sdk_factory_receives_verified_snapshot_even_when_resource_path_changes_during_load() {
    let (dir, mut s, mut m, _) = setup();
    let a = Access::public();
    let bytes = b"synthetic verified SDK resource";
    let mut d = descriptor("factory");
    d.resource_sha256 = Some(resource_digest(bytes));
    let v = s
        .stage_engine(d, Some(&mut Cursor::new(bytes)), dir.path(), 100, true, &a)
        .unwrap();
    let path = v.local_resource.clone().unwrap();
    let loaded = Arc::new(Mutex::new(Vec::new()));
    let capture = Arc::clone(&loaded);
    let mut prepared = s
        .engine_activation_plan(v.id, &a)
        .unwrap()
        .prepare_recogniser_with(100, &control(), move |descriptor, resource, control| {
            control.check()?;
            let resource = resource.expect("verified resource snapshot");
            assert_eq!(
                descriptor.resource_sha256.as_deref(),
                Some(resource_digest(resource).as_str())
            );
            std::fs::write(path, b"different future file")?;
            *capture.lock().unwrap() = resource.to_vec();
            Ok(adapter(false))
        })
        .unwrap();
    assert_eq!(&*loaded.lock().unwrap(), bytes);
    drop(
        s.commit_engine_activation(&mut prepared, &mut m, &a)
            .unwrap(),
    );
    assert_eq!(m.descriptor("synthetic").unwrap().version, "factory");
    assert_eq!(m.readiness("synthetic"), Readiness::Ready);
}
#[test]
fn resource_rejection_or_cancel_never_invokes_sdk_factory() {
    let (dir, mut s, _m, _) = setup();
    let a = Access::public();
    let bytes = b"verified synthetic resource";
    let mut d = descriptor("factory");
    d.resource_sha256 = Some(resource_digest(bytes));
    let v = s
        .stage_engine(d, Some(&mut Cursor::new(bytes)), dir.path(), 100, true, &a)
        .unwrap();
    let called = AtomicBool::new(false);
    let factory = |_: &Descriptor, _: Option<&[u8]>, _: &JobControl| {
        called.store(true, Ordering::Release);
        Ok(adapter(false))
    };
    assert!(matches!(
        s.engine_activation_plan(v.id, &a)
            .unwrap()
            .prepare_recogniser_with(1, &control(), factory),
        Err(Error::ResourceLimit)
    ));
    let c = control();
    c.cancel();
    assert!(matches!(
        s.engine_activation_plan(v.id, &a)
            .unwrap()
            .prepare_recogniser_with(100, &c, factory),
        Err(Error::Cancelled)
    ));
    std::fs::write(v.local_resource.unwrap(), b"corrupt").unwrap();
    assert!(matches!(
        s.engine_activation_plan(v.id, &a)
            .unwrap()
            .prepare_recogniser_with(100, &control(), factory),
        Err(Error::Integrity)
    ));
    assert!(!called.load(Ordering::Acquire));
}
#[test]
fn factory_panic_and_factory_cancellation_leave_old_runtime_and_metadata_unchanged() {
    let (dir, mut s, m, old) = setup();
    let a = Access::public();
    let v = s
        .stage_engine(descriptor("factory"), None, dir.path(), 100, true, &a)
        .unwrap();
    assert!(matches!(
        s.engine_activation_plan(v.id, &a)
            .unwrap()
            .prepare_recogniser_with(100, &control(), |_, _, _| panic!("synthetic factory panic")),
        Err(Error::WorkerFailed)
    ));
    assert!(matches!(
        s.engine_activation_plan(v.id, &a)
            .unwrap()
            .prepare_recogniser_with(100, &control(), |_, resource, c| {
                assert!(resource.is_none());
                c.cancel();
                Ok(adapter(false))
            }),
        Err(Error::Cancelled)
    ));
    assert_eq!(m.descriptor("synthetic").unwrap().version, "old");
    assert_eq!(
        s.active_engine("synthetic", &a).unwrap().unwrap().id,
        old.id
    );
    assert_eq!(
        s.engine_version(v.id, &a).unwrap().state,
        Installation::Staged
    );
}
#[test]
fn converter_factory_uses_the_same_verified_resource_contract() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let bytes = b"synthetic converter configuration";
    let mut d = descriptor("converter-factory");
    d.family = Family::Converter;
    d.input = InputContract::EditableV1;
    d.tasks.clear();
    d.resource_sha256 = Some(resource_digest(bytes));
    let v = s
        .stage_engine(d, Some(&mut Cursor::new(bytes)), dir.path(), 100, true, &a)
        .unwrap();
    let mut prepared = s
        .engine_activation_plan(v.id, &a)
        .unwrap()
        .prepare_converter_with(100, &control(), |descriptor, resource, c| {
            c.check()?;
            assert_eq!(descriptor.family, Family::Converter);
            assert_eq!(resource, Some(bytes.as_slice()));
            Ok(Box::new(TextConverter))
        })
        .unwrap();
    let mut m = Manager::new();
    drop(
        s.commit_engine_activation(&mut prepared, &mut m, &a)
            .unwrap(),
    );
    assert_eq!(m.readiness("synthetic"), Readiness::Ready);
}

#[test]
fn typed_factories_reject_the_other_adapter_family_before_construction() {
    let (dir, mut s, _m, _) = setup();
    let a = Access::public();
    let recognised = s
        .stage_engine(
            descriptor("recogniser-factory"),
            None,
            dir.path(),
            100,
            true,
            &a,
        )
        .unwrap();
    let mut d = descriptor("converter-factory");
    d.family = Family::Converter;
    d.input = InputContract::EditableV1;
    d.tasks.clear();
    let converted = s.stage_engine(d, None, dir.path(), 100, true, &a).unwrap();
    let called = AtomicBool::new(false);
    assert!(matches!(
        s.engine_activation_plan(recognised.id, &a)
            .unwrap()
            .prepare_converter_with(100, &control(), |_, _, _| {
                called.store(true, Ordering::Release);
                Ok(Box::new(TextConverter))
            }),
        Err(Error::EngineUnavailable)
    ));
    assert!(matches!(
        s.engine_activation_plan(converted.id, &a)
            .unwrap()
            .prepare_recogniser_with(100, &control(), |_, _, _| {
                called.store(true, Ordering::Release);
                Ok(adapter(false))
            }),
        Err(Error::EngineUnavailable)
    ));
    assert!(!called.load(Ordering::Acquire));
}
#[test]
fn corrupt_stored_identity_resource_or_privacy_metadata_never_invokes_factory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let mut s = Store::open(&path).unwrap();
    let a = Access::public();
    let bytes = b"synthetic verified resource";
    let mut d = descriptor("factory");
    d.resource_sha256 = Some(resource_digest(bytes));
    let v = s
        .stage_engine(d, Some(&mut Cursor::new(bytes)), dir.path(), 100, true, &a)
        .unwrap();
    let db = rusqlite::Connection::open(path).unwrap();
    let replace = |value: &EngineVersion| {
        db.execute(
            "UPDATE engine_versions SET payload=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_string(value).unwrap(), v.id.to_string()],
        )
        .unwrap();
    };
    let mut corrupt = v.clone();
    corrupt.id = stem_core::Id::new();
    replace(&corrupt);
    assert!(matches!(
        s.engine_activation_plan(v.id, &a),
        Err(Error::Integrity)
    ));
    corrupt = v.clone();
    corrupt.local_resource = None;
    replace(&corrupt);
    let called = AtomicBool::new(false);
    let factory = |_: &Descriptor, _: Option<&[u8]>, _: &JobControl| {
        called.store(true, Ordering::Release);
        Ok(adapter(false))
    };
    assert!(matches!(
        s.engine_activation_plan(v.id, &a)
            .unwrap()
            .prepare_recogniser_with(100, &control(), factory),
        Err(Error::Integrity)
    ));
    corrupt.descriptor.content_egress = true;
    replace(&corrupt);
    assert!(matches!(
        s.engine_activation_plan(v.id, &a)
            .unwrap()
            .prepare_recogniser_with(100, &control(), factory),
        Err(Error::Denied)
    ));
    assert!(!called.load(Ordering::Acquire));
}
