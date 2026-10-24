mod common;
use common::*;
use serde_json::Value;
use std::{
    io::{self, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, Weak,
    },
    thread,
    time::{Duration, Instant},
};
use stem_core::{
    backup::BackupOptions,
    content::Content,
    crypto::{self, stream::DecryptStream},
    host::{Command, Host, Request, WriterLimits},
    processing::JobControl,
    security::{Authenticator, Scope},
    store::{Kind, Store},
    Error, Id,
};
const PASSWORD: &str = "Deliberate native backup test passphrase";
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(60)).unwrap()
}
fn options() -> BackupOptions {
    BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    }
}
fn wire(command: Command) -> String {
    serde_json::to_string(&Request {
        version: 1,
        command,
    })
    .unwrap()
}
fn call(host: &Host, command: Command) -> Value {
    let result = host.request(&wire(command)).unwrap();
    assert!(result.ok, "{:?}", result.error);
    result.data.unwrap_or(Value::Null)
}
fn fixture() -> (tempfile::TempDir, Arc<Host>, Id, Id) {
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(Host::open(dir.path().join("library.db"), 8).unwrap());
    let note = Id::parse(
        call(
            &host,
            Command::Create {
                kind: Kind::Note,
                parent: None,
                title: "native backup".into(),
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let block = Id::parse(
        call(
            &host,
            Command::Capture {
                ticket: Id::new(),
                entity: note,
                strokes: vec![stroke(0)],
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    (dir, host, note, block)
}
#[derive(Clone)]
struct Captured {
    bytes: Arc<Mutex<Vec<u8>>>,
    retired: Arc<AtomicBool>,
}
impl Captured {
    fn new() -> Self {
        Self {
            bytes: Arc::new(Mutex::new(Vec::new())),
            retired: Arc::new(AtomicBool::new(false)),
        }
    }
}
struct Sink(Captured);
impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.bytes.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Drop for Sink {
    fn drop(&mut self) {
        self.0.retired.store(true, Ordering::Release);
    }
}
struct BlockSink {
    sink: Sink,
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    armed: bool,
}
impl Write for BlockSink {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if self.armed {
            self.armed = false;
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(20)).unwrap();
        }
        self.sink.write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.sink.flush()
    }
}
fn blocked_sink() -> (Captured, BlockSink, mpsc::Receiver<()>, mpsc::Sender<()>) {
    let capture = Captured::new();
    let (entered, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    (
        capture.clone(),
        BlockSink {
            sink: Sink(capture),
            entered,
            release: blocked,
            armed: true,
        },
        waiting,
        release,
    )
}
fn wait_for(mut ready: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(
            Instant::now() < until,
            "native job did not reach expected state"
        );
        thread::yield_now();
    }
}
fn private_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    dir
}
#[test]
fn native_backup_retires_sink_before_receipt_and_restores_exact_original_ink() {
    let (_root, host, note, block) = fixture();
    let original: Vec<stem_core::ink::Stroke> =
        serde_json::from_value(call(&host, Command::SourceInk { block })).unwrap();
    call(
        &host,
        Command::Edit {
            block,
            revision: 0,
            content: Content::Text("keep incorrect 1+1=3".into()),
        },
    );
    let review = host.review_backup(options()).unwrap();
    assert!(review.summary().includes_original_ink);
    assert!(host.backup_status().reserved);
    assert!(!host.backup_status().running);
    let captured = Captured::new();
    let receipt = host
        .finish_backup(
            review,
            PASSWORD,
            Box::new(TestAuth(true)),
            true,
            Sink(captured.clone()),
            &control(),
        )
        .unwrap();
    assert!(captured.retired.load(Ordering::Acquire));
    assert!(!host.backup_status().reserved);
    let bytes = captured.bytes.lock().unwrap();
    let stage = private_dir();
    let restored = Store::restore_stream_backup(
        bytes.as_slice(),
        crypto::Credential::Recovery(receipt.recovery_key()),
        10 * 1024 * 1024,
        stage.path(),
        &control(),
    )
    .unwrap();
    assert_eq!(
        restored
            .store()
            .block(block, &stem_core::security::Access::public())
            .unwrap()
            .content,
        Content::Text("keep incorrect 1+1=3".into())
    );
    assert_eq!(
        restored
            .store()
            .source_ink(block, &stem_core::security::Access::public())
            .unwrap(),
        original
    );
    call(
        &host,
        Command::Append {
            entity: note,
            content: Content::Text("authoring remains usable".into()),
        },
    );
}
#[test]
fn native_review_confirmation_fresh_proof_foreign_host_and_closed_host_are_enforced() {
    let (root, host, _, block) = fixture();
    host.authenticate(
        [Scope::Block(block)].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    call(
        &host,
        Command::SetBlockLock {
            block,
            revision: 0,
            locked: true,
        },
    );
    let review = host.review_backup(options()).unwrap();
    let capture = Captured::new();
    assert!(matches!(
        host.finish_backup(
            review,
            PASSWORD,
            Box::new(TestAuth(false)),
            true,
            Sink(capture.clone()),
            &control()
        ),
        Err(Error::Denied)
    ));
    assert!(capture.bytes.lock().unwrap().is_empty());
    assert!(capture.retired.load(Ordering::Acquire));
    assert!(!host.backup_status().reserved);
    let review = host.review_backup(options()).unwrap();
    assert!(matches!(
        host.finish_backup(
            review,
            PASSWORD,
            Box::new(TestAuth(true)),
            false,
            Vec::new(),
            &control()
        ),
        Err(Error::ReviewRequired)
    ));
    assert!(!host.backup_status().reserved);
    let review = host.review_backup(options()).unwrap();
    assert!(matches!(
        host.review_backup(options()),
        Err(Error::ResourceLimit)
    ));
    let (_other_root, other, _, _) = fixture();
    assert!(matches!(
        other.finish_backup(
            review,
            PASSWORD,
            Box::new(TestAuth(true)),
            true,
            Vec::new(),
            &control()
        ),
        Err(Error::Denied)
    ));
    assert!(!host.backup_status().reserved);
    let review = host.review_backup(options()).unwrap();
    host.close().unwrap();
    assert!(!host.backup_status().reserved);
    let reopened = Host::open(root.path().join("library.db"), 8).unwrap();
    assert!(matches!(
        reopened.finish_backup(
            review,
            PASSWORD,
            Box::new(TestAuth(true)),
            true,
            Vec::new(),
            &control()
        ),
        Err(Error::Denied)
    ));
}
#[test]
fn blocked_sink_does_not_hold_writer_and_source_edits_invalidate_completion() {
    let (_root, host, note, _) = fixture();
    let review = host.review_backup(options()).unwrap();
    let (capture, sink, waiting, release) = blocked_sink();
    let copy = host.clone();
    let worker = thread::spawn(move || {
        copy.finish_backup(
            review,
            PASSWORD,
            Box::new(TestAuth(true)),
            true,
            sink,
            &control(),
        )
    });
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(host.backup_status().running);
    assert!(matches!(
        host.review_backup(options()),
        Err(Error::ResourceLimit)
    ));
    call(
        &host,
        Command::Capture {
            ticket: Id::new(),
            entity: note,
            strokes: vec![stroke(1)],
        },
    );
    release.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Err(Error::Conflict)));
    assert!(capture.retired.load(Ordering::Acquire));
    assert!(!host.backup_status().reserved);
    let bytes = capture.bytes.lock().unwrap();
    let mut reader = DecryptStream::new(
        bytes.as_slice(),
        crypto::Credential::Password(PASSWORD),
        10 * 1024 * 1024,
    )
    .unwrap();
    assert!(reader.read_to_end(&mut Vec::new()).is_err());
    assert_eq!(
        call(
            &host,
            Command::Blocks {
                entity: note,
                offset: 0,
                limit: 10
            }
        )
        .as_array()
        .unwrap()
        .len(),
        2
    );
}
#[test]
fn capture_cancels_running_and_idle_reviews_without_reviving_them_after_pen_up() {
    let (_root, host, _, _) = fixture();
    let review = host.review_backup(options()).unwrap();
    let contact = Id::new();
    host.set_capture_priority(contact, true).unwrap();
    assert!(matches!(
        host.finish_backup(
            review,
            PASSWORD,
            Box::new(TestAuth(true)),
            true,
            Vec::new(),
            &control()
        ),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        host.review_backup(options()),
        Err(Error::ResourceLimit)
    ));
    host.set_capture_priority(contact, false).unwrap();
    let old = host.review_backup(options()).unwrap();
    host.set_capture_priority(contact, true).unwrap();
    host.set_capture_priority(contact, false).unwrap();
    let review = host.review_backup(options()).unwrap();
    drop(old); // old holder cannot release the new reservation
    let (capture, sink, waiting, release) = blocked_sink();
    let copy = host.clone();
    let worker = thread::spawn(move || {
        copy.finish_backup(
            review,
            PASSWORD,
            Box::new(TestAuth(true)),
            true,
            sink,
            &control(),
        )
    });
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    host.set_capture_priority(contact, true).unwrap();
    host.set_capture_priority(contact, false).unwrap();
    release.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    assert!(capture.retired.load(Ordering::Acquire));
    assert!(!host.backup_status().reserved);
}
#[test]
fn close_waits_for_actual_sink_retirement_and_preserves_exclusive_library_ownership() {
    let (root, host, _, _) = fixture();
    let review = host.review_backup(options()).unwrap();
    let (capture, sink, waiting, release) = blocked_sink();
    let copy = host.clone();
    let worker = thread::spawn(move || {
        copy.finish_backup(
            review,
            PASSWORD,
            Box::new(TestAuth(true)),
            true,
            sink,
            &control(),
        )
    });
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    let closing = host.clone();
    let (tx, rx) = mpsc::channel();
    let closer = thread::spawn(move || {
        let result = closing.close();
        tx.send(result).unwrap();
    });
    wait_for(|| host.backup_status().closed);
    assert!(rx.try_recv().is_err());
    assert!(!capture.retired.load(Ordering::Acquire));
    assert!(matches!(
        Host::open(root.path().join("library.db"), 8),
        Err(Error::Unavailable)
    ));
    release.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    closer.join().unwrap();
    assert!(capture.retired.load(Ordering::Acquire));
    assert!(!host.backup_status().reserved);
    Host::open(root.path().join("library.db"), 8).unwrap();
}
struct ReentrantSink {
    host: Weak<Host>,
    result: Arc<AtomicBool>,
    panic_write: bool,
}
impl Write for ReentrantSink {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if self.panic_write {
            panic!("synthetic backup write panic");
        }
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Drop for ReentrantSink {
    fn drop(&mut self) {
        self.result.store(
            matches!(
                self.host.upgrade().unwrap().close(),
                Err(Error::WorkerFailed)
            ),
            Ordering::Release,
        );
    }
}
#[test]
fn sink_write_and_destructor_callbacks_cannot_self_join_or_publish_a_success_after_close() {
    for panic_write in [false, true] {
        let (_root, host, _, _) = fixture();
        let review = host.review_backup(options()).unwrap();
        let result = Arc::new(AtomicBool::new(false));
        let sink = ReentrantSink {
            host: Arc::downgrade(&host),
            result: result.clone(),
            panic_write,
        };
        assert!(host
            .finish_backup(
                review,
                PASSWORD,
                Box::new(TestAuth(true)),
                true,
                sink,
                &control()
            )
            .is_err());
        assert!(result.load(Ordering::Acquire));
        host.close().unwrap();
        assert!(!host.backup_status().reserved);
    }
}
#[test]
fn verifier_and_sink_panics_do_not_kill_authoring_or_leave_a_backup_reservation() {
    struct PanicProof;
    impl Authenticator for PanicProof {
        fn verify(&mut self, _: Id, _: &std::collections::BTreeSet<Scope>) -> bool {
            panic!("synthetic backup proof panic")
        }
    }
    struct PanicDrop;
    impl Write for PanicDrop {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Drop for PanicDrop {
        fn drop(&mut self) {
            panic!("synthetic backup destructor panic");
        }
    }
    struct PanicWrite;
    impl Write for PanicWrite {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            panic!("synthetic backup sink panic")
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let (_root, host, note, block) = fixture();
    host.authenticate(
        [Scope::Block(block)].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    call(
        &host,
        Command::SetBlockLock {
            block,
            revision: 0,
            locked: true,
        },
    );
    let review = host.review_backup(options()).unwrap();
    assert!(matches!(
        host.finish_backup(
            review,
            PASSWORD,
            Box::new(PanicProof),
            true,
            Vec::new(),
            &control()
        ),
        Err(Error::WorkerFailed)
    ));
    assert!(!host.backup_status().reserved);
    let review = host.review_backup(options()).unwrap();
    assert!(matches!(
        host.finish_backup(
            review,
            PASSWORD,
            Box::new(TestAuth(true)),
            true,
            PanicWrite,
            &control()
        ),
        Err(Error::WorkerFailed)
    ));
    assert!(!host.backup_status().reserved);
    let review = host.review_backup(options()).unwrap();
    assert!(matches!(
        host.finish_backup(
            review,
            PASSWORD,
            Box::new(TestAuth(true)),
            true,
            PanicDrop,
            &control()
        ),
        Err(Error::WorkerFailed)
    ));
    assert!(!host.backup_status().reserved);
    call(
        &host,
        Command::Append {
            entity: note,
            content: Content::Text("still writable".into()),
        },
    );
}
#[test]
fn reserved_cleanup_retires_backup_when_ordinary_queries_are_saturated() {
    struct BlockingProof {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl Authenticator for BlockingProof {
        fn verify(&mut self, _: Id, _: &std::collections::BTreeSet<Scope>) -> bool {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(20)).unwrap();
            true
        }
    }
    let root = tempfile::tempdir().unwrap();
    let host = Arc::new(
        Host::open_with_limits(
            root.path().join("library.db"),
            WriterLimits {
                authoring_count: 8,
                authoring_bytes: 32 * 1024 * 1024,
                query_count: 1,
                query_bytes: 8 * 1024 * 1024,
            },
        )
        .unwrap(),
    );
    call(
        &host,
        Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "cleanup".into(),
        },
    );
    let review = host.review_backup(options()).unwrap();
    let (_, sink, waiting, release_sink) = blocked_sink();
    let copy = host.clone();
    let worker = thread::spawn(move || {
        copy.finish_backup(
            review,
            PASSWORD,
            Box::new(TestAuth(true)),
            true,
            sink,
            &control(),
        )
    });
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    let (entered, entered_rx) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let copy = host.clone();
    let authentication = thread::spawn(move || {
        copy.authenticate(
            [Scope::App].into_iter().collect(),
            Box::new(BlockingProof {
                entered,
                release: blocked,
            }),
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let queued = host.submit(&wire(Command::Status {})).unwrap();
    assert!(matches!(
        host.submit(&wire(Command::Status {})),
        Err(Error::ResourceLimit)
    ));
    release_sink.send(()).unwrap();
    let mut status = host.subscribe_status();
    loop {
        let next = status
            .next_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap();
        if next.pending_queries == 2 {
            break;
        }
    }
    release.send(()).unwrap();
    authentication.join().unwrap().unwrap();
    assert!(queued.wait().unwrap().ok);
    assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    assert!(!host.backup_status().reserved);
}

#[test]
fn finished_ciphertext_does_not_release_ownership_while_sink_destruction_is_blocked() {
    struct DropBarrier {
        sink: Sink,
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl Write for DropBarrier {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.sink.write(b)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.sink.flush()
        }
    }
    impl Drop for DropBarrier {
        fn drop(&mut self) {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(20)).unwrap();
        }
    }
    let (root, host, _, _) = fixture();
    let review = host.review_backup(options()).unwrap();
    let captured = Captured::new();
    let (entered, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let sink = DropBarrier {
        sink: Sink(captured.clone()),
        entered,
        release: blocked,
    };
    let copy = host.clone();
    let worker = thread::spawn(move || {
        copy.finish_backup(
            review,
            PASSWORD,
            Box::new(TestAuth(true)),
            true,
            sink,
            &control(),
        )
    });
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    // The stream is already complete, but no native receipt/retirement exists.
    {
        let bytes = captured.bytes.lock().unwrap();
        let mut reader = DecryptStream::new(
            bytes.as_slice(),
            crypto::Credential::Password(PASSWORD),
            10 * 1024 * 1024,
        )
        .unwrap();
        reader.read_to_end(&mut Vec::new()).unwrap();
        reader.finish().unwrap();
    }
    assert!(!captured.retired.load(Ordering::Acquire));
    assert!(host.backup_status().running);
    let copy = host.clone();
    let (tx, rx) = mpsc::channel();
    let closer = thread::spawn(move || tx.send(copy.close()).unwrap());
    wait_for(|| host.backup_status().closed);
    assert!(rx.try_recv().is_err());
    assert!(matches!(
        Host::open(root.path().join("library.db"), 8),
        Err(Error::Unavailable)
    ));
    release.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    closer.join().unwrap();
    assert!(captured.retired.load(Ordering::Acquire));
    assert!(!host.backup_status().reserved);
    Host::open(root.path().join("library.db"), 8).unwrap();
}

#[test]
fn native_host_streams_a_library_above_the_legacy_limit_without_releasing_its_writer_ownership() {
    let root = private_dir();
    let path = root.path().join("large.db");
    let public = stem_core::security::Access::public();
    let mut source = Store::open(&path).unwrap();
    let text = "Preserve incorrect native backup working 1+1=3.\n".repeat(25000);
    let mut blocks = Vec::new();
    let mut first = None;
    for i in 0..24 {
        let note = source
            .create(Kind::Note, None, &format!("large native note {i}"), &public)
            .unwrap();
        first.get_or_insert(note.id);
        blocks.push(
            source
                .append_content(note.id, Content::Text(text.clone()), &public)
                .unwrap()
                .id,
        );
    }
    let original = stroke(0);
    let ink = source
        .append_ink(first.unwrap(), vec![original.clone()], &public)
        .unwrap();
    assert!(matches!(
        source.prepare_backup(options(), &public),
        Err(Error::ResourceLimit)
    ));
    drop(source);
    let host = Host::open(path.clone(), 8).unwrap();
    let review = host.review_backup(options()).unwrap();
    let archive = root.path().join("encrypted.native-backup");
    let sink = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&archive)
        .unwrap();
    let budget = JobControl::new(Duration::from_secs(300)).unwrap();
    let receipt = host
        .finish_backup(
            review,
            PASSWORD,
            Box::new(TestAuth(true)),
            true,
            sink,
            &budget,
        )
        .unwrap();
    assert!(std::fs::metadata(&archive).unwrap().len() > 64 * 1024 * 1024);
    assert!(!host.backup_status().reserved);
    assert!(matches!(
        Host::open(path.clone(), 8),
        Err(Error::Unavailable)
    ));
    let stage = private_dir();
    let restored = Store::restore_stream_backup(
        std::fs::File::open(&archive).unwrap(),
        crypto::Credential::Recovery(receipt.recovery_key()),
        256 * 1024 * 1024,
        stage.path(),
        &budget,
    )
    .unwrap();
    for id in blocks {
        assert_eq!(
            restored.store().block(id, &public).unwrap().content,
            Content::Text(text.clone())
        );
    }
    assert_eq!(
        restored.store().source_ink(ink.id, &public).unwrap(),
        vec![original]
    );
    host.close().unwrap();
    Host::open(path, 8).unwrap();
}
#[test]
fn optional_backup_metadata_budget_failure_keeps_authoring_available() {
    let root = tempfile::tempdir().unwrap();
    let host = Host::open_with_limits(
        root.path().join("small-budget.db"),
        WriterLimits {
            authoring_count: 8,
            authoring_bytes: 32 * 1024 * 1024,
            query_count: 1,
            query_bytes: 512,
        },
    )
    .unwrap();
    let note = Id::parse(
        call(
            &host,
            Command::Create {
                kind: Kind::Note,
                parent: None,
                title: "writer priority".into(),
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        host.review_backup(options()),
        Err(Error::ResourceLimit)
    ));
    assert!(!host.backup_status().reserved);
    call(
        &host,
        Command::Capture {
            ticket: Id::new(),
            entity: note,
            strokes: vec![stroke(0)],
        },
    );
    assert_eq!(
        call(
            &host,
            Command::Blocks {
                entity: note,
                offset: 0,
                limit: 10
            }
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
}
