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
