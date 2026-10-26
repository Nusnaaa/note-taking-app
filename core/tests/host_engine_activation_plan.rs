mod common;
use common::*;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, ThreadId},
    time::{Duration, Instant},
};
use stem_core::{
    host::{Command, Host, Request},
    processing::*,
    security::{Access, Scope},
    store::*,
    Error, Id, Result,
};
fn descriptor() -> Descriptor {
    Descriptor {
        id: "synthetic".into(),
        version: "1".into(),
        adapter_version: "1".into(),
        family: Family::Recogniser,
        input: InputContract::OrderedStrokesV1,
        tasks: vec![Task::Prose],
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some("app-owned stored-plan lifecycle fixture".into()),
        redistributable_resources: false,
        resource_sha256: None,
        runtime_contract: "stem-processing-v1".into(),
    }
}
fn fixture(capacity: usize) -> (tempfile::TempDir, Arc<Host>, Id) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    let mut store = Store::open(&path).unwrap();
    let version = store
        .stage_engine(
            descriptor(),
            None,
            root.path(),
            100,
            true,
            &Access::public(),
        )
        .unwrap();
    drop(store);
    let host = Arc::new(Host::open(path, capacity).unwrap());
    (root, host, version.id)
}
fn call(h: &Host, c: Command) {
    let r = h
        .request(
            &serde_json::to_string(&Request {
                version: 1,
                command: c,
            })
            .unwrap(),
        )
        .unwrap();
    assert!(r.ok, "{:?}", r.error);
}
fn wait_for(mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
}
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(30)).unwrap()
}
struct Gate {
    entered: mpsc::Sender<ThreadId>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl Gate {
    fn wait(&self) {
        let _ = self.entered.send(thread::current().id());
        let _ = self.release.lock().unwrap().recv();
    }
}
fn gate() -> (Gate, mpsc::Receiver<ThreadId>, mpsc::Sender<()>) {
    let (tx, rx) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    (
        Gate {
            entered: tx,
            release: Mutex::new(blocked),
        },
        rx,
        release,
    )
}
struct Model {
    calls: Arc<AtomicUsize>,
    probe: Option<Gate>,
    cleanup: Option<Gate>,
}
impl Drop for Model {
    fn drop(&mut self) {
        if let Some(g) = &self.cleanup {
            g.wait();
        }
    }
}
impl Recogniser for Model {
    fn probe(&self) -> Result<()> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        if let Some(g) = &self.probe {
            g.wait();
        }
        Ok(())
    }
    fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
        Err(Error::EngineUnavailable)
    }
}
fn model(probe: Option<Gate>, cleanup: Option<Gate>) -> (Box<dyn Recogniser>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    (
        Box::new(Model {
            calls: Arc::clone(&calls),
            probe,
            cleanup,
        }),
        calls,
    )
}
#[test]
fn metadata_reservation_is_bounded_and_never_probes_or_marks_installed() {
    let (root, h, id) = fixture(1);
    let plan = h.engine_activation_plan(id).unwrap();
    assert!(matches!(
        h.engine_activation_plan(id),
        Err(Error::ResourceLimit)
    ));
    assert_eq!(h.recognition_status().reserved, 1);
    drop(plan);
    wait_for(|| h.recognition_status().reserved == 0);
    h.close().unwrap();
    let store = Store::open(root.path().join("library.db")).unwrap();
    assert_eq!(
        store.engine_version(id, &Access::public()).unwrap().state,
        Installation::Staged
    );
}
#[test]
fn probe_is_off_writer_and_pen_cancellation_never_revives_a_prepared_candidate() {
    let (_root, h, id) = fixture(8);
    let (probe, entered, release) = gate();
    let (adapter, calls) = model(Some(probe), None);
    let plan = h.engine_activation_plan(id).unwrap();
    let worker = thread::spawn(move || plan.prepare_recogniser(adapter, 100, &control()));
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(h.recognition_status().running, 1);
    call(
        &h,
        Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "saved during optional probe".into(),
        },
    );
    let pen = Id::new();
    h.set_capture_priority(pen, true).unwrap();
    h.set_capture_priority(pen, false).unwrap();
    release.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    wait_for(|| h.recognition_status().reserved == 0);
    assert_eq!(calls.load(Ordering::Acquire), 1);
    h.close().unwrap();
}
#[test]
fn close_waits_for_probe_and_actual_cleanup_before_library_can_reopen() {
    let (root, h, id) = fixture(8);
    let (probe, entered, release_probe) = gate();
    let (cleanup, dropping, release_drop) = gate();
    let (adapter, _) = model(Some(probe), Some(cleanup));
    let plan = h.engine_activation_plan(id).unwrap();
    let worker = thread::spawn(move || plan.prepare_recogniser(adapter, 100, &control()));
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let closing = Arc::clone(&h);
    let (tx, rx) = mpsc::channel();
    let closer = thread::spawn(move || tx.send(closing.close()).unwrap());
    wait_for(|| h.recognition_status().closed);
    assert!(rx.try_recv().is_err());
    assert!(matches!(
        Host::open(root.path().join("library.db"), 8),
        Err(Error::Unavailable)
    ));
    release_probe.send(()).unwrap();
    dropping.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(rx.try_recv().is_err());
    assert!(matches!(
        Host::open(root.path().join("library.db"), 8),
        Err(Error::Unavailable)
    ));
    release_drop.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    closer.join().unwrap();
    Host::open(root.path().join("library.db"), 8)
        .unwrap()
        .close()
        .unwrap();
}
#[test]
fn idle_prepared_disposal_is_off_writer_and_holds_owner_until_cleanup() {
    let (root, h, id) = fixture(8);
    let (cleanup, entered, release) = gate();
    let (adapter, _) = model(None, Some(cleanup));
    let result = h
        .engine_activation_plan(id)
        .unwrap()
        .prepare_recogniser(adapter, 100, &control())
        .unwrap();
    let caller = thread::current().id();
    drop(result);
    assert_ne!(
        entered.recv_timeout(Duration::from_secs(10)).unwrap(),
        caller
    );
    call(
        &h,
        Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "writer remains free".into(),
        },
    );
    let closing = Arc::clone(&h);
    let (tx, rx) = mpsc::channel();
    let closer = thread::spawn(move || tx.send(closing.close()).unwrap());
    wait_for(|| h.recognition_status().closed);
    assert!(rx.try_recv().is_err());
    assert!(matches!(
        Host::open(root.path().join("library.db"), 8),
        Err(Error::Unavailable)
    ));
    release.send(()).unwrap();
    rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    closer.join().unwrap();
}
#[test]
fn relock_cancels_old_metadata_plan_and_locked_app_cannot_reserve_new_plan() {
    let (_root, h, id) = fixture(8);
    let plan = h.engine_activation_plan(id).unwrap();
    h.authenticate([Scope::App].into_iter().collect(), Box::new(TestAuth(true)))
        .unwrap();
    call(&h, Command::SetAppLock { enabled: true });
    call(&h, Command::Relock {});
    let (adapter, calls) = model(None, None);
    assert!(plan.prepare_recogniser(adapter, 100, &control()).is_err());
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert!(matches!(h.engine_activation_plan(id), Err(Error::Denied)));
    h.close().unwrap();
}
