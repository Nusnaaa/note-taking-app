use std::{
    collections::BTreeSet,
    io::{self, Cursor, Write},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};
use stem_core::{
    backup::{BackupAuthority, BackupAuthorizer, BackupOptions},
    content::Content,
    crypto::Credential,
    host::{Command, Host, Request},
    processing::JobControl,
    security::{Access, Authenticator, Scope},
    store::{Kind, Store},
    Error, Id,
};
const PASSWORD: &str = "Deliberate delegated backup test passphrase";
fn options() -> BackupOptions {
    BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    }
}
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(30)).unwrap()
}
struct Proof;
impl Authenticator for Proof {
    fn verify(&mut self, _: Id, _: &BTreeSet<Scope>) -> bool {
        true
    }
}
struct Reject;
impl Authenticator for Reject {
    fn verify(&mut self, _: Id, _: &BTreeSet<Scope>) -> bool {
        false
    }
}
struct Expected(BackupAuthority);
impl BackupAuthorizer for Expected {
    fn verify(&mut self, _: Id, authority: &BackupAuthority) -> bool {
        self.0 == *authority
    }
}
fn wire(command: Command) -> String {
    serde_json::to_string(&Request {
        version: 1,
        command,
    })
    .unwrap()
}
fn fixture(app: bool) -> (tempfile::TempDir, Arc<Host>, Id, BackupAuthority) {
    let root = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let path = root.path().join("library.db");
    let mut store = Store::open(&path).unwrap();
    let public = Access::public();
    let note = store
        .create(Kind::Note, None, "locked source", &public)
        .unwrap();
    store
        .append_content(
            note.id,
            Content::Text("original protected scientific content".into()),
            &public,
        )
        .unwrap();
    let access = Access::authenticate(
        &mut Proof,
        [Scope::App, Scope::Entity(note.id)].into_iter().collect(),
    )
    .unwrap();
    let revision = store.entity(note.id, &public).unwrap().revision;
    store.set_lock(note.id, revision, true, &access).unwrap();
    if app {
        store.set_app_lock(true, &access).unwrap();
    }
    let expected = store
        .prepare_stream_backup(options(), &access)
        .unwrap()
        .authority()
        .unwrap();
    drop(store);
    let host = Arc::new(Host::open(path, 8).unwrap());
    (root, host, note.id, expected)
}
#[derive(Clone, Default)]
struct Bytes(Arc<Mutex<Vec<u8>>>);
impl Write for Bytes {
    fn write(&mut self, value: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(value);
        Ok(value.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn delegated_encryption_preserves_app_and_note_locks_and_independent_restore() {
    let (root, host, note, expected) = fixture(true);
    assert!(expected.protected_content);
    assert!(!host.request(&wire(Command::Read { id: note })).unwrap().ok);
    assert!(matches!(host.review_backup(options()), Err(Error::Denied)));
    let review = host
        .review_backup_delegated(options(), Box::new(Expected(expected.clone())))
        .unwrap();
    assert_eq!(review.summary().authority, expected);
    assert_eq!(expected.library.len(), 36);
    assert!(host
        .backup_snapshot_matches(
            expected.library.clone(),
            review.summary().generation.clone()
        )
        .unwrap());
    let bytes = Bytes::default();
    let receipt = host
        .finish_backup(
            review,
            PASSWORD,
            Box::new(Reject),
            true,
            bytes.clone(),
            &control(),
        )
        .unwrap();
    assert!(!host.request(&wire(Command::Read { id: note })).unwrap().ok);
    let stage = tempfile::tempdir_in(root.path()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(stage.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let encrypted = bytes.0.lock().unwrap().clone();
    assert!(encrypted.starts_with(b"STEMENC2"));
    let restored = Store::restore_stream_backup(
        Cursor::new(encrypted),
        Credential::Password(PASSWORD),
        10_000_000,
        stage.path(),
        &control(),
    )
    .unwrap();
    let recovered = root.path().join("independent.db");
    restored
        .publish(&recovered, &mut Proof, true, &control())
        .unwrap();
    let copy = Store::open(&recovered).unwrap();
    assert!(matches!(
        copy.entity(note, &Access::public()),
        Err(Error::Denied)
    ));
    let access = Access::authenticate(
        &mut Proof,
        [Scope::App, Scope::Entity(note)].into_iter().collect(),
    )
    .unwrap();
    assert_eq!(copy.blocks(note, 0, 100, &access).unwrap().len(), 1);
    assert!(receipt.recovery_key().len() == 64);
    host.close().unwrap();
}
#[test]
fn wrong_library_digest_and_backup_options_cannot_reuse_approval() {
    let (_root, host, _note, expected) = fixture(false);
    for altered in [
        BackupAuthority {
            library: Id::new().to_string(),
            ..expected.clone()
        },
        BackupAuthority {
            scope_digest: "0".repeat(64),
            ..expected.clone()
        },
    ] {
        assert!(matches!(
            host.review_backup_delegated(options(), Box::new(Expected(altered))),
            Err(Error::Denied)
        ));
        assert!(!host.backup_status().reserved);
    }
    let changed = BackupOptions {
        include_authorised_source_assets: true,
        ..options()
    };
    assert!(matches!(
        host.review_backup_delegated(changed, Box::new(Expected(expected))),
        Err(Error::Denied)
    ));
    host.close().unwrap();
}
#[test]
fn new_protected_scope_requires_new_approval_and_does_not_unlock_ordinary_reads() {
    let (_root, host, old, expected) = fixture(false);
    let created = host
        .request(&wire(Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "new private area".into(),
        }))
        .unwrap();
    assert!(created.ok);
    let new = Id::parse(created.data.unwrap()["id"].as_str().unwrap()).unwrap();
    host.authenticate([Scope::Entity(new)].into_iter().collect(), Box::new(Proof))
        .unwrap();
    assert!(
        host.request(&wire(Command::SetLock {
            entity: new,
            revision: 0,
            locked: true
        }))
        .unwrap()
        .ok
    );
    assert!(host.request(&wire(Command::Relock {})).unwrap().ok);
    assert!(matches!(
        host.review_backup_delegated(options(), Box::new(Expected(expected))),
        Err(Error::Denied)
    ));
    assert!(!host.request(&wire(Command::Read { id: old })).unwrap().ok);
    assert!(!host.request(&wire(Command::Read { id: new })).unwrap().ok);
    host.close().unwrap();
}
#[test]
fn approved_scope_remains_usable_for_new_authored_content_without_unlocking_host_reads() {
    let (root, host, note, expected) = fixture(false);
    host.close().unwrap();
    let path = root.path().join("library.db");
    let mut store = Store::open(&path).unwrap();
    let access =
        Access::authenticate(&mut Proof, [Scope::Entity(note)].into_iter().collect()).unwrap();
    store
        .append_content(
            note,
            Content::Text("new deliberate revision".into()),
            &access,
        )
        .unwrap();
    assert_eq!(
        store
            .prepare_stream_backup(options(), &access)
            .unwrap()
            .authority()
            .unwrap(),
        expected
    );
    drop(store);
    let next = Host::open(path, 8).unwrap();
    let review = next
        .review_backup_delegated(options(), Box::new(Expected(expected)))
        .unwrap();
    let bytes = Bytes::default();
    assert!(next
        .finish_backup(
            review,
            PASSWORD,
            Box::new(Reject),
            true,
            bytes.clone(),
            &control()
        )
        .is_ok());
    assert!(!bytes.0.lock().unwrap().is_empty());
    assert!(!next.request(&wire(Command::Read { id: note })).unwrap().ok);
    next.close().unwrap();
}
struct BlockingAuth {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
impl Authenticator for BlockingAuth {
    fn verify(&mut self, _: Id, _: &BTreeSet<Scope>) -> bool {
        self.entered.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(10)).unwrap();
        true
    }
}
#[test]
fn fresh_interactive_verification_runs_off_writer_and_rechecks_generation() {
    let (_root, host, note, _expected) = fixture(false);
    host.authenticate([Scope::Entity(note)].into_iter().collect(), Box::new(Proof))
        .unwrap();
    let review = host.review_backup(options()).unwrap();
    let (entered, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let bytes = Bytes::default();
    let observer = bytes.clone();
    let worker_host = host.clone();
    let worker = thread::spawn(move || {
        worker_host.finish_backup(
            review,
            PASSWORD,
            Box::new(BlockingAuth {
                entered,
                release: blocked,
            }),
            true,
            bytes,
            &control(),
        )
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    // Controlled deadlock bound, not a target-device latency promise.
    let (reply, result) = mpsc::channel();
    let writer_host = host.clone();
    let writer = thread::spawn(move || {
        reply
            .send(writer_host.request(&wire(Command::Create {
                kind: Kind::Note,
                parent: None,
                title: "author while authenticating".into(),
            })))
            .unwrap()
    });
    let response = result.recv_timeout(Duration::from_secs(2));
    release.send(()).unwrap();
    writer.join().unwrap();
    assert!(response.unwrap().unwrap().ok);
    assert!(matches!(worker.join().unwrap(), Err(Error::Conflict)));
    assert!(observer.0.lock().unwrap().is_empty());
    host.close().unwrap();
}
struct BlockingDelegation {
    expected: BackupAuthority,
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
impl BackupAuthorizer for BlockingDelegation {
    fn verify(&mut self, _: Id, authority: &BackupAuthority) -> bool {
        self.entered.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(10)).unwrap();
        *authority == self.expected
    }
}
#[test]
fn delegated_verification_is_off_writer_and_capture_invalidates_pending_review() {
    let (_root, host, _note, expected) = fixture(false);
    let (entered, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let worker_host = host.clone();
    let worker = thread::spawn(move || {
        worker_host.review_backup_delegated(
            options(),
            Box::new(BlockingDelegation {
                expected,
                entered,
                release: blocked,
            }),
        )
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(host.request(&wire(Command::Status {})).unwrap().ok);
    let contact = Id::new();
    host.set_capture_priority(contact, true).unwrap();
    release.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    host.set_capture_priority(contact, false).unwrap();
    assert!(!host.backup_status().reserved);
    host.close().unwrap();
}
struct DropBlocked {
    expected: BackupAuthority,
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
impl BackupAuthorizer for DropBlocked {
    fn verify(&mut self, _: Id, authority: &BackupAuthority) -> bool {
        *authority == self.expected
    }
}
impl Drop for DropBlocked {
    fn drop(&mut self) {
        self.entered.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(10)).unwrap();
    }
}
#[test]
fn close_retains_exclusive_ownership_through_actual_authorizer_destruction() {
    let (root, host, _note, expected) = fixture(false);
    let (entered, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let worker_host = host.clone();
    let worker = thread::spawn(move || {
        worker_host.review_backup_delegated(
            options(),
            Box::new(DropBlocked {
                expected,
                entered,
                release: blocked,
            }),
        )
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let closer = host.clone();
    let (done, closed) = mpsc::channel();
    let closing = thread::spawn(move || done.send(closer.close()).unwrap());
    assert!(closed.recv_timeout(Duration::from_millis(30)).is_err());
    assert!(Host::open(root.path().join("library.db"), 8).is_err());
    release.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    closed
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    closing.join().unwrap();
    Host::open(root.path().join("library.db"), 8)
        .unwrap()
        .close()
        .unwrap();
}
