use std::{
    io::{self, Cursor, Read},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};
use stem_core::{
    host::{Command, Host, Request},
    processing::*,
    provisioning::resource_digest,
    security::Access,
    store::*,
    Error, Id,
};
fn descriptor(bytes: &[u8]) -> Descriptor {
    Descriptor {
        id: "synthetic-host-import".into(),
        version: "1".into(),
        adapter_version: "1".into(),
        family: Family::Recogniser,
        input: InputContract::OrderedStrokesV1,
        tasks: vec![Task::Prose],
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some("app-owned synthetic host import; no real SDK".into()),
        redistributable_resources: false,
        resource_sha256: Some(resource_digest(bytes)),
        runtime_contract: "stem-processing-v1".into(),
    }
}
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(30)).unwrap()
}
fn fixture(capacity: usize) -> (tempfile::TempDir, Arc<Host>) {
    let root = tempfile::tempdir().unwrap();
    let h = Arc::new(Host::open(root.path().join("library.db"), capacity).unwrap());
    (root, h)
}
fn call(h: &Host, command: Command) {
    assert!(
        h.request(
            &serde_json::to_string(&Request {
                version: 1,
                command
            })
            .unwrap()
        )
        .unwrap()
        .ok
    );
}
struct Gate {
    entered: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl Gate {
    fn wait(&self) {
        self.entered.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
    }
}
fn gate() -> (Gate, mpsc::Receiver<()>, mpsc::Sender<()>) {
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
struct Reader {
    bytes: Cursor<Vec<u8>>,
    read: Option<Gate>,
    drop: Option<Gate>,
}
impl Read for Reader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if let Some(g) = self.read.take() {
            g.wait();
        }
        self.bytes.read(output)
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        if let Some(g) = &self.drop {
            g.wait();
        }
    }
}
#[test]
fn metadata_reservation_is_bounded_and_completed_import_does_not_load_a_runtime() {
    let (root, h) = fixture(1);
    let bytes = b"synthetic host resource";
    let models = root.path().join("models");
    let plan = h
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true)
        .unwrap();
    assert!(!models.exists());
    assert!(matches!(
        h.engine_import_plan(descriptor(bytes), bytes.len() as u64, true),
        Err(Error::ResourceLimit)
    ));
    let transfer = plan.start(&models, None, &control()).unwrap();
    let id = transfer.id();
    assert_eq!(transfer.committed_bytes(), 0);
    let prepared = transfer.prepare(Cursor::new(bytes), &control()).unwrap();
    let version = h.commit_engine_import(&prepared).unwrap();
    assert_eq!(version.id, id);
    assert_eq!(version.state, Installation::Staged);
    assert_eq!(h.commit_engine_import(&prepared).unwrap(), version);
    drop(prepared);
    h.close().unwrap();
    let s = Store::open(root.path().join("library.db")).unwrap();
    assert_eq!(s.engine_version(id, &Access::public()).unwrap(), version);
    assert!(s
        .active_engine("synthetic-host-import", &Access::public())
        .unwrap()
        .is_none());
}
#[test]
fn authoring_runs_during_blocked_reader_and_pen_cancel_preserves_explicit_resume() {
    let (root, h) = fixture(8);
    let bytes = b"synthetic cancellable host resource";
    let models = root.path().join("models");
    let transfer = h
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true)
        .unwrap()
        .start(&models, None, &control())
        .unwrap();
    let id = transfer.id();
    let (read, entered, release) = gate();
    let reader = Reader {
        bytes: Cursor::new(bytes.to_vec()),
        read: Some(read),
        drop: None,
    };
    let worker = thread::spawn(move || transfer.prepare(reader, &control()));
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    call(
        &h,
        Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "saved while model reader is blocked".into(),
        },
    );
    let pen = Id::new();
    h.set_capture_priority(pen, true).unwrap();
    h.set_capture_priority(pen, false).unwrap();
    release.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    let transfer = h
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true)
        .unwrap()
        .start(&models, Some(id), &control())
        .unwrap();
    assert_eq!(transfer.committed_bytes(), 0);
    let prepared = transfer.prepare(Cursor::new(bytes), &control()).unwrap();
    assert_eq!(h.commit_engine_import(&prepared).unwrap().id, id);
    drop(prepared);
    h.close().unwrap();
}
#[test]
fn close_waits_for_reader_return_and_actual_reader_destructor_before_reopen() {
    let (root, h) = fixture(8);
    let bytes = b"reader ownership fixture";
    let models = root.path().join("models");
    let transfer = h
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true)
        .unwrap()
        .start(&models, None, &control())
        .unwrap();
    let (read, entered, release) = gate();
    let (drop, dropping, release_drop) = gate();
    let reader = Reader {
        bytes: Cursor::new(bytes.to_vec()),
        read: Some(read),
        drop: Some(drop),
    };
    let worker = thread::spawn(move || transfer.prepare(reader, &control()));
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let closing = Arc::clone(&h);
    let (done, closed) = mpsc::channel();
    let closer = thread::spawn(move || done.send(closing.close()).unwrap());
    assert!(Host::open(root.path().join("library.db"), 8).is_err());
    assert!(closed.try_recv().is_err());
    release.send(()).unwrap();
    dropping.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(Host::open(root.path().join("library.db"), 8).is_err());
    assert!(closed.try_recv().is_err());
    release_drop.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
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
struct ErrorOwner {
    host: Arc<Host>,
    gate: Gate,
    self_wait: Arc<AtomicBool>,
}
impl std::fmt::Debug for ErrorOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "private provider diagnostic")
    }
}
impl std::fmt::Display for ErrorOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "private provider diagnostic")
    }
}
impl std::error::Error for ErrorOwner {}
impl Drop for ErrorOwner {
    fn drop(&mut self) {
        self.self_wait.store(
            matches!(self.host.close(), Err(Error::WorkerFailed)),
            Ordering::Release,
        );
        self.gate.wait();
    }
}
struct ErrorReader(Option<ErrorOwner>);
impl Read for ErrorReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other(self.0.take().unwrap()))
    }
}
#[test]
fn provider_error_payload_retires_inside_running_scope_and_cannot_escape_with_library_owner() {
    let (root, h) = fixture(8);
    let bytes = b"error payload fixture";
    let models = root.path().join("models");
    let transfer = h
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true)
        .unwrap()
        .start(&models, None, &control())
        .unwrap();
    let (gate, dropping, release) = gate();
    let observed = Arc::new(AtomicBool::new(false));
    let reader = ErrorReader(Some(ErrorOwner {
        host: Arc::clone(&h),
        gate,
        self_wait: Arc::clone(&observed),
    }));
    let worker = thread::spawn(move || transfer.prepare(reader, &control()));
    dropping.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(observed.load(Ordering::Acquire));
    assert_eq!(h.recognition_status().running, 1);
    assert!(Host::open(root.path().join("library.db"), 8).is_err());
    release.send(()).unwrap();
    let error = worker.join().unwrap().err().unwrap();
    assert!(matches!(&error,Error::Io(e) if e.kind()==io::ErrorKind::Other));
    assert!(!error.to_string().contains("private"));
    h.close().unwrap();
    Host::open(root.path().join("library.db"), 8)
        .unwrap()
        .close()
        .unwrap();
}
#[test]
fn foreign_host_and_stale_generation_cannot_publish_and_fresh_resume_keeps_transfer_id() {
    let (root, h) = fixture(8);
    let (_other, other) = fixture(8);
    let bytes = b"generation bound resource";
    let models = root.path().join("models");
    let transfer = h
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true)
        .unwrap()
        .start(&models, None, &control())
        .unwrap();
    let id = transfer.id();
    let prepared = transfer.prepare(Cursor::new(bytes), &control()).unwrap();
    assert!(matches!(
        other.commit_engine_import(&prepared),
        Err(Error::Denied)
    ));
    call(
        &h,
        Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "generation change".into(),
        },
    );
    assert!(matches!(
        h.commit_engine_import(&prepared),
        Err(Error::Conflict)
    ));
    drop(prepared);
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while h.recognition_status().reserved != 0 {
        assert!(
            std::time::Instant::now() < until,
            "previous import owner did not retire"
        );
        thread::sleep(Duration::from_millis(1));
    }
    let resumed = h
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true)
        .unwrap()
        .start(&models, Some(id), &control())
        .unwrap();
    assert_eq!(resumed.committed_bytes(), bytes.len() as u64);
    let prepared = resumed
        .prepare(Cursor::new(Vec::<u8>::new()), &control())
        .unwrap();
    assert_eq!(h.commit_engine_import(&prepared).unwrap().id, id);
    drop(prepared);
    h.close().unwrap();
    other.close().unwrap();
}

#[test]
fn host_prepared_publication_owns_transfer_until_actual_retirement() {
    use stem_core::provisioning::Transfer;
    let (root, h) = fixture(8);
    let bytes = b"held publication fixture";
    let models = root.path().join("models");
    let transfer = h
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true)
        .unwrap()
        .start(&models, None, &control())
        .unwrap();
    let id = transfer.id();
    let prepared = transfer.prepare(Cursor::new(bytes), &control()).unwrap();
    assert!(matches!(
        Transfer::resume(&models, id),
        Err(Error::ResourceLimit)
    ));
    h.commit_engine_import(&prepared).unwrap();
    assert!(matches!(
        Transfer::resume(&models, id),
        Err(Error::ResourceLimit)
    ));
    drop(prepared);
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while h.recognition_status().reserved != 0 {
        assert!(std::time::Instant::now() < until);
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        Transfer::resume(&models, id).unwrap().committed_bytes(),
        bytes.len() as u64
    );
    h.close().unwrap();
}
#[test]
fn cancelled_prepared_import_retires_ownership_without_staging_metadata() {
    use stem_core::provisioning::Transfer;
    let (root, h) = fixture(8);
    let bytes = b"cancelled ownership fixture";
    let models = root.path().join("models");
    let transfer = h
        .engine_import_plan(descriptor(bytes), bytes.len() as u64, true)
        .unwrap()
        .start(&models, None, &control())
        .unwrap();
    let id = transfer.id();
    let prepared = transfer.prepare(Cursor::new(bytes), &control()).unwrap();
    assert!(matches!(
        Transfer::resume(&models, id),
        Err(Error::ResourceLimit)
    ));
    let pen = Id::new();
    h.set_capture_priority(pen, true).unwrap();
    h.set_capture_priority(pen, false).unwrap();
    assert!(h.commit_engine_import(&prepared).is_err());
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while h.recognition_status().reserved != 0 {
        assert!(std::time::Instant::now() < until);
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        Transfer::resume(&models, id).unwrap().committed_bytes(),
        bytes.len() as u64
    );
    assert!(h.engine_catalogue(None, 8).unwrap().versions.is_empty());
    drop(prepared);
    h.close().unwrap();
}
