mod common;
use common::*;
use serde_json::Value;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, ThreadId},
    time::{Duration, Instant},
};
use stem_core::{
    content::Content,
    host::{Command, Host, Request},
    ink::Stroke,
    processing::*,
    security::Scope,
    store::{InkSnapshot, Kind},
    Error, Id, Result,
};
fn call(h: &Host, command: Command) -> Value {
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
    r.data.unwrap()
}
fn fixture(capacity: usize) -> (tempfile::TempDir, Arc<Host>, Id, Id, Stroke) {
    let root = tempfile::tempdir().unwrap();
    let h = Arc::new(Host::open(root.path().join("library.db"), capacity).unwrap());
    let n = Id::parse(
        call(
            &h,
            Command::Create {
                kind: Kind::Note,
                parent: None,
                title: "synthetic activation fixture".into(),
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let raw = stroke(0);
    let b = Id::parse(
        call(
            &h,
            Command::Capture {
                ticket: Id::new(),
                entity: n,
                strokes: vec![raw.clone()],
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    (root, h, n, b, raw)
}
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
        personal_use_evidence: Some(
            "app-owned synthetic lifecycle fixture; no model quality claim".into(),
        ),
        redistributable_resources: false,
        resource_sha256: None,
        runtime_contract: "stem-processing-v1".into(),
    }
}
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(30)).unwrap()
}
fn wait_for(mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(
            Instant::now() < end,
            "synthetic lifecycle handshake watchdog"
        );
        thread::sleep(Duration::from_millis(1));
    }
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
    let (release, wait) = mpsc::channel();
    (
        Gate {
            entered: tx,
            release: Mutex::new(wait),
        },
        rx,
        release,
    )
}
struct Model {
    calls: Arc<AtomicUsize>,
    probe: Option<Gate>,
    cleanup: Option<Gate>,
    retired: Arc<AtomicBool>,
    error: Mutex<Option<Gate>>,
}
impl Drop for Model {
    fn drop(&mut self) {
        if let Some(g) = &self.cleanup {
            g.wait();
        }
        self.retired.store(true, Ordering::Release);
    }
}
#[derive(Debug)]
struct ErrorPayload(Gate);
impl std::fmt::Debug for Gate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("synthetic gate")
    }
}
impl std::fmt::Display for ErrorPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("synthetic provider error")
    }
}
impl std::error::Error for ErrorPayload {}
impl Drop for ErrorPayload {
    fn drop(&mut self) {
        self.0.wait();
    }
}
impl Recogniser for Model {
    fn probe(&self) -> Result<()> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        if let Some(g) = &self.probe {
            g.wait();
        }
        if let Some(g) = self.error.lock().unwrap().take() {
            return Err(Error::Io(std::io::Error::other(ErrorPayload(g))));
        }
        Ok(())
    }
    fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
        Err(Error::EngineUnavailable)
    }
}
fn model(
    probe: Option<Gate>,
    cleanup: Option<Gate>,
    error: Option<Gate>,
) -> (Manager, Arc<AtomicUsize>, Arc<AtomicBool>) {
    let mut m = Manager::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let retired = Arc::new(AtomicBool::new(false));
    m.register_recogniser(
        descriptor(),
        Box::new(Model {
            calls: Arc::clone(&calls),
            probe,
            cleanup,
            retired: Arc::clone(&retired),
            error: Mutex::new(error),
        }),
    )
    .unwrap();
    (m, calls, retired)
}
#[test]
fn host_and_app_authority_bind_completion_and_locked_app_cannot_reserve_activation() {
    let (_root, h, _, _, _) = fixture(8);
    let (_other, foreign, _, _, _) = fixture(8);
    let (mut m, calls, _) = model(None, None, None);
    let result = h
        .activation_source()
        .unwrap()
        .prepare(&mut m, "synthetic", None, true)
        .unwrap()
        .run(&control())
        .unwrap();
    assert!(matches!(
        foreign.complete_activation(&mut m, &result),
        Err(Error::Denied)
    ));
    assert_eq!(m.readiness("synthetic"), Readiness::Failed);
    h.complete_activation(&mut m, &result).unwrap();
    assert_eq!(m.readiness("synthetic"), Readiness::Ready);
    assert!(h.complete_activation(&mut m, &result).is_err());
    drop(result);
    wait_for(|| h.recognition_status().reserved == 0);
    let result = h
        .activation_source()
        .unwrap()
        .prepare(&mut m, "synthetic", None, true)
        .unwrap()
        .run(&control())
        .unwrap();
    h.authenticate([Scope::App].into_iter().collect(), Box::new(TestAuth(true)))
        .unwrap();
    call(&h, Command::SetAppLock { enabled: true });
    call(&h, Command::Relock {});
    assert!(h.complete_activation(&mut m, &result).is_err());
    assert_eq!(m.readiness("synthetic"), Readiness::Failed);
    assert!(matches!(h.activation_source(), Err(Error::Denied)));
    drop(result);
    wait_for(|| h.recognition_status().reserved == 0);
    assert_eq!(calls.load(Ordering::Acquire), 2);
    h.close().unwrap();
    foreign.close().unwrap();
}
#[test]
fn optional_capacity_and_pen_priority_never_run_a_cancelled_idle_probe_or_mark_authoring_failed() {
    let (_root, h, n, _, _) = fixture(1);
    let (mut m, calls, _) = model(None, None, None);
    let source = h.activation_source().unwrap();
    assert!(matches!(h.activation_source(), Err(Error::ResourceLimit)));
    drop(source);
    wait_for(|| h.recognition_status().reserved == 0);
    let work = h
        .activation_source()
        .unwrap()
        .prepare(&mut m, "synthetic", None, true)
        .unwrap();
    let pen = Id::new();
    h.set_capture_priority(pen, true).unwrap();
    assert!(matches!(h.activation_source(), Err(Error::ResourceLimit)));
    assert!(work.run(&control()).is_err());
    h.set_capture_priority(pen, false).unwrap();
    wait_for(|| h.recognition_status().reserved == 0);
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert_eq!(m.readiness("synthetic"), Readiness::Failed);
    call(
        &h,
        Command::Append {
            entity: n,
            content: Content::Text("authoring remains available".into()),
        },
    );
    let mut status = h.subscribe_status();
    let s = status.next_timeout(Duration::ZERO).unwrap().unwrap();
    assert!(!s.needs_attention);
    h.close().unwrap();
}
#[test]
fn running_probe_is_cancelled_by_pen_priority_while_authoring_and_raw_source_remain_available() {
    let (_root, h, n, b, raw) = fixture(8);
    let (g, entered, release) = gate();
    let (mut m, _, _) = model(Some(g), None, None);
    let work = h
        .activation_source()
        .unwrap()
        .prepare(&mut m, "synthetic", None, true)
        .unwrap();
    let worker = thread::spawn(move || work.run(&control()));
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(h.recognition_status().running, 1);
    call(
        &h,
        Command::Append {
            entity: n,
            content: Content::Text("saved during probe".into()),
        },
    );
    assert_eq!(
        call(&h, Command::SourceInk { block: b }),
        serde_json::to_value(vec![raw]).unwrap()
    );
    let pen = Id::new();
    h.set_capture_priority(pen, true).unwrap();
    h.set_capture_priority(pen, false).unwrap();
    release.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    wait_for(|| h.recognition_status().reserved == 0);
    assert_eq!(m.readiness("synthetic"), Readiness::Failed);
    h.close().unwrap();
}
#[test]
fn idle_adapter_cleanup_is_off_writer_and_full_close_retains_library_ownership_until_it_finishes() {
    let (root, h, n, b, raw) = fixture(8);
    let (g, entered, release) = gate();
    let (mut m, _, retired) = model(None, Some(g), None);
    let work = h
        .activation_source()
        .unwrap()
        .prepare(&mut m, "synthetic", None, true)
        .unwrap();
    m.remove("synthetic").unwrap();
    let caller = thread::current().id();
    drop(work);
    let cleanup_thread = entered.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_ne!(cleanup_thread, caller);
    assert_eq!(h.recognition_status().reserved, 1);
    assert!(!retired.load(Ordering::Acquire));
    call(
        &h,
        Command::Append {
            entity: n,
            content: Content::Text("writer is free during cleanup".into()),
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
    assert!(retired.load(Ordering::Acquire));
    let reopened = Host::open(root.path().join("library.db"), 8).unwrap();
    assert_eq!(
        call(&reopened, Command::SourceInk { block: b }),
        serde_json::to_value(vec![raw]).unwrap()
    );
    reopened.close().unwrap();
}
#[test]
fn close_waits_for_running_probe_then_actual_adapter_destruction_before_reopening_library() {
    let (root, h, _, b, raw) = fixture(8);
    let (probe, started, release_probe) = gate();
    let (cleanup, dropping, release_drop) = gate();
    let (mut m, _, retired) = model(Some(probe), Some(cleanup), None);
    let work = h
        .activation_source()
        .unwrap()
        .prepare(&mut m, "synthetic", None, true)
        .unwrap();
    let worker = thread::spawn(move || work.run(&control()));
    let probe_thread = started.recv_timeout(Duration::from_secs(10)).unwrap();
    m.remove("synthetic").unwrap();
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
    assert!(worker.join().unwrap().is_err());
    let cleanup_thread = dropping.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_ne!(cleanup_thread, probe_thread);
    assert!(!retired.load(Ordering::Acquire));
    assert!(rx.try_recv().is_err());
    assert!(matches!(
        Host::open(root.path().join("library.db"), 8),
        Err(Error::Unavailable)
    ));
    release_drop.send(()).unwrap();
    rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    closer.join().unwrap();
    assert!(retired.load(Ordering::Acquire));
    let reopened = Host::open(root.path().join("library.db"), 8).unwrap();
    assert_eq!(
        call(&reopened, Command::SourceInk { block: b }),
        serde_json::to_value(vec![raw]).unwrap()
    );
    reopened.close().unwrap();
}
#[test]
fn provider_error_payload_retires_off_writer_after_completion_failure() {
    let (_root, h, n, _, _) = fixture(8);
    let (g, entered, release) = gate();
    let (mut m, _, _) = model(None, None, Some(g));
    let result = h
        .activation_source()
        .unwrap()
        .prepare(&mut m, "synthetic", None, true)
        .unwrap()
        .run(&control())
        .unwrap();
    let applying = Arc::clone(&h);
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let outcome = applying.complete_activation(&mut m, &result);
        tx.send((outcome, m, result)).unwrap();
    });
    let finished = rx.recv_timeout(Duration::from_secs(10));
    if finished.is_err() {
        let _ = release.send(());
        worker.join().unwrap();
        panic!("completion blocked on provider-owned error cleanup");
    }
    let (outcome, m, result) = finished.unwrap();
    worker.join().unwrap();
    assert!(outcome.is_err());
    assert_eq!(m.readiness("synthetic"), Readiness::Failed);
    assert!(entered.try_recv().is_err());
    let caller = thread::current().id();
    drop(result);
    assert_ne!(
        entered.recv_timeout(Duration::from_secs(10)).unwrap(),
        caller
    );
    assert_eq!(h.recognition_status().reserved, 1);
    call(
        &h,
        Command::Append {
            entity: n,
            content: Content::Text("saved during error cleanup".into()),
        },
    );
    release.send(()).unwrap();
    wait_for(|| h.recognition_status().reserved == 0);
    h.close().unwrap();
}

#[test]
fn descriptor_frame_rejection_preserves_existing_ready_engine_and_authoring() {
    let (_root, h, n, _, _) = fixture(8);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut m = Manager::new();
    let mut d = descriptor();
    d.personal_use_evidence = Some("synthetic fixture metadata ".repeat(400_000));
    m.register_recogniser(
        d,
        Box::new(Model {
            calls: Arc::clone(&calls),
            probe: None,
            cleanup: None,
            retired: Arc::new(AtomicBool::new(false)),
            error: Mutex::new(None),
        }),
    )
    .unwrap();
    m.activate_local("synthetic", None, true).unwrap();
    assert_eq!(m.readiness("synthetic"), Readiness::Ready);
    assert!(matches!(
        h.activation_source()
            .unwrap()
            .prepare(&mut m, "synthetic", None, true),
        Err(Error::ResourceLimit)
    ));
    wait_for(|| h.recognition_status().reserved == 0);
    assert_eq!(m.readiness("synthetic"), Readiness::Ready);
    assert_eq!(calls.load(Ordering::Acquire), 1);
    call(
        &h,
        Command::Append {
            entity: n,
            content: Content::Text("still saved after optional rejection".into()),
        },
    );
    h.close().unwrap();
}
