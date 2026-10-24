mod common;
use common::*;
use std::{
    io::{Read, Write},
    time::Duration,
};
use stem_core::{
    backup::BackupOptions,
    content::Content,
    crypto::{
        self,
        stream::{DecryptStream, EncryptStream},
    },
    processing::JobControl,
    security::{Access, Scope},
    store::{Kind, Store},
    Error,
};
const PASSWORD: &str = "Deliberate bounded backup pull passphrase";
fn job() -> JobControl {
    JobControl::new(Duration::from_secs(60)).unwrap()
}
fn options() -> BackupOptions {
    BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    }
}
#[test]
fn bounded_pulls_span_large_cells_and_restore_exact_source_with_independent_encryption() {
    let public = Access::public();
    let mut source = Store::memory().unwrap();
    let note = source
        .create(Kind::Note, None, "large cells", &public)
        .unwrap();
    let text = "Incorrect engineering working 1+1=3; ∫x² dx\n".repeat(5000);
    let block = source
        .append_content(note.id, Content::Text(text.clone()), &public)
        .unwrap();
    let ink = source
        .append_ink(note.id, vec![stroke(0)], &public)
        .unwrap();
    let plan = source.prepare_stream_backup(options(), &public).unwrap();
    let control = job();
    let mut cursor = source
        .open_stream_backup_cursor(plan, &mut TestAuth(true), true, &control)
        .unwrap();
    assert!(matches!(
        cursor.validate_complete(&source, &public, &control),
        Err(Error::ReviewRequired)
    ));
    assert!(matches!(
        cursor.next_chunk(&source, &public, &control),
        Err(Error::Cancelled)
    ));
    let plan = source.prepare_stream_backup(options(), &public).unwrap();
    let mut cursor = source
        .open_stream_backup_cursor(plan, &mut TestAuth(true), true, &control)
        .unwrap();
    let mut encrypted = EncryptStream::new(Vec::new(), PASSWORD).unwrap();
    let mut chunks = 0;
    while let Some(bytes) = cursor.next_chunk(&source, &public, &control).unwrap() {
        assert!(bytes.len() <= 64 * 1024);
        encrypted.write_all(&bytes).unwrap();
        chunks += 1;
    }
    assert!(chunks > 5);
    cursor
        .validate_complete(&source, &public, &control)
        .unwrap();
    let (bytes, key) = encrypted.finish().unwrap();
    let dir = tempfile::Builder::new().tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let restored = Store::restore_stream_backup(
        bytes.as_slice(),
        crypto::Credential::Recovery(&key),
        10 * 1024 * 1024,
        dir.path(),
        &control,
    )
    .unwrap();
    assert_eq!(
        restored.store().block(block.id, &public).unwrap().content,
        Content::Text(text)
    );
    assert_eq!(
        restored.store().source_ink(ink.id, &public).unwrap(),
        source.source_ink(ink.id, &public).unwrap()
    );
}
#[test]
fn current_scope_and_generation_are_checked_on_every_pull_and_failed_cursors_never_revive() {
    let public = Access::public();
    let mut source = Store::memory().unwrap();
    let note = source
        .create(Kind::Note, None, "protected", &public)
        .unwrap();
    let block = source
        .append_ink(note.id, vec![stroke(0)], &public)
        .unwrap();
    let scoped = access([Scope::Block(block.id)]);
    source.set_block_lock(block.id, 0, true, &scoped).unwrap();
    let control = job();
    let plan = source.prepare_stream_backup(options(), &scoped).unwrap();
    let mut cursor = source
        .open_stream_backup_cursor(plan, &mut TestAuth(true), true, &control)
        .unwrap();
    assert!(matches!(
        cursor.next_chunk(&source, &public, &control),
        Err(Error::Denied)
    ));
    assert!(matches!(
        cursor.next_chunk(&source, &scoped, &control),
        Err(Error::Cancelled)
    ));
    let plan = source.prepare_stream_backup(options(), &scoped).unwrap();
    let mut cursor = source
        .open_stream_backup_cursor(plan, &mut TestAuth(true), true, &control)
        .unwrap();
    assert!(cursor
        .next_chunk(&source, &scoped, &control)
        .unwrap()
        .is_some());
    source
        .append_content(
            note.id,
            Content::Text("safe authoring after a pull".into()),
            &public,
        )
        .unwrap();
    assert!(matches!(
        cursor.next_chunk(&source, &scoped, &control),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        cursor.validate_complete(&source, &scoped, &control),
        Err(Error::Cancelled)
    ));
    let plan = source.prepare_stream_backup(options(), &scoped).unwrap();
    let mut cursor = source
        .open_stream_backup_cursor(plan, &mut TestAuth(true), true, &control)
        .unwrap();
    control.cancel();
    assert!(matches!(
        cursor.next_chunk(&source, &scoped, &control),
        Err(Error::Cancelled)
    ));
}
#[test]
fn a_blocked_ciphertext_sink_holds_no_read_transaction_or_authoring_writer() {
    use std::sync::{mpsc, Arc, Mutex};
    struct BlockSink {
        output: Arc<Mutex<Vec<u8>>>,
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        armed: bool,
    }
    impl Write for BlockSink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if self.armed {
                self.armed = false;
                self.entered.send(()).unwrap();
                self.release.recv_timeout(Duration::from_secs(10)).unwrap();
            }
            self.output.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let public = Access::public();
    let mut source = Store::memory().unwrap();
    let note = source
        .create(Kind::Note, None, "writer first", &public)
        .unwrap();
    source
        .append_content(note.id, Content::Text("x".repeat(200000)), &public)
        .unwrap();
    let control = job();
    let plan = source.prepare_stream_backup(options(), &public).unwrap();
    let mut cursor = source
        .open_stream_backup_cursor(plan, &mut TestAuth(true), true, &control)
        .unwrap();
    let first = cursor
        .next_chunk(&source, &public, &control)
        .unwrap()
        .unwrap();
    let output = Arc::new(Mutex::new(Vec::new()));
    let (entered, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let (send, recv) = mpsc::channel();
    let captured = output.clone();
    let worker = std::thread::spawn(move || {
        let mut encryptor = EncryptStream::new(
            BlockSink {
                output: captured,
                entered,
                release: blocked,
                armed: true,
            },
            PASSWORD,
        )
        .unwrap(); // block during header I/O, away from Store
        encryptor.write_all(&first).unwrap();
        match recv.recv_timeout(Duration::from_secs(10)).unwrap() {
            true => panic!("stale export must not complete"),
            false => drop(encryptor),
        }
    });
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    let ink = source
        .append_ink(note.id, vec![stroke(0)], &public)
        .unwrap();
    assert_eq!(source.source_ink(ink.id, &public).unwrap().len(), 1);
    assert!(matches!(
        cursor.next_chunk(&source, &public, &control),
        Err(Error::Conflict)
    ));
    send.send(false).unwrap();
    release.send(()).unwrap();
    worker.join().unwrap();
    let output = output.lock().unwrap();
    let mut decryptor = DecryptStream::new(
        output.as_slice(),
        crypto::Credential::Password(PASSWORD),
        10 * 1024 * 1024,
    )
    .unwrap();
    assert!(decryptor.read_to_end(&mut Vec::new()).is_err());
    source.check_integrity().unwrap();
}
