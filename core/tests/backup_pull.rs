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

#[test]
fn replacing_or_relocking_a_cached_access_capability_never_reuses_its_old_scope_decision() {
    let public = Access::public();
    let mut source = Store::memory().unwrap();
    let note = source
        .create(Kind::Note, None, "current grant", &public)
        .unwrap();
    let block = source
        .append_content(note.id, Content::Text("x".repeat(200000)), &public)
        .unwrap();
    let mut current = access([Scope::Block(block.id)]);
    source.set_block_lock(block.id, 0, true, &current).unwrap();
    let control = job();
    let plan = source.prepare_stream_backup(options(), &current).unwrap();
    let mut cursor = source
        .open_stream_backup_cursor(plan, &mut TestAuth(true), true, &control)
        .unwrap();
    cursor
        .next_chunk(&source, &current, &control)
        .unwrap()
        .unwrap();
    cursor
        .next_chunk(&source, &current, &control)
        .unwrap()
        .unwrap();
    // Same Rust variable/address, newly authenticated but unrelated scopes.
    current = access([Scope::App]);
    assert!(matches!(
        cursor.next_chunk(&source, &current, &control),
        Err(Error::Denied)
    ));
    current = access([Scope::Block(block.id)]);
    assert!(matches!(
        cursor.next_chunk(&source, &current, &control),
        Err(Error::Cancelled)
    ));
    let plan = source.prepare_stream_backup(options(), &current).unwrap();
    let mut cursor = source
        .open_stream_backup_cursor(plan, &mut TestAuth(true), true, &control)
        .unwrap();
    cursor
        .next_chunk(&source, &current, &control)
        .unwrap()
        .unwrap();
    current.relock();
    assert!(matches!(
        cursor.next_chunk(&source, &current, &control),
        Err(Error::Denied)
    ));
}
#[test]
fn long_runs_of_live_caches_yield_and_never_export_cache_bytes_when_retained_assets_are_selected() {
    use sha2::{Digest, Sha256};
    use stem_core::sources::{AssetClass, RetentionChoice};
    let public = Access::public();
    let mut source = Store::memory().unwrap();
    let note = source
        .create(Kind::Note, None, "asset pulls", &public)
        .unwrap();
    let digest = format!("{:x}", Sha256::digest(b"%PDF external source"));
    let pdf = source
        .link_pdf(note.id, "content://local-source", &digest, &public)
        .unwrap();
    let cache = b"PRIVATE_LIVE_CACHE_ONLY_MARKER".repeat(2048);
    let mut cache_ids = Vec::new();
    for _ in 0..130 {
        cache_ids.push(
            source
                .retain_source_bytes(
                    pdf.id,
                    &cache,
                    RetentionChoice {
                        class: AssetClass::LiveCache,
                        authority: None,
                        export_allowed: false,
                        consented: true,
                    },
                    &public,
                )
                .unwrap(),
        );
    }
    let retained = b"%PDF retained snapshot permitted";
    let snapshot = source
        .retain_source_bytes(
            pdf.id,
            retained,
            RetentionChoice {
                class: AssetClass::HistoricalSnapshot,
                authority: Some("Synthetic explicit export authority"),
                export_allowed: true,
                consented: true,
            },
            &public,
        )
        .unwrap();
    let plan = source
        .prepare_stream_backup(
            BackupOptions {
                include_authorised_source_assets: true,
                ..options()
            },
            &public,
        )
        .unwrap();
    assert_eq!(plan.source_assets(), 1);
    let control = job();
    let mut cursor = source
        .open_stream_backup_cursor(plan, &mut TestAuth(true), true, &control)
        .unwrap();
    let mut output = Vec::new();
    let mut empty_yields = 0;
    while let Some(part) = cursor.next_chunk(&source, &public, &control).unwrap() {
        assert!(part.len() <= 64 * 1024);
        if part.is_empty() {
            empty_yields += 1;
        }
        output.extend_from_slice(&part);
    }
    cursor
        .validate_complete(&source, &public, &control)
        .unwrap();
    assert!(empty_yields > 0);
    assert!(!output
        .windows(b"PRIVATE_LIVE_CACHE_ONLY_MARKER".len())
        .any(|v| v == b"PRIVATE_LIVE_CACHE_ONLY_MARKER"));
    let mut encryptor = EncryptStream::new(Vec::new(), PASSWORD).unwrap();
    encryptor.write_all(&output).unwrap();
    let (bytes, key) = encryptor.finish().unwrap();
    let dir = tempfile::tempdir().unwrap();
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
        restored
            .store()
            .source_bytes(snapshot, true, &public)
            .unwrap()
            .bytes,
        retained
    );
    for id in cache_ids {
        assert!(restored.store().source_bytes(id, false, &public).is_err());
    }
}
