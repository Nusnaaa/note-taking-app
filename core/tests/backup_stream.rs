mod common;
use common::*;
use std::{
    io::{self, Read, Write},
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
    sources::Availability,
    store::{Kind, Store},
    Error,
};
fn private_dir() -> tempfile::TempDir {
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder.tempdir().unwrap()
}
const PASSWORD: &str = "Deliberate streaming library backup passphrase";
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(300)).unwrap()
}
fn options() -> BackupOptions {
    BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    }
}
fn archive(store: &Store, access: &Access) -> (Vec<u8>, String) {
    let plan = store.prepare_stream_backup(options(), access).unwrap();
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
}
fn entries(dir: &std::path::Path) -> Vec<std::ffi::OsString> {
    let mut names = std::fs::read_dir(dir)
        .unwrap()
        .map(|v| v.unwrap().file_name())
        .collect::<Vec<_>>();
    names.sort();
    names
}
#[test]
fn incremental_library_restore_preserves_ink_undo_and_source_relationships_then_publishes_without_overwrite(
) {
    let access = Access::public();
    let mut source = Store::memory().unwrap();
    let note = source.create(Kind::Note, None, "master", &access).unwrap();
    let block = source
        .append_ink(note.id, vec![stroke(0)], &access)
        .unwrap();
    source
        .edit_content(
            block.id,
            0,
            Content::Text("intentionally wrong 1+1=3".into()),
            &access,
        )
        .unwrap();
    let original = source.source_ink(block.id, &access).unwrap();
    let identity = source.library_identity(&access).unwrap();
    use sha2::{Digest, Sha256};
    let hash = format!("{:x}", Sha256::digest(b"%PDF external relationship"));
    let pdf = source
        .link_pdf(note.id, "content://not-a-grant", &hash, &access)
        .unwrap();
    let (bytes, key) = archive(&source, &access);
    let dir = private_dir();
    let mut restored = Store::restore_stream_backup(
        bytes.as_slice(),
        crypto::Credential::Recovery(&key),
        10 * 1024 * 1024,
        dir.path(),
        &control(),
    )
    .unwrap();
    assert_eq!(
        restored
            .store()
            .source(pdf.id, &access)
            .unwrap()
            .availability,
        Availability::Missing
    );
    assert_ne!(
        restored.store().library_identity(&access).unwrap(),
        identity
    );
    assert_eq!(
        restored.store().source_ink(block.id, &access).unwrap(),
        original
    );
    assert_eq!(
        restored.store().block(block.id, &access).unwrap().content,
        Content::Text("intentionally wrong 1+1=3".into())
    );
    restored.store_mut().undo(note.id, &access).unwrap();
    assert!(matches!(
        restored.store().block(block.id, &access).unwrap().content,
        Content::Ink
    ));
    restored.store_mut().redo(note.id, &access).unwrap();
    let output = private_dir();
    let path = output.path().join("restored.db");
    restored
        .store()
        .persist_restored_library(&path, &access, &mut TestAuth(true), true)
        .unwrap();
    assert!(restored
        .store()
        .persist_restored_library(&path, &access, &mut TestAuth(true), true)
        .is_err());
    drop(restored);
    assert!(entries(dir.path()).is_empty());
    let published = Store::open(&path).unwrap();
    assert_eq!(published.source_ink(block.id, &access).unwrap(), original);
    assert_eq!(source.source_ink(block.id, &access).unwrap(), original);
    source.check_integrity().unwrap();
}
#[test]
fn stream_review_requires_ink_consent_fresh_scope_and_current_source_generation_before_any_output()
{
    let public = Access::public();
    let mut store = Store::memory().unwrap();
    let note = store.create(Kind::Note, None, "private", &public).unwrap();
    let block = store.append_ink(note.id, vec![stroke(0)], &public).unwrap();
    assert!(matches!(
        store.prepare_stream_backup(
            BackupOptions {
                include_original_ink: false,
                ..options()
            },
            &public
        ),
        Err(Error::ReviewRequired)
    ));
    let scoped = access([Scope::Block(block.id)]);
    store.set_block_lock(block.id, 0, true, &scoped).unwrap();
    assert!(matches!(
        store.prepare_stream_backup(options(), &public),
        Err(Error::Denied)
    ));
    let mut output = Vec::new();
    let plan = store.prepare_stream_backup(options(), &scoped).unwrap();
    assert!(plan.includes_original_ink());
    assert!(matches!(
        store.finish_stream_backup(
            plan,
            PASSWORD,
            &mut TestAuth(false),
            true,
            &mut output,
            &control()
        ),
        Err(Error::Denied)
    ));
    assert!(output.is_empty());
    let plan = store.prepare_stream_backup(options(), &scoped).unwrap();
    assert!(matches!(
        store.finish_stream_backup(
            plan,
            PASSWORD,
            &mut TestAuth(true),
            false,
            &mut output,
            &control()
        ),
        Err(Error::ReviewRequired)
    ));
    assert!(output.is_empty());
    let plan = store.prepare_stream_backup(options(), &scoped).unwrap();
    store
        .rename(
            note.id,
            store.entity(note.id, &public).unwrap().revision,
            "changed",
            &public,
        )
        .unwrap();
    assert!(matches!(
        store.finish_stream_backup(
            plan,
            PASSWORD,
            &mut TestAuth(true),
            true,
            &mut output,
            &control()
        ),
        Err(Error::Conflict)
    ));
    assert!(output.is_empty());
    let job = control();
    job.cancel();
    let plan = store.prepare_stream_backup(options(), &scoped).unwrap();
    assert!(matches!(
        store.finish_stream_backup(plan, PASSWORD, &mut TestAuth(true), true, &mut output, &job),
        Err(Error::Cancelled)
    ));
    assert!(output.is_empty());
    let (bytes, key) = archive(&store, &scoped);
    let dir = private_dir();
    let restored = Store::restore_stream_backup(
        bytes.as_slice(),
        crypto::Credential::Recovery(&key),
        10 * 1024 * 1024,
        dir.path(),
        &control(),
    )
    .unwrap();
    assert!(matches!(
        restored.store().block(block.id, &public),
        Err(Error::Denied)
    ));
    assert!(restored.store().block(block.id, &scoped).is_ok());
    let output = private_dir();
    assert!(restored
        .store()
        .persist_restored_library(
            &output.path().join("denied.db"),
            &public,
            &mut TestAuth(true),
            true
        )
        .is_err());
    assert!(entries(output.path()).is_empty());
}
#[test]
fn truncation_trailing_data_malformed_tables_and_budget_failures_leave_no_restore_scratch() {
    let public = Access::public();
    let mut source = Store::memory().unwrap();
    source.create(Kind::Note, None, "small", &public).unwrap();
    let (bytes, key) = archive(&source, &public);
    let dir = private_dir();
    for malformed in [&bytes[..bytes.len() - 1], &bytes[..187]] {
        assert!(Store::restore_stream_backup(
            malformed,
            crypto::Credential::Recovery(&key),
            10 * 1024 * 1024,
            dir.path(),
            &control()
        )
        .is_err());
        assert!(entries(dir.path()).is_empty());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(Store::restore_stream_backup(
        trailing.as_slice(),
        crypto::Credential::Recovery(&key),
        10 * 1024 * 1024,
        dir.path(),
        &control()
    )
    .is_err());
    assert!(entries(dir.path()).is_empty());
    assert!(Store::restore_stream_backup(
        bytes.as_slice(),
        crypto::Credential::Recovery(&key),
        1,
        dir.path(),
        &control()
    )
    .is_err());
    assert!(entries(dir.path()).is_empty());
    assert!(Store::restore_stream_backup(
        bytes.as_slice(),
        crypto::Credential::Password("wrong password"),
        10 * 1024 * 1024,
        dir.path(),
        &control()
    )
    .is_err());
    assert!(entries(dir.path()).is_empty());
    let mut reader = DecryptStream::new(
        bytes.as_slice(),
        crypto::Credential::Recovery(&key),
        10 * 1024 * 1024,
    )
    .unwrap();
    let mut plain = Vec::new();
    reader.read_to_end(&mut plain).unwrap();
    // No omissions: the first fixed-table marker follows magic/schema/count.
    assert_eq!(&plain[..8], b"STEMLIB2");
    plain[16..18].copy_from_slice(&u16::MAX.to_be_bytes());
    let mut writer = EncryptStream::new(Vec::new(), PASSWORD).unwrap();
    writer.write_all(&plain).unwrap();
    let (malformed, key) = writer.finish().unwrap();
    assert!(Store::restore_stream_backup(
        malformed.as_slice(),
        crypto::Credential::Recovery(&key),
        10 * 1024 * 1024,
        dir.path(),
        &control()
    )
    .is_err());
    assert!(entries(dir.path()).is_empty());
}
#[test]
fn cancelled_stream_restore_removes_private_staging_and_does_not_modify_source() {
    struct CancelRead<'a> {
        bytes: &'a [u8],
        control: JobControl,
        read: usize,
    }
    impl Read for CancelRead<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let n = self.bytes.read(out)?;
            self.read += n;
            if self.read > 187 {
                self.control.cancel();
            }
            Ok(n)
        }
    }
    let public = Access::public();
    let mut source = Store::memory().unwrap();
    let note = source
        .create(Kind::Note, None, "still here", &public)
        .unwrap();
    let (bytes, key) = archive(&source, &public);
    let dir = private_dir();
    let job = control();
    let input = CancelRead {
        bytes: &bytes,
        control: job.clone(),
        read: 0,
    };
    assert!(matches!(
        Store::restore_stream_backup(
            input,
            crypto::Credential::Recovery(&key),
            10 * 1024 * 1024,
            dir.path(),
            &job
        ),
        Err(Error::Cancelled)
    ));
    assert!(entries(dir.path()).is_empty());
    assert_eq!(source.entity(note.id, &public).unwrap().title, "still here");
}

#[test]
fn authorised_retained_pdf_bytes_are_explicit_while_revoked_relationships_stay_revoked() {
    use sha2::{Digest, Sha256};
    use stem_core::sources::{AssetClass, RetentionChoice};
    let public = Access::public();
    let mut source = Store::memory().unwrap();
    let note = source
        .create(Kind::Note, None, "retention", &public)
        .unwrap();
    let bytes = b"%PDF authorised independent snapshot";
    let hash = format!("{:x}", Sha256::digest(bytes));
    let pdf = source
        .link_pdf(note.id, "file://external", &hash, &public)
        .unwrap();
    let asset = source
        .retain_source_bytes(
            pdf.id,
            bytes,
            RetentionChoice {
                class: AssetClass::HistoricalSnapshot,
                authority: Some("Synthetic independent retention consent"),
                export_allowed: true,
                consented: true,
            },
            &public,
        )
        .unwrap();
    source.revoke_source(pdf.id, &public).unwrap();
    let omitted = source.prepare_stream_backup(options(), &public).unwrap();
    assert_eq!(omitted.source_assets(), 0);
    assert!(!omitted.omissions().is_empty());
    let dir = private_dir();
    let (data, key) = source
        .finish_stream_backup(
            omitted,
            PASSWORD,
            &mut TestAuth(true),
            true,
            Vec::new(),
            &control(),
        )
        .unwrap();
    let omitted = Store::restore_stream_backup(
        data.as_slice(),
        crypto::Credential::Recovery(&key),
        10 * 1024 * 1024,
        dir.path(),
        &control(),
    )
    .unwrap();
    assert!(omitted.store().source_bytes(asset, true, &public).is_err());
    drop(omitted);
    assert!(entries(dir.path()).is_empty());
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
    let (data, key) = source
        .finish_stream_backup(
            plan,
            PASSWORD,
            &mut TestAuth(true),
            true,
            Vec::new(),
            &control(),
        )
        .unwrap();
    let restored = Store::restore_stream_backup(
        data.as_slice(),
        crypto::Credential::Recovery(&key),
        10 * 1024 * 1024,
        dir.path(),
        &control(),
    )
    .unwrap();
    assert_eq!(
        restored
            .store()
            .source(pdf.id, &public)
            .unwrap()
            .availability,
        Availability::Revoked
    );
    assert_eq!(
        restored
            .store()
            .source_bytes(asset, true, &public)
            .unwrap()
            .bytes,
        bytes
    );
}
#[test]
fn concurrent_generation_change_prevents_finalising_an_old_read_snapshot() {
    struct ChangeSink {
        bytes: Vec<u8>,
        path: std::path::PathBuf,
        changed: bool,
    }
    impl Write for ChangeSink {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            if !self.changed {
                let db = rusqlite::Connection::open(&self.path).unwrap();
                db.execute(
                    "UPDATE settings SET value=CAST(value AS INTEGER)+1 WHERE key='generation'",
                    [],
                )
                .unwrap();
                self.changed = true;
            }
            self.bytes.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let dir = private_dir();
    let path = dir.path().join("source.db");
    let public = Access::public();
    let mut store = Store::open(&path).unwrap();
    store.create(Kind::Note, None, "snapshot", &public).unwrap();
    let plan = store.prepare_stream_backup(options(), &public).unwrap();
    let mut sink = ChangeSink {
        bytes: Vec::new(),
        path,
        changed: false,
    };
    assert!(matches!(
        store.finish_stream_backup(
            plan,
            PASSWORD,
            &mut TestAuth(true),
            true,
            &mut sink,
            &control()
        ),
        Err(Error::Conflict)
    ));
    assert!(sink.changed);
    let mut reader = DecryptStream::new(
        sink.bytes.as_slice(),
        crypto::Credential::Password(PASSWORD),
        10 * 1024 * 1024,
    )
    .unwrap();
    assert!(reader.read_to_end(&mut Vec::new()).is_err());
    store.check_integrity().unwrap();
}
#[cfg(unix)]
#[test]
fn restore_rejects_shared_or_symlinked_directories_before_creating_private_content() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let public = Access::public();
    let source = Store::memory().unwrap();
    let (bytes, key) = archive(&source, &public);
    let dir = private_dir();
    let shared = dir.path().join("shared");
    std::fs::create_dir(&shared).unwrap();
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        Store::restore_stream_backup(
            bytes.as_slice(),
            crypto::Credential::Recovery(&key),
            10 * 1024 * 1024,
            &shared,
            &control()
        ),
        Err(Error::Denied)
    ));
    assert!(entries(&shared).is_empty());
    let private = private_dir();
    let link = dir.path().join("link");
    symlink(private.path(), &link).unwrap();
    assert!(Store::restore_stream_backup(
        bytes.as_slice(),
        crypto::Credential::Recovery(&key),
        10 * 1024 * 1024,
        &link,
        &control()
    )
    .is_err());
    assert!(entries(private.path()).is_empty());
}
#[test]
fn whole_library_larger_than_legacy_limit_roundtrips_with_incremental_encryption_and_private_sqlite_restore(
) {
    let root = private_dir();
    let public = Access::public();
    let mut source = Store::open(root.path().join("source.db")).unwrap();
    let content = "Preserve incorrect scientific working: 1+1=3.\n".repeat(25000);
    let mut blocks = Vec::new();
    let mut first = None;
    for i in 0..24 {
        let note = source
            .create(Kind::Note, None, &format!("large document {i}"), &public)
            .unwrap();
        first.get_or_insert(note.id);
        blocks.push(
            source
                .append_content(note.id, Content::Text(content.clone()), &public)
                .unwrap()
                .id,
        );
    }
    let ink = source
        .append_ink(first.unwrap(), vec![stroke(0)], &public)
        .unwrap();
    let original = source.source_ink(ink.id, &public).unwrap();
    assert!(matches!(
        source.prepare_backup(options(), &public),
        Err(Error::ResourceLimit)
    ));
    let plan = source.prepare_stream_backup(options(), &public).unwrap();
    let archive_path = root.path().join("encrypted.backup");
    let file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&archive_path)
        .unwrap();
    let (file, key) = source
        .finish_stream_backup(plan, PASSWORD, &mut TestAuth(true), true, file, &control())
        .unwrap();
    file.sync_all().unwrap();
    drop(file);
    assert!(std::fs::metadata(&archive_path).unwrap().len() > 64 * 1024 * 1024);
    let stage = private_dir();
    let restored = Store::restore_stream_backup(
        std::fs::File::open(&archive_path).unwrap(),
        crypto::Credential::Recovery(&key),
        256 * 1024 * 1024,
        stage.path(),
        &control(),
    )
    .unwrap();
    for block in blocks {
        assert_eq!(
            restored.store().block(block, &public).unwrap().content,
            Content::Text(content.clone())
        );
    }
    assert_eq!(
        restored.store().source_ink(ink.id, &public).unwrap(),
        original
    );
    restored.store().check_integrity().unwrap();
    drop(restored);
    assert!(entries(stage.path()).is_empty());
}
