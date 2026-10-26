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

fn prepared(h: &Host, id: Id) -> stem_core::host::HostPreparedEngineActivation {
    let (adapter, _) = model(None, None);
    h.engine_activation_plan(id)
        .unwrap()
        .prepare_recogniser(adapter, 100, &control())
        .unwrap()
}
#[test]
fn host_publication_installs_runtime_and_durable_metadata_and_rejects_replay() {
    let (root, h, id) = fixture(8);
    let ready = prepared(&h, id);
    let manager = Arc::new(Mutex::new(Manager::new()));
    h.commit_engine_activation(&manager, &ready).unwrap();
    assert_eq!(
        manager
            .lock()
            .unwrap()
            .descriptor("synthetic")
            .unwrap()
            .version,
        "1"
    );
    assert!(matches!(
        h.commit_engine_activation(&manager, &ready),
        Err(Error::Conflict)
    ));
    drop(ready);
    wait_for(|| h.recognition_status().reserved == 0);
    h.close().unwrap();
    let store = Store::open(root.path().join("library.db")).unwrap();
    assert_eq!(
        store.engine_version(id, &Access::public()).unwrap().state,
        Installation::Installed
    );
}
#[test]
fn busy_manager_rejects_without_writer_wait_and_preserves_candidate_for_retry() {
    let (_root, h, id) = fixture(8);
    let ready = prepared(&h, id);
    let manager = Arc::new(Mutex::new(Manager::new()));
    let held = manager.lock().unwrap();
    assert!(matches!(
        h.commit_engine_activation(&manager, &ready),
        Err(Error::ResourceLimit)
    ));
    // This request completes while the controller still holds the Manager lock.
    call(
        &h,
        Command::Help {
            topic: stem_core::help::Topic::Recognition,
        },
    );
    drop(held);
    h.commit_engine_activation(&manager, &ready).unwrap();
    drop(ready);
    h.close().unwrap();
}
#[test]
fn publication_rejects_foreign_scope_and_stale_generation_without_installing() {
    let (root, h, id) = fixture(8);
    let (_other_root, other, _) = fixture(8);
    let ready = prepared(&h, id);
    let manager = Arc::new(Mutex::new(Manager::new()));
    assert!(matches!(
        other.commit_engine_activation(&manager, &ready),
        Err(Error::Denied)
    ));
    call(
        &h,
        Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "generation changes".into(),
        },
    );
    assert!(matches!(
        h.commit_engine_activation(&manager, &ready),
        Err(Error::Conflict)
    ));
    assert!(manager.lock().unwrap().descriptor("synthetic").is_none());
    drop(ready);
    h.close().unwrap();
    other.close().unwrap();
    let store = Store::open(root.path().join("library.db")).unwrap();
    assert_eq!(
        store.engine_version(id, &Access::public()).unwrap().state,
        Installation::Staged
    );
}
#[test]
fn pen_cancellation_prevents_host_publication_after_preparation() {
    let (_root, h, id) = fixture(8);
    let ready = prepared(&h, id);
    let manager = Arc::new(Mutex::new(Manager::new()));
    let pen = Id::new();
    h.set_capture_priority(pen, true).unwrap();
    h.set_capture_priority(pen, false).unwrap();
    assert!(matches!(
        h.commit_engine_activation(&manager, &ready),
        Err(Error::Cancelled)
    ));
    assert!(manager.lock().unwrap().descriptor("synthetic").is_none());
    drop(ready);
    h.close().unwrap();
}

#[test]
fn replaced_runtime_cleanup_is_off_writer_and_keeps_library_ownership() {
    let (root, h, id) = fixture(8);
    let manager = Arc::new(Mutex::new(Manager::new()));
    let (cleanup, entered, release) = gate();
    let (old, _) = model(None, Some(cleanup));
    {
        let mut m = manager.lock().unwrap();
        let mut d = descriptor();
        d.version = "old".into();
        m.register_recogniser(d, old).unwrap();
        m.activate_local("synthetic", None, true).unwrap();
    }
    let ready = prepared(&h, id);
    h.commit_engine_activation(&manager, &ready).unwrap();
    assert_eq!(
        manager
            .lock()
            .unwrap()
            .descriptor("synthetic")
            .unwrap()
            .version,
        "1"
    );
    drop(ready);
    let retirement_thread = entered.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_ne!(retirement_thread, thread::current().id());
    // The writer and Manager are available while the old SDK destructor blocks.
    call(
        &h,
        Command::Help {
            topic: stem_core::help::Topic::Recognition,
        },
    );
    assert!(manager.try_lock().is_ok());
    let h2 = Arc::clone(&h);
    let closer = thread::spawn(move || h2.close().unwrap());
    assert!(Host::open(root.path().join("library.db"), 8).is_err());
    release.send(()).unwrap();
    closer.join().unwrap();
    Host::open(root.path().join("library.db"), 8)
        .unwrap()
        .close()
        .unwrap();
}

struct RegistryAuth {
    gate: Gate,
    manager: Arc<Mutex<Manager>>,
}
impl stem_core::security::Authenticator for RegistryAuth {
    fn verify(&mut self, _: Id, _: &std::collections::BTreeSet<Scope>) -> bool {
        self.gate.wait();
        // The writer's verifier may inspect the registry. The queued publisher
        // must hold a strong lease, never the Manager mutex while awaiting RPC.
        assert!(self.manager.try_lock().is_ok());
        true
    }
}
#[test]
fn queued_publication_does_not_lock_registry_while_writer_authenticates() {
    let (_root, h, id) = fixture(8);
    let ready = prepared(&h, id);
    let manager = Arc::new(Mutex::new(Manager::new()));
    let (gate, entered, release) = gate();
    let auth = RegistryAuth {
        gate,
        manager: Arc::clone(&manager),
    };
    let h_auth = Arc::clone(&h);
    let auth_thread = thread::spawn(move || {
        h_auth.authenticate([Scope::App].into_iter().collect(), Box::new(auth))
    });
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let publisher_host = Arc::clone(&h);
    let publisher_manager = Arc::clone(&manager);
    let mut status = h.subscribe_status();
    let publisher = thread::spawn(move || {
        let result = publisher_host.commit_engine_activation(&publisher_manager, &ready);
        drop(ready);
        result
    });
    // Observe admission while the verifier still occupies the writer.
    loop {
        if status
            .next_timeout(Duration::from_secs(10))
            .unwrap()
            .expect("publication admission")
            .pending_queries
            > 0
        {
            break;
        }
    }
    release.send(()).unwrap();
    auth_thread.join().unwrap().unwrap();
    // Authentication invalidates the old optional authority, so publication may
    // be cancelled. In either case it must return without a registry deadlock.
    let result = publisher.join().unwrap();
    assert!(result.is_ok() || matches!(result, Err(Error::Cancelled)));
    h.close().unwrap();
}

struct SharedModel {
    cleanup: Option<Gate>,
    host: Option<std::sync::Weak<Host>>,
}
impl Recogniser for SharedModel {
    fn probe(&self) -> Result<()> {
        Ok(())
    }
    fn recognise(
        &self,
        ink: &InkSnapshot,
        _: Task,
        control: &JobControl,
    ) -> Result<RecognitionOutput> {
        control.check()?;
        assert!(!ink.strokes().is_empty());
        Ok(RecognitionOutput {
            candidates: vec![stem_core::content::Content::Text("1 + 1 = 3".into())],
            raw_output: "1 + 1 = 3".into(),
            confidence: None,
            diagnostics: vec![],
        })
    }
}
impl Drop for SharedModel {
    fn drop(&mut self) {
        assert_eq!(thread::current().name(), Some("stem-runtime-retirement"));
        if let Some(h) = self.host.as_ref().and_then(|h| h.upgrade()) {
            assert!(matches!(h.close(), Err(Error::WorkerFailed)));
        }
        if let Some(gate) = &self.cleanup {
            gate.wait();
        }
    }
}
fn data(h: &Host, command: Command) -> serde_json::Value {
    let response = h
        .request(
            &serde_json::to_string(&Request {
                version: 1,
                command,
            })
            .unwrap(),
        )
        .unwrap();
    assert!(response.ok, "{:?}", response.error);
    response.data.unwrap()
}
fn ink_block(h: &Host) -> Id {
    let note = Id::parse(
        data(
            h,
            Command::Create {
                kind: Kind::Note,
                parent: None,
                title: "private runtime".into(),
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    Id::parse(
        data(
            h,
            Command::Capture {
                ticket: Id::new(),
                entity: note,
                strokes: vec![stroke(0)],
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}
#[test]
fn host_private_runtime_recognises_into_a_review_without_rewriting_source() {
    let (root, h, id) = fixture(8);
    let block = ink_block(&h);
    let original = data(&h, Command::SourceInk { block });
    let ready = h
        .engine_activation_plan(id)
        .unwrap()
        .prepare_recogniser(
            Box::new(SharedModel {
                cleanup: None,
                host: None,
            }),
            100,
            &control(),
        )
        .unwrap();
    h.install_prepared_engine(&ready).unwrap();
    drop(ready);
    let work = h
        .prepare_runtime_recognition(block, None, Task::Prose, Some("synthetic"))
        .unwrap();
    let completed = thread::spawn(move || work.run(&control()))
        .join()
        .unwrap()
        .unwrap();
    let proposal = h.publish_recognition(completed).unwrap();
    assert_eq!(
        data(
            &h,
            Command::Proposal {
                proposal: proposal.id
            }
        )["output"]["raw_output"],
        "1 + 1 = 3"
    );
    assert_eq!(data(&h, Command::SourceInk { block }), original);
    h.close().unwrap();
    let reopened = Host::open(root.path().join("library.db"), 8).unwrap();
    // Durable installed metadata does not recreate a runtime after restart.
    assert!(reopened
        .prepare_runtime_recognition(block, None, Task::Prose, Some("synthetic"))
        .is_err());
    assert_eq!(data(&reopened, Command::SourceInk { block }), original);
    reopened.close().unwrap();
}
#[test]
fn host_close_retires_private_manager_off_writer_before_releasing_library() {
    let (root, h, id) = fixture(8);
    let (cleanup, entered, release) = gate();
    let ready = h
        .engine_activation_plan(id)
        .unwrap()
        .prepare_recogniser(
            Box::new(SharedModel {
                cleanup: Some(cleanup),
                host: Some(Arc::downgrade(&h)),
            }),
            100,
            &control(),
        )
        .unwrap();
    h.install_prepared_engine(&ready).unwrap();
    drop(ready);
    wait_for(|| h.recognition_status().reserved == 0);
    let closing_host = Arc::clone(&h);
    let (done, closed) = mpsc::channel();
    let closer = thread::spawn(move || {
        let result = closing_host.close();
        done.send(result).unwrap();
    });
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(closed.try_recv().is_err());
    assert!(Host::open(root.path().join("library.db"), 8).is_err());
    release.send(()).unwrap();
    closed
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    closer.join().unwrap();
    Host::open(root.path().join("library.db"), 8)
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn host_factory_runs_off_writer_and_remains_owned_until_cancelled_construction_returns() {
    let (root, h, id) = fixture(8);
    let plan = h.engine_activation_plan(id).unwrap();
    let (loading, entered, release) = gate();
    let (adapter, calls) = model(None, None);
    let worker = thread::spawn(move || {
        plan.prepare_recogniser_with(100, &control(), move |descriptor, resource, c| {
            assert_eq!(descriptor.id, "synthetic");
            assert!(resource.is_none());
            c.check()?;
            loading.wait();
            Ok(adapter)
        })
    });
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    call(
        &h,
        Command::Help {
            topic: stem_core::help::Topic::Recognition,
        },
    );
    let h2 = Arc::clone(&h);
    let closer = thread::spawn(move || h2.close());
    wait_for(|| h.recognition_status().closed);
    assert!(Host::open(root.path().join("library.db"), 8).is_err());
    release.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    assert_eq!(calls.load(Ordering::Acquire), 0);
    closer.join().unwrap().unwrap();
}
