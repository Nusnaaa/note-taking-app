mod common;
use common::*;
use std::{
    collections::BTreeSet,
    io::{self, Cursor, Read},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, MutexGuard, OnceLock, Weak,
    },
    thread,
    time::Duration,
};
use stem_core::{
    backup::{BackupOptions, RestorePublication},
    content::Content,
    crypto::Credential,
    host::{Command, Host, Request},
    processing::JobControl,
    security::{Access, Authenticator, Scope},
    store::{Kind, Order, Store, WritingTarget},
    Error, Id,
};
const PASSWORD: &str = "Deliberate host restored-copy passphrase";
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(300)).unwrap()
}
fn private() -> tempfile::TempDir {
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder.tempdir().unwrap()
}
fn open_host(dir: &tempfile::TempDir) -> Arc<Host> {
    Arc::new(Host::open(dir.path().join("live.db"), 8).unwrap())
}
fn create(host: &Host) {
    let request = serde_json::to_string(&Request {
        version: 1,
        command: Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "live authoring".into(),
        },
    })
    .unwrap();
    assert!(host.request(&request).unwrap().ok);
}
fn archive(legacy: bool, protected: bool) -> (Vec<u8>, String, Id, Id, stem_core::ink::Stroke) {
    let mut store = Store::memory().unwrap();
    let public = Access::public();
    let note = store
        .create(Kind::Note, None, "restored source", &public)
        .unwrap();
    let original = stroke(0);
    let block = store
        .append_capture(
            Id::new(),
            WritingTarget::Entity(note.id),
            vec![original.clone()],
            &public,
        )
        .unwrap();
    let access = if protected {
        let grant = access([Scope::Entity(note.id)]);
        store
            .set_lock(
                note.id,
                store.entity(note.id, &grant).unwrap().revision,
                true,
                &grant,
            )
            .unwrap();
        grant
    } else {
        public
    };
    let options = BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    };
    let (bytes, key) = if legacy {
        let plan = store.prepare_backup(options, &access).unwrap();
        let finished = store
            .finish_backup(plan, PASSWORD, &mut TestAuth(true), true)
            .unwrap();
        (finished.bytes, finished.recovery_key.to_string())
    } else {
        let plan = store.prepare_stream_backup(options, &access).unwrap();
        let (bytes, key) = store
            .finish_stream_backup(
                plan,
                PASSWORD,
                &mut TestAuth(true),
                true,
                Vec::new(),
                &control(),
            )
            .unwrap();
        (bytes, key.to_string())
    };
    (bytes, key, note.id, block.id, original)
}
struct Owned {
    input: Cursor<Vec<u8>>,
    retired: Arc<AtomicBool>,
}
impl Read for Owned {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.input.read(out)
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        self.retired.store(true, Ordering::SeqCst);
    }
}
#[test]
fn current_and_legacy_password_and_recovery_roundtrip_exact_original_ink() {
    let _serial = serial();
    for legacy in [false, true] {
        let (bytes, key, note, block, original) = archive(legacy, true);
        for recovery in [false, true] {
            let owner = private();
            let staging = private();
            let output = private();
            let host = open_host(&owner);
            let retired = Arc::new(AtomicBool::new(false));
            let review = host
                .prepare_restore(
                    Owned {
                        input: Cursor::new(bytes.clone()),
                        retired: Arc::clone(&retired),
                    },
                    if recovery {
                        Credential::Recovery(&key)
                    } else {
                        Credential::Password(PASSWORD)
                    },
                    16 * 1024 * 1024,
                    staging.path(),
                    &control(),
                )
                .unwrap();
            assert!(retired.load(Ordering::SeqCst));
            assert!(review.is_current());
            assert_eq!(review.summary().strokes, 1);
            assert_eq!(review.summary().protected_scopes, 1);
            let destination = output.path().join("recovered.db");
            assert_eq!(
                host.publish_restore(review, &destination, &mut TestAuth(true), true, &control())
                    .unwrap(),
                RestorePublication::Published
            );
            assert!(!host.restore_status().reserved);
            assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 0);
            let restored = Store::open(&destination).unwrap();
            assert!(matches!(
                restored.block(block, &Access::public()),
                Err(Error::Denied)
            ));
            let grant = access([Scope::Entity(note)]);
            assert_eq!(
                restored.source_ink(block, &grant).unwrap(),
                vec![original.clone()]
            );
            create(&host);
            host.close().unwrap();
        }
    }
}
#[test]
fn publication_requires_confirmation_and_one_fresh_proof_even_when_unprotected() {
    let _serial = serial();
    let (bytes, _, _, _, _) = archive(false, false);
    let owner = private();
    let staging = private();
    let output = private();
    let host = open_host(&owner);
    for (confirmed, accepted) in [(false, true), (true, false)] {
        let review = host
            .prepare_restore(
                Cursor::new(bytes.clone()),
                Credential::Password(PASSWORD),
                16 * 1024 * 1024,
                staging.path(),
                &control(),
            )
            .unwrap();
        assert!(host
            .publish_restore(
                review,
                &output.path().join("rejected.db"),
                &mut TestAuth(accepted),
                confirmed,
                &control()
            )
            .is_err());
        assert!(!output.path().join("rejected.db").exists());
        assert!(!host.restore_status().reserved);
    }
    struct Once(usize);
    impl Authenticator for Once {
        fn verify(&mut self, _: Id, scopes: &BTreeSet<Scope>) -> bool {
            self.0 += 1;
            assert!(scopes.contains(&Scope::App));
            self.0 == 1
        }
    }
    let review = host
        .prepare_restore(
            Cursor::new(bytes),
            Credential::Password(PASSWORD),
            16 * 1024 * 1024,
            staging.path(),
            &control(),
        )
        .unwrap();
    let mut proof = Once(0);
    assert_eq!(
        host.publish_restore(
            review,
            &output.path().join("accepted.db"),
            &mut proof,
            true,
            &control()
        )
        .unwrap(),
        RestorePublication::Published
    );
    assert_eq!(proof.0, 1);
    host.close().unwrap();
}
#[test]
fn wrong_credentials_malformed_finality_and_budgets_retire_input_and_scratch() {
    let _serial = serial();
    let (bytes, _, _, _, _) = archive(false, false);
    let owner = private();
    let staging = private();
    let host = open_host(&owner);
    for mode in 0..5 {
        let mut bad = bytes.clone();
        let mut password = PASSWORD;
        let mut budget = 16 * 1024 * 1024;
        match mode {
            0 => password = "Wrong deliberate restore credential",
            1 => {
                bad.pop();
            }
            2 => bad.push(9),
            3 => budget = 1,
            _ => bad[0] ^= 1,
        }
        let retired = Arc::new(AtomicBool::new(false));
        assert!(host
            .prepare_restore(
                Owned {
                    input: Cursor::new(bad),
                    retired: Arc::clone(&retired)
                },
                Credential::Password(password),
                budget,
                staging.path(),
                &control()
            )
            .is_err());
        assert!(retired.load(Ordering::SeqCst));
        assert!(!host.restore_status().reserved);
        assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 0);
        create(&host);
    }
    let (legacy, _, _, _, _) = archive(true, false);
    assert!(host
        .prepare_restore(
            Cursor::new(legacy),
            Credential::Password(PASSWORD),
            1,
            staging.path(),
            &control()
        )
        .is_err());
    host.close().unwrap();
}
#[test]
fn single_slot_and_retained_closed_review_do_not_strand_host_close() {
    let _serial = serial();
    let (bytes, _, _, _, _) = archive(false, false);
    let owner = private();
    let staging = private();
    let host = open_host(&owner);
    let review = host
        .prepare_restore(
            Cursor::new(bytes.clone()),
            Credential::Password(PASSWORD),
            16 * 1024 * 1024,
            staging.path(),
            &control(),
        )
        .unwrap();
    assert!(matches!(
        host.prepare_restore(
            Cursor::new(bytes),
            Credential::Password(PASSWORD),
            16 * 1024 * 1024,
            staging.path(),
            &control()
        ),
        Err(Error::ResourceLimit)
    ));
    host.close().unwrap();
    assert!(!review.is_current());
    assert!(!host.restore_status().reserved);
    assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 0);
    Host::open(owner.path().join("live.db"), 8)
        .unwrap()
        .close()
        .unwrap();
}
struct Blocked {
    input: Cursor<Vec<u8>>,
    entered: Option<mpsc::Sender<()>>,
    release: mpsc::Receiver<()>,
    retired: Arc<AtomicBool>,
    drop_block: bool,
}
impl Read for Blocked {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if !self.drop_block {
            if let Some(entered) = self.entered.take() {
                entered.send(()).unwrap();
                self.release.recv().unwrap();
            }
        }
        self.input.read(out)
    }
}
impl Drop for Blocked {
    fn drop(&mut self) {
        if self.drop_block {
            self.entered.take().unwrap().send(()).unwrap();
            self.release.recv().unwrap();
        }
        self.retired.store(true, Ordering::SeqCst);
    }
}
#[test]
fn close_retains_native_sqlite_ownership_through_read_and_reader_destructor_retirement() {
    let _serial = serial();
    let (bytes, _, _, _, _) = archive(false, false);
    for drop_block in [false, true] {
        let owner = private();
        let staging = private();
        let host = open_host(&owner);
        let (entered, observed) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let retired = Arc::new(AtomicBool::new(false));
        let input = Blocked {
            input: Cursor::new(bytes.clone()),
            entered: Some(entered),
            release: resume,
            retired: Arc::clone(&retired),
            drop_block,
        };
        let worker_host = Arc::clone(&host);
        let path = staging.path().to_owned();
        let work = thread::spawn(move || {
            worker_host.prepare_restore(
                input,
                Credential::Password(PASSWORD),
                16 * 1024 * 1024,
                &path,
                &control(),
            )
        });
        observed.recv_timeout(Duration::from_secs(20)).unwrap();
        create(&host);
        let closer_host = Arc::clone(&host);
        let (done, finished) = mpsc::channel();
        let closer = thread::spawn(move || {
            done.send(closer_host.close()).unwrap();
        });
        assert!(finished.recv_timeout(Duration::from_millis(100)).is_err());
        assert!(Host::open(owner.path().join("live.db"), 8).is_err());
        assert!(!retired.load(Ordering::SeqCst));
        release.send(()).unwrap();
        assert!(work.join().unwrap().is_err());
        finished
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap();
        closer.join().unwrap();
        assert!(retired.load(Ordering::SeqCst));
        assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 0);
        Host::open(owner.path().join("live.db"), 8)
            .unwrap()
            .close()
            .unwrap();
    }
}
#[test]
fn capture_cancels_idle_and_running_restore_without_reviving_it_at_pen_up() {
    let _serial = serial();
    let (bytes, _, _, _, _) = archive(false, false);
    let owner = private();
    let staging = private();
    let host = open_host(&owner);
    let stale = host
        .prepare_restore(
            Cursor::new(bytes.clone()),
            Credential::Password(PASSWORD),
            16 * 1024 * 1024,
            staging.path(),
            &control(),
        )
        .unwrap();
    let contact = Id::new();
    host.set_capture_priority(contact, true).unwrap();
    host.set_capture_priority(contact, false).unwrap();
    assert!(!stale.is_current());
    let fresh = host
        .prepare_restore(
            Cursor::new(bytes.clone()),
            Credential::Password(PASSWORD),
            16 * 1024 * 1024,
            staging.path(),
            &control(),
        )
        .unwrap();
    drop(fresh);
    drop(stale);
    let (entered, observed) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let input = Blocked {
        input: Cursor::new(bytes),
        entered: Some(entered),
        release: resume,
        retired: Arc::new(AtomicBool::new(false)),
        drop_block: false,
    };
    let worker = Arc::clone(&host);
    let path = staging.path().to_owned();
    let work = thread::spawn(move || {
        worker.prepare_restore(
            input,
            Credential::Password(PASSWORD),
            16 * 1024 * 1024,
            &path,
            &control(),
        )
    });
    observed.recv_timeout(Duration::from_secs(10)).unwrap();
    let contact = Id::new();
    host.set_capture_priority(contact, true).unwrap();
    host.set_capture_priority(contact, false).unwrap();
    release.send(()).unwrap();
    assert!(work.join().unwrap().is_err());
    create(&host);
    host.close().unwrap();
}
struct PanicReader {
    retired: Arc<AtomicBool>,
}
impl Read for PanicReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("synthetic restore reader panic")
    }
}
impl Drop for PanicReader {
    fn drop(&mut self) {
        self.retired.store(true, Ordering::SeqCst);
        panic!("synthetic reader destructor panic")
    }
}
#[test]
fn read_and_drop_panics_are_separately_contained_and_authoring_survives() {
    let _serial = serial();
    let owner = private();
    let staging = private();
    let host = open_host(&owner);
    let retired = Arc::new(AtomicBool::new(false));
    assert!(matches!(
        host.prepare_restore(
            PanicReader {
                retired: Arc::clone(&retired)
            },
            Credential::Password(PASSWORD),
            1024,
            staging.path(),
            &control()
        ),
        Err(Error::WorkerFailed)
    ));
    assert!(retired.load(Ordering::SeqCst));
    assert!(!host.restore_status().reserved);
    create(&host);
    host.close().unwrap();
}
#[test]
fn reentrant_close_from_reader_drop_and_verifier_cannot_self_join_or_publish() {
    let _serial = serial();
    let (bytes, _, _, _, _) = archive(false, false);
    struct Closing {
        input: Cursor<Vec<u8>>,
        host: Weak<Host>,
    }
    impl Read for Closing {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            self.input.read(output)
        }
    }
    impl Drop for Closing {
        fn drop(&mut self) {
            assert!(self.host.upgrade().unwrap().close().is_err());
        }
    }
    let owner = private();
    let staging = private();
    let host = open_host(&owner);
    assert!(host
        .prepare_restore(
            Closing {
                input: Cursor::new(bytes.clone()),
                host: Arc::downgrade(&host)
            },
            Credential::Password(PASSWORD),
            16 * 1024 * 1024,
            staging.path(),
            &control()
        )
        .is_err());
    host.close().unwrap();
    let host = Host::open(owner.path().join("live.db"), 8).unwrap();
    struct ClosingProof<'a>(&'a Host);
    impl Authenticator for ClosingProof<'_> {
        fn verify(&mut self, _: Id, _: &BTreeSet<Scope>) -> bool {
            assert!(self.0.close().is_err());
            true
        }
    }
    let review = host
        .prepare_restore(
            Cursor::new(bytes),
            Credential::Password(PASSWORD),
            16 * 1024 * 1024,
            staging.path(),
            &control(),
        )
        .unwrap();
    let destination = staging.path().join("must-not-publish.db");
    assert!(host
        .publish_restore(
            review,
            &destination,
            &mut ClosingProof(&host),
            true,
            &control()
        )
        .is_err());
    assert!(!destination.exists());
    host.close().unwrap();
}
#[test]
fn verifier_is_off_live_writer_and_an_independent_copy_does_not_replace_live_edits() {
    let _serial = serial();
    let (bytes, _, _, _, _) = archive(false, false);
    let owner = private();
    let staging = private();
    let output = private();
    let host = open_host(&owner);
    let review = host
        .prepare_restore(
            Cursor::new(bytes),
            Credential::Password(PASSWORD),
            16 * 1024 * 1024,
            staging.path(),
            &control(),
        )
        .unwrap();
    struct Authoring<'a>(&'a Host);
    impl Authenticator for Authoring<'_> {
        fn verify(&mut self, _: Id, _: &BTreeSet<Scope>) -> bool {
            create(self.0);
            true
        }
    }
    let destination = output.path().join("independent.db");
    assert_eq!(
        host.publish_restore(
            review,
            &destination,
            &mut Authoring(&host),
            true,
            &control()
        )
        .unwrap(),
        RestorePublication::Published
    );
    assert_eq!(
        Store::open(&destination)
            .unwrap()
            .list(None, Order::Name, None, None, &Access::public())
            .unwrap()
            .len(),
        1
    );
    host.close().unwrap();
}
#[test]
fn publication_cannot_replace_existing_live_or_unrelated_files_or_accept_foreign_reviews() {
    let _serial = serial();
    let (bytes, _, _, _, _) = archive(false, false);
    let owner = private();
    let other_dir = private();
    let staging = private();
    let host = open_host(&owner);
    let other = open_host(&other_dir);
    let existing = owner.path().join("existing");
    std::fs::write(&existing, b"keep").unwrap();
    for target in [existing.clone(), owner.path().join("live.db")] {
        let review = host
            .prepare_restore(
                Cursor::new(bytes.clone()),
                Credential::Password(PASSWORD),
                16 * 1024 * 1024,
                staging.path(),
                &control(),
            )
            .unwrap();
        assert!(host
            .publish_restore(review, &target, &mut TestAuth(true), true, &control())
            .is_err());
        create(&host);
    }
    assert_eq!(std::fs::read(existing).unwrap(), b"keep");
    let review = host
        .prepare_restore(
            Cursor::new(bytes),
            Credential::Password(PASSWORD),
            16 * 1024 * 1024,
            staging.path(),
            &control(),
        )
        .unwrap();
    assert!(matches!(
        other.publish_restore(
            review,
            &other_dir.path().join("foreign.db"),
            &mut TestAuth(true),
            true,
            &control()
        ),
        Err(Error::Denied)
    ));
    host.close().unwrap();
    other.close().unwrap();
}
#[test]
fn stream_restore_through_host_exceeds_the_legacy_whole_archive_limit() {
    let _serial = serial();
    let mut source = Store::memory().unwrap();
    let public = Access::public();
    let note = source
        .create(Kind::Note, None, "above legacy bound", &public)
        .unwrap();
    for _ in 0..66 {
        source
            .append_content(note.id, Content::Text("x".repeat(1024 * 1024)), &public)
            .unwrap();
    }
    let original = stroke(0);
    let ink = source
        .append_ink(note.id, vec![original.clone()], &public)
        .unwrap();
    let plan = source
        .prepare_stream_backup(
            BackupOptions {
                include_original_ink: true,
                include_authorised_source_assets: false,
            },
            &public,
        )
        .unwrap();
    let (bytes, _) = source
        .finish_stream_backup(
            plan,
            PASSWORD,
            &mut TestAuth(true),
            true,
            Vec::new(),
            &control(),
        )
        .unwrap();
    assert!(bytes.len() > 64 * 1024 * 1024);
    drop(source);
    let owner = private();
    let staging = private();
    let output = private();
    let host = open_host(&owner);
    let review = host
        .prepare_restore(
            Cursor::new(bytes),
            Credential::Password(PASSWORD),
            256 * 1024 * 1024,
            staging.path(),
            &control(),
        )
        .unwrap();
    assert_eq!(review.summary().blocks, 67);
    assert_eq!(review.summary().strokes, 1);
    let destination = output.path().join("large.db");
    assert_eq!(
        host.publish_restore(review, &destination, &mut TestAuth(true), true, &control())
            .unwrap(),
        RestorePublication::Published
    );
    let recovered = Store::open(destination).unwrap();
    assert_eq!(
        recovered.source_ink(ink.id, &public).unwrap(),
        vec![original]
    );
    assert!(!host.restore_status().reserved);
    host.close().unwrap();
}
#[test]
fn expired_controls_retire_admitted_readers_without_retaining_a_stage() {
    let _serial = serial();
    let owner = private();
    let staging = private();
    let host = open_host(&owner);
    let retired = Arc::new(AtomicBool::new(false));
    let expired = JobControl::new(Duration::ZERO).unwrap();
    assert!(matches!(
        host.prepare_restore(
            Owned {
                input: Cursor::new(Vec::new()),
                retired: Arc::clone(&retired)
            },
            Credential::Password(PASSWORD),
            1024,
            staging.path(),
            &expired
        ),
        Err(Error::Timeout)
    ));
    assert!(retired.load(Ordering::SeqCst));
    assert!(!host.restore_status().reserved);
    create(&host);
    host.close().unwrap();
}
#[cfg(unix)]
#[test]
fn private_stage_and_plaintext_destination_reject_public_directories_and_symlinks() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let _serial = serial();
    let (bytes, _, _, _, _) = archive(false, false);
    let owner = private();
    let staging = private();
    let public = private();
    let host = open_host(&owner);
    std::fs::set_permissions(public.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        host.prepare_restore(
            Cursor::new(bytes.clone()),
            Credential::Password(PASSWORD),
            16 * 1024 * 1024,
            public.path(),
            &control()
        ),
        Err(Error::Denied)
    ));
    let link = owner.path().join("symlink-stage");
    symlink(staging.path(), &link).unwrap();
    assert!(host
        .prepare_restore(
            Cursor::new(bytes.clone()),
            Credential::Password(PASSWORD),
            16 * 1024 * 1024,
            &link,
            &control()
        )
        .is_err());
    let review = host
        .prepare_restore(
            Cursor::new(bytes),
            Credential::Password(PASSWORD),
            16 * 1024 * 1024,
            staging.path(),
            &control(),
        )
        .unwrap();
    assert!(matches!(
        host.publish_restore(
            review,
            &public.path().join("unsafe.db"),
            &mut TestAuth(true),
            true,
            &control()
        ),
        Err(Error::Denied)
    ));
    assert!(!public.path().join("unsafe.db").exists());
    assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 0);
    host.close().unwrap();
}
