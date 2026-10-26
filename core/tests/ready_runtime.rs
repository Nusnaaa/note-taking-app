mod common;
use common::*;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, Weak,
    },
    time::Duration,
};
use stem_core::{
    processing::*,
    security::Access,
    store::{InkSnapshot, Kind, Store},
    Error, Result,
};
fn descriptor(id: &str, version: &str, family: Family) -> Descriptor {
    let ink = family == Family::Recogniser;
    Descriptor {
        id: id.into(),
        version: version.into(),
        adapter_version: "1".into(),
        family,
        input: if ink {
            InputContract::OrderedStrokesV1
        } else {
            InputContract::EditableV1
        },
        tasks: if ink { vec![Task::Prose] } else { vec![] },
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some("app-owned synthetic ready-handoff fixture".into()),
        redistributable_resources: false,
        resource_sha256: None,
        runtime_contract: "stem-processing-v1".into(),
    }
}
struct Model {
    drop: Arc<AtomicBool>,
    fail: bool,
}
impl Drop for Model {
    fn drop(&mut self) {
        self.drop.store(true, Ordering::Release);
    }
}
impl Recogniser for Model {
    fn probe(&self) -> Result<()> {
        if self.fail {
            Err(Error::EngineUnavailable)
        } else {
            Ok(())
        }
    }
    fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
        Err(Error::EngineUnavailable)
    }
}
fn manager(version: &str) -> (Manager, Arc<AtomicBool>) {
    let mut m = Manager::new();
    let drop = Arc::new(AtomicBool::new(false));
    m.register_recogniser(
        descriptor("model", version, Family::Recogniser),
        Box::new(Model {
            drop: Arc::clone(&drop),
            fail: false,
        }),
    )
    .unwrap();
    m.activate_local("model", None, true).unwrap();
    m.select(Task::Prose, "model").unwrap();
    (m, drop)
}
fn snapshot() -> (Store, InkSnapshot) {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "handoff", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let snap = s.ink_snapshot(b.id, &a).unwrap();
    (s, snap)
}
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(30)).unwrap()
}
#[test]
fn failed_candidate_keeps_old_engine_and_unready_transfer_has_no_effect() {
    let (old, old_drop) = manager("old");
    let mut candidate = Manager::new();
    candidate
        .register_recogniser(
            descriptor("model", "failed", Family::Recogniser),
            Box::new(Model {
                drop: Arc::new(AtomicBool::new(false)),
                fail: true,
            }),
        )
        .unwrap();
    assert!(candidate.take_ready("model").is_err());
    assert!(candidate.activate_local("model", None, true).is_err());
    assert!(candidate.take_ready("model").is_err());
    assert_eq!(old.readiness("model"), Readiness::Ready);
    assert_eq!(old.descriptor("model").unwrap().version, "old");
    assert!(!old_drop.load(Ordering::Acquire));
}
#[test]
fn replacement_invalidates_old_work_but_keeps_it_pinned_and_preserves_compatible_default() {
    let (mut old, old_drop) = manager("old");
    let (_s, ink) = snapshot();
    let pending = old.prepare_recognition(ink, Task::Prose, None).unwrap();
    let (mut candidate, new_drop) = manager("new");
    let mut ready = candidate.take_ready("model").unwrap();
    assert_eq!(candidate.readiness("model"), Readiness::Missing);
    let detached = old.install_ready(&mut ready).unwrap().unwrap();
    assert!(ready.descriptor().is_none());
    assert!(old.install_ready(&mut ready).is_err());
    assert_eq!(old.descriptor("model").unwrap().version, "new");
    assert!(!old_drop.load(Ordering::Acquire));
    drop(detached);
    assert!(!old_drop.load(Ordering::Acquire));
    assert!(matches!(pending.run(&control()), Err(Error::Cancelled)));
    assert!(old_drop.load(Ordering::Acquire));
    assert!(!new_drop.load(Ordering::Acquire));
    let (_s, ink) = snapshot();
    assert_eq!(
        old.prepare_recognition(ink, Task::Prose, None)
            .unwrap()
            .descriptor()
            .version,
        "new"
    );
}
#[test]
fn extraction_freezes_source_work_and_abandoned_holder_disposes_candidate() {
    let (mut source, dropped) = manager("candidate");
    let (_s, ink) = snapshot();
    let pending = source.prepare_recognition(ink, Task::Prose, None).unwrap();
    let ready = source.take_ready("model").unwrap();
    assert!(matches!(pending.run(&control()), Err(Error::Cancelled)));
    assert!(!dropped.load(Ordering::Acquire));
    drop(ready);
    assert!(dropped.load(Ordering::Acquire));
}
#[test]
fn family_mismatch_preserves_both_holders_and_unsupported_tasks_have_no_fallback() {
    use stem_core::converters::TextConverter;
    let (mut destination, old_drop) = manager("old");
    let mut converter = Manager::new();
    converter
        .register_converter(
            descriptor("model", "converter", Family::Converter),
            Box::new(TextConverter),
        )
        .unwrap();
    converter.activate_local("model", None, true).unwrap();
    let mut wrong = converter.take_ready("model").unwrap();
    assert!(matches!(
        destination.install_ready(&mut wrong),
        Err(Error::Conflict)
    ));
    assert!(wrong.descriptor().is_some());
    assert_eq!(destination.descriptor("model").unwrap().version, "old");
    assert!(!old_drop.load(Ordering::Acquire));
    let mut candidate = Manager::new();
    let mut d = descriptor("model", "math-only", Family::Recogniser);
    d.tasks = vec![Task::Maths];
    candidate
        .register_recogniser(
            d,
            Box::new(Model {
                drop: Arc::new(AtomicBool::new(false)),
                fail: false,
            }),
        )
        .unwrap();
    candidate.activate_local("model", None, true).unwrap();
    let mut ready = candidate.take_ready("model").unwrap();
    drop(destination.install_ready(&mut ready).unwrap());
    let (_s, ink) = snapshot();
    assert!(destination
        .prepare_recognition(ink, Task::Prose, None)
        .is_err());
    destination.select(Task::Maths, "model").unwrap();
}
struct Callback {
    manager: Weak<Mutex<Manager>>,
    probe_seen: Arc<AtomicBool>,
    drop_seen: mpsc::Sender<String>,
}
impl Recogniser for Callback {
    fn probe(&self) -> Result<()> {
        let version = self
            .manager
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .descriptor("model")
            .unwrap()
            .version
            .clone();
        assert_eq!(version, "old");
        self.probe_seen.store(true, Ordering::Release);
        Ok(())
    }
    fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
        Err(Error::EngineUnavailable)
    }
}
impl Drop for Callback {
    fn drop(&mut self) {
        let version = self
            .manager
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .descriptor("model")
            .map(|d| d.version.clone())
            .unwrap_or_default();
        self.drop_seen.send(version).unwrap();
    }
}
#[test]
fn candidate_probe_can_reenter_existing_registry_before_publication() {
    let (old, _) = manager("old");
    let shared = Arc::new(Mutex::new(old));
    let (seen, rx) = mpsc::channel();
    let probe_seen = Arc::new(AtomicBool::new(false));
    let mut candidate = Manager::new();
    candidate
        .register_recogniser(
            descriptor("model", "new", Family::Recogniser),
            Box::new(Callback {
                manager: Arc::downgrade(&shared),
                probe_seen: Arc::clone(&probe_seen),
                drop_seen: seen,
            }),
        )
        .unwrap();
    let worker = std::thread::spawn(move || {
        candidate.activate_local("model", None, true).unwrap();
        candidate.take_ready("model").unwrap()
    });
    let mut ready = worker.join().unwrap();
    assert!(probe_seen.load(Ordering::Acquire));
    let old = { shared.lock().unwrap().install_ready(&mut ready).unwrap() };
    drop(old);
    let detached = { shared.lock().unwrap().detach("model").unwrap() };
    let retire = std::thread::spawn(move || drop(detached));
    assert_eq!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), "");
    retire.join().unwrap();
}
