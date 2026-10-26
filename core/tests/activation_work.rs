use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use stem_core::{
    content::Content, converters::TextConverter, processing::*, store::InkSnapshot, Error, Result,
};
fn descriptor(id: &str, family: Family) -> Descriptor {
    let recogniser = family == Family::Recogniser;
    Descriptor {
        id: id.into(),
        version: "synthetic-1".into(),
        adapter_version: "1".into(),
        family,
        input: if recogniser {
            InputContract::OrderedStrokesV1
        } else {
            InputContract::EditableV1
        },
        tasks: if recogniser {
            vec![Task::Prose]
        } else {
            vec![]
        },
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some(
            "app-owned synthetic activation fixture; no model/quality claim".into(),
        ),
        redistributable_resources: false,
        resource_sha256: None,
        runtime_contract: "stem-processing-v1".into(),
    }
}
struct Probe {
    calls: Arc<AtomicUsize>,
    mode: u8,
}
impl Recogniser for Probe {
    fn probe(&self) -> Result<()> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        match self.mode {
            1 => Err(Error::EngineUnavailable),
            2 => panic!("synthetic provider probe panic"),
            _ => Ok(()),
        }
    }
    fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
        Ok(RecognitionOutput {
            candidates: vec![Content::Text("unmodified".into())],
            raw_output: "unmodified".into(),
            confidence: None,
            diagnostics: vec![],
        })
    }
}
fn manager(mode: u8) -> (Manager, Arc<AtomicUsize>) {
    let mut m = Manager::new();
    let calls = Arc::new(AtomicUsize::new(0));
    m.register_recogniser(
        descriptor("fixture", Family::Recogniser),
        Box::new(Probe {
            calls: Arc::clone(&calls),
            mode,
        }),
    )
    .unwrap();
    (m, calls)
}
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(30)).unwrap()
}
struct Blocked {
    started: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    retired: Arc<AtomicBool>,
}
impl Drop for Blocked {
    fn drop(&mut self) {
        self.retired.store(true, Ordering::Release);
    }
}
impl Recogniser for Blocked {
    fn probe(&self) -> Result<()> {
        self.started.send(()).unwrap();
        self.release.lock().unwrap().recv().unwrap();
        Ok(())
    }
    fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
        Err(Error::EngineUnavailable)
    }
}
#[test]
fn blocked_probe_releases_manager_borrow_and_removal_keeps_adapter_until_completion_retirement() {
    let mut manager = Manager::new();
    let (started, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let retired = Arc::new(AtomicBool::new(false));
    manager
        .register_recogniser(
            descriptor("blocked", Family::Recogniser),
            Box::new(Blocked {
                started,
                release: Mutex::new(blocked),
                retired: Arc::clone(&retired),
            }),
        )
        .unwrap();
    let work = manager.prepare_activation("blocked", None, true).unwrap();
    let worker = std::thread::spawn(move || work.run(&control()));
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(manager.readiness("blocked"), Readiness::Failed);
    assert!(manager.select(Task::Prose, "blocked").is_err());
    manager.remove("blocked").unwrap();
    assert!(!retired.load(Ordering::Acquire));
    manager
        .register_converter(
            descriptor("other", Family::Converter),
            Box::new(TextConverter),
        )
        .unwrap();
    manager.activate_local("other", None, true).unwrap();
    release.send(()).unwrap();
    let mut completed = worker.join().unwrap();
    assert!(!retired.load(Ordering::Acquire));
    assert!(manager.complete_activation(&mut completed).is_err());
    assert!(!retired.load(Ordering::Acquire));
    drop(completed);
    assert!(retired.load(Ordering::Acquire));
}
#[test]
fn superseded_and_foreign_completions_cannot_enable_an_engine_or_overwrite_new_state() {
    let (mut m, calls) = manager(0);
    let old = m.prepare_activation("fixture", None, true).unwrap();
    let new = m.prepare_activation("fixture", None, true).unwrap();
    let mut old = old.run(&control());
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert!(m.complete_activation(&mut old).is_err());
    let mut ready = new.run(&control());
    let (mut foreign, _) = manager(0);
    foreign.activate_local("fixture", None, true).unwrap();
    assert!(foreign.complete_activation(&mut ready).is_err());
    assert_eq!(foreign.readiness("fixture"), Readiness::Ready);
    m.complete_activation(&mut ready).unwrap();
    assert_eq!(m.readiness("fixture"), Readiness::Ready);
    assert!(m.complete_activation(&mut ready).is_err());
    drop(old);
    drop(ready);
    assert_eq!(m.readiness("fixture"), Readiness::Ready);
    m.select(Task::Prose, "fixture").unwrap();
}
#[test]
fn cancellation_before_probe_or_after_probe_prevents_readiness_and_probe_failures_are_contained() {
    let (mut m, calls) = manager(0);
    let work = m.prepare_activation("fixture", None, true).unwrap();
    let c = control();
    c.cancel();
    let mut done = work.run(&c);
    assert!(matches!(
        m.complete_activation(&mut done),
        Err(Error::Cancelled)
    ));
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert_eq!(m.readiness("fixture"), Readiness::Failed);
    let work = m.prepare_activation("fixture", None, true).unwrap();
    let c = control();
    let mut done = work.run(&c);
    c.cancel();
    assert!(matches!(
        m.complete_activation(&mut done),
        Err(Error::Cancelled)
    ));
    assert_eq!(m.readiness("fixture"), Readiness::Failed);
    for mode in [1, 2] {
        let (mut m, _) = manager(mode);
        let work = m.prepare_activation("fixture", None, true).unwrap();
        let mut done = work.run(&control());
        assert!(m.complete_activation(&mut done).is_err());
        assert_eq!(m.readiness("fixture"), Readiness::Failed);
    }
}
#[test]
fn consent_failure_preserves_ready_but_failed_integrity_reverification_retires_old_readiness() {
    let (mut m, _) = manager(0);
    m.activate_local("fixture", None, true).unwrap();
    assert!(matches!(
        m.prepare_activation("fixture", None, false),
        Err(Error::Denied)
    ));
    assert_eq!(m.readiness("fixture"), Readiness::Ready);
    let old = m.prepare_activation("fixture", None, true).unwrap();
    assert!(m
        .prepare_activation("fixture", Some(b"unreviewed bytes"), true)
        .is_err());
    let mut old = old.run(&control());
    assert!(m.complete_activation(&mut old).is_err());
    assert_eq!(m.readiness("fixture"), Readiness::Failed);
    let mut m = Manager::new();
    let mut d = descriptor("hashed", Family::Recogniser);
    d.resource_sha256 = Some(stem_core::provisioning::resource_digest(
        b"reviewed synthetic resource",
    ));
    m.register_recogniser(
        d,
        Box::new(Probe {
            calls: Arc::new(AtomicUsize::new(0)),
            mode: 0,
        }),
    )
    .unwrap();
    assert!(matches!(
        m.prepare_activation("hashed", Some(b"wrong bytes"), true),
        Err(Error::Integrity)
    ));
    let work = m
        .prepare_activation("hashed", Some(b"reviewed synthetic resource"), true)
        .unwrap();
    let mut done = work.run(&control());
    m.complete_activation(&mut done).unwrap();
    assert_eq!(m.readiness("hashed"), Readiness::Ready);
}
#[test]
fn converter_probe_and_abandoned_work_never_invent_ready_state() {
    let mut m = Manager::new();
    m.register_converter(
        descriptor("text", Family::Converter),
        Box::new(TextConverter),
    )
    .unwrap();
    let work = m.prepare_activation("text", None, true).unwrap();
    drop(work);
    assert_eq!(m.readiness("text"), Readiness::Failed);
    let work = m.prepare_activation("text", None, true).unwrap();
    let done = work.run(&control());
    drop(done);
    assert_eq!(m.readiness("text"), Readiness::Failed);
    let work = m.prepare_activation("text", None, true).unwrap();
    let mut done = std::thread::spawn(move || work.run(&control()))
        .join()
        .unwrap();
    m.complete_activation(&mut done).unwrap();
    assert_eq!(m.readiness("text"), Readiness::Ready);
}

#[derive(Debug)]
struct ErrorPayload(Arc<AtomicBool>);
impl std::fmt::Display for ErrorPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("synthetic provider error payload")
    }
}
impl std::error::Error for ErrorPayload {}
impl Drop for ErrorPayload {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
struct ErrorOwner(Arc<AtomicBool>);
impl Recogniser for ErrorOwner {
    fn probe(&self) -> Result<()> {
        Err(Error::Io(std::io::Error::other(ErrorPayload(Arc::clone(
            &self.0,
        )))))
    }
    fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
        Err(Error::EngineUnavailable)
    }
}
#[test]
fn provider_error_payloads_remain_owned_until_completion_retirement_even_after_cancellation() {
    for cancel in [false, true] {
        let dropped = Arc::new(AtomicBool::new(false));
        let mut m = Manager::new();
        m.register_recogniser(
            descriptor("error-owner", Family::Recogniser),
            Box::new(ErrorOwner(Arc::clone(&dropped))),
        )
        .unwrap();
        let work = m.prepare_activation("error-owner", None, true).unwrap();
        let c = control();
        let mut done = work.run(&c);
        if cancel {
            c.cancel();
        }
        assert!(!dropped.load(Ordering::Acquire));
        assert!(m.complete_activation(&mut done).is_err());
        assert!(!dropped.load(Ordering::Acquire));
        drop(done);
        assert!(dropped.load(Ordering::Acquire));
        assert_eq!(m.readiness("error-owner"), Readiness::Failed);
    }
}

#[test]
fn synchronous_compatibility_keeps_original_provider_error_for_the_caller() {
    let dropped = Arc::new(AtomicBool::new(false));
    let mut m = Manager::new();
    m.register_recogniser(
        descriptor("legacy-error", Family::Recogniser),
        Box::new(ErrorOwner(Arc::clone(&dropped))),
    )
    .unwrap();
    let error = m.activate_local("legacy-error", None, true).unwrap_err();
    assert!(
        matches!(&error,Error::Io(error) if error.get_ref().is_some_and(|payload| payload.is::<ErrorPayload>()))
    );
    assert_eq!(m.readiness("legacy-error"), Readiness::Failed);
    assert!(!dropped.load(Ordering::Acquire));
    drop(error);
    assert!(dropped.load(Ordering::Acquire));
}
