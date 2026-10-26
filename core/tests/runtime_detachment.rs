mod common;
use common::*;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex, Weak,
    },
    thread,
    time::Duration,
};
use stem_core::{
    content::Content,
    processing::*,
    provisioning::{resource_digest, ResourceLease, ResourceLeases},
    security::Access,
    store::{InkSnapshot, Kind, Store},
    Error, Result,
};
fn descriptor(id: &str, family: Family) -> Descriptor {
    let recogniser = family == Family::Recogniser;
    Descriptor {
        id: id.into(),
        version: "synthetic-old".into(),
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
            "app-owned synthetic detachment fixture; no model quality claim".into(),
        ),
        redistributable_resources: false,
        resource_sha256: None,
        runtime_contract: "stem-processing-v1".into(),
    }
}
fn snapshot() -> (Store, InkSnapshot) {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "detachment", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let ink = s.ink_snapshot(b.id, &a).unwrap();
    (s, ink)
}
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(30)).unwrap()
}
struct Model {
    dropped: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}
impl Drop for Model {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}
impl Recogniser for Model {
    fn probe(&self) -> Result<()> {
        Ok(())
    }
    fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        Err(Error::EngineUnavailable)
    }
}
fn manager() -> (Manager, Arc<AtomicBool>, Arc<AtomicUsize>) {
    let mut m = Manager::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    m.register_recogniser(
        descriptor("fixture", Family::Recogniser),
        Box::new(Model {
            dropped: Arc::clone(&dropped),
            calls: Arc::clone(&calls),
        }),
    )
    .unwrap();
    m.activate_local("fixture", None, true).unwrap();
    m.select(Task::Prose, "fixture").unwrap();
    (m, dropped, calls)
}
#[test]
fn detach_disables_selection_without_destroying_adapter_and_old_inference_cannot_run() {
    let (mut m, dropped, calls) = manager();
    let (s, ink) = snapshot();
    let id = ink.block();
    let source = ink.strokes().to_vec();
    let work = m.prepare_recognition(ink, Task::Prose, None).unwrap();
    let detached = m.detach("fixture").unwrap();
    assert_eq!(detached.descriptor().version, "synthetic-old");
    assert_eq!(m.readiness("fixture"), Readiness::Missing);
    assert!(m.descriptors().is_empty());
    assert!(!dropped.load(Ordering::Acquire));
    assert!(matches!(work.run(&control()), Err(Error::Cancelled)));
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert!(!dropped.load(Ordering::Acquire));
    drop(detached);
    assert!(dropped.load(Ordering::Acquire));
    assert_eq!(s.source_ink(id, &Access::public()).unwrap(), source);
}
struct Reentrant {
    manager: Weak<Mutex<Manager>>,
    finished: mpsc::Sender<bool>,
}
impl Drop for Reentrant {
    fn drop(&mut self) {
        let empty = self
            .manager
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .descriptors()
            .is_empty();
        self.finished.send(empty).unwrap();
    }
}
impl Recogniser for Reentrant {
    fn probe(&self) -> Result<()> {
        Ok(())
    }
    fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
        Err(Error::EngineUnavailable)
    }
}
#[test]
fn detached_cleanup_can_reenter_the_manager_after_lock_release() {
    let manager = Arc::new(Mutex::new(Manager::new()));
    let (done, finished) = mpsc::channel();
    manager
        .lock()
        .unwrap()
        .register_recogniser(
            descriptor("reentrant", Family::Recogniser),
            Box::new(Reentrant {
                manager: Arc::downgrade(&manager),
                finished: done,
            }),
        )
        .unwrap();
    manager
        .lock()
        .unwrap()
        .activate_local("reentrant", None, true)
        .unwrap();
    let (copy, txrx) = (Arc::clone(&manager), mpsc::channel());
    let (tx, rx) = txrx;
    let detacher = thread::spawn(move || {
        let detached = copy.lock().unwrap().detach("reentrant").unwrap();
        tx.send(detached).unwrap();
    });
    let detached = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    detacher.join().unwrap();
    assert!(finished.try_recv().is_err());
    let worker = thread::spawn(move || drop(detached));
    assert!(finished.recv_timeout(Duration::from_secs(10)).unwrap());
    worker.join().unwrap();
}
#[test]
fn reused_identifier_does_not_accept_old_activation_or_restore_removed_default() {
    let (mut m, old_dropped, _) = manager();
    let activation = m.prepare_activation("fixture", None, true).unwrap();
    let detached = m.detach("fixture").unwrap();
    let mut next = descriptor("fixture", Family::Recogniser);
    next.version = "synthetic-new".into();
    m.register_recogniser(
        next,
        Box::new(Model {
            dropped: Arc::new(AtomicBool::new(false)),
            calls: Arc::new(AtomicUsize::new(0)),
        }),
    )
    .unwrap();
    m.activate_local("fixture", None, true).unwrap();
    let mut old = activation.run(&control());
    assert!(m.complete_activation(&mut old).is_err());
    assert_eq!(m.readiness("fixture"), Readiness::Ready);
    assert_eq!(m.descriptor("fixture").unwrap().version, "synthetic-new");
    assert_eq!(detached.descriptor().version, "synthetic-old");
    let (_s, ink) = snapshot();
    assert!(m.prepare_recognition(ink, Task::Prose, None).is_err());
    drop(detached);
    assert!(!old_dropped.load(Ordering::Acquire));
    drop(old);
    assert!(old_dropped.load(Ordering::Acquire));
}
struct Blocking {
    started: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    _resource: ResourceLease,
}
impl Recogniser for Blocking {
    fn probe(&self) -> Result<()> {
        Ok(())
    }
    fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
        self.started.send(()).unwrap();
        self.release.lock().unwrap().recv().unwrap();
        Ok(RecognitionOutput {
            candidates: vec![Content::Text("late".into())],
            raw_output: "late".into(),
            confidence: None,
            diagnostics: vec![],
        })
    }
}
#[test]
fn dropping_detached_holder_is_not_a_false_acknowledgement_of_running_worker_resource_retirement() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("synthetic.resource");
    std::fs::write(&path, b"synthetic resource").unwrap();
    let pool = ResourceLeases::default();
    let lease = pool
        .acquire(&path, &resource_digest(b"synthetic resource"))
        .unwrap();
    let (started, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let mut m = Manager::new();
    let mut d = descriptor("resource", Family::Recogniser);
    d.resource_sha256 = Some(resource_digest(b"synthetic resource"));
    m.register_recogniser(
        d,
        Box::new(Blocking {
            started,
            release: Mutex::new(blocked),
            _resource: lease,
        }),
    )
    .unwrap();
    m.activate_local("resource", Some(b"synthetic resource"), true)
        .unwrap();
    let (_s, ink) = snapshot();
    let work = m
        .prepare_recognition(ink, Task::Prose, Some("resource"))
        .unwrap();
    let worker = thread::spawn(move || work.run(&control()));
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    let detached = m.detach("resource").unwrap();
    thread::spawn(move || drop(detached)).join().unwrap();
    assert!(matches!(pool.remove(&path), Err(Error::Conflict)));
    assert!(path.exists());
    release.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    pool.remove(&path).unwrap();
    assert!(!path.exists());
}
struct ConverterModel(Arc<AtomicBool>);
impl Drop for ConverterModel {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
impl Converter for ConverterModel {
    fn formats(&self) -> Vec<Format> {
        vec![Format::PlainText]
    }
    fn convert(&self, _: &Content, _: Format, _: &JobControl) -> Result<Converted> {
        Err(Error::EngineUnavailable)
    }
}
#[test]
fn converter_work_pins_detached_runtime_and_synchronous_remove_still_disposes_idle_adapters() {
    let mut m = Manager::new();
    let dropped = Arc::new(AtomicBool::new(false));
    m.register_converter(
        descriptor("converter", Family::Converter),
        Box::new(ConverterModel(Arc::clone(&dropped))),
    )
    .unwrap();
    m.activate_local("converter", None, true).unwrap();
    let work = m
        .prepare_conversion(
            "converter",
            Content::Code {
                text: "001 [ unfinished".into(),
                language: None,
            },
            Format::PlainText,
        )
        .unwrap();
    let detached = m.detach("converter").unwrap();
    drop(detached);
    assert!(!dropped.load(Ordering::Acquire));
    assert!(matches!(work.run(&control()), Err(Error::Cancelled)));
    assert!(dropped.load(Ordering::Acquire));
    let (mut m, dropped, _) = manager();
    m.remove("fixture").unwrap();
    assert!(dropped.load(Ordering::Acquire));
    assert!(m.detach("absent").is_err());
}
