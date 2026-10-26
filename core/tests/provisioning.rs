use stem_core::{processing::*, provisioning::*, Error};
fn descriptor(bytes: &[u8]) -> Descriptor {
    Descriptor {
        id: "local-fixture".into(),
        version: "1".into(),
        adapter_version: "1".into(),
        family: Family::Recogniser,
        input: InputContract::OrderedStrokesV1,
        tasks: vec![Task::Prose],
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some("app-authored fixture bytes".into()),
        redistributable_resources: false,
        resource_sha256: Some(resource_digest(bytes)),
        runtime_contract: "stem-processing-v1".into(),
    }
}
#[test]
fn local_install_resume_retry_and_integrity_precede_publication() {
    let d = tempfile::tempdir().unwrap();
    let bytes = b"synthetic public test resource";
    let mut t = Transfer::begin(d.path(), descriptor(bytes), bytes.len() as u64, true).unwrap();
    let id = t.id();
    t.append(0, &bytes[..10]).unwrap();
    assert_eq!(t.append(0, &bytes[..10]).unwrap(), 10);
    assert!(matches!(t.append(0, b"mismatched"), Err(Error::Conflict)));
    drop(t);
    let mut t = Transfer::resume(d.path(), id).unwrap();
    assert_eq!(t.committed_bytes(), 10);
    assert!(t.finish().is_err());
    t.append(10, &bytes[10..]).unwrap();
    let path = t.finish().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert!(matches!(
        t.append(bytes.len() as u64, b"x"),
        Err(Error::Conflict)
    ));
}
#[test]
fn incomplete_hash_mismatch_and_no_consent_do_not_activate_resources() {
    let d = tempfile::tempdir().unwrap();
    assert!(matches!(
        Transfer::begin(d.path(), descriptor(b"correct"), 7, false),
        Err(Error::Denied)
    ));
    let mut t = Transfer::begin(d.path(), descriptor(b"correct"), 7, true).unwrap();
    t.append(0, b"invalid").unwrap();
    assert!(matches!(t.finish(), Err(Error::Integrity)));
    assert!(!d.path().join(format!("{}.resource", t.id())).exists());
    let id = t.id();
    t.cancel().unwrap();
    // Coordination inodes remain stable across retries and are never unlinked.
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
    assert_eq!(
        std::fs::read(d.path().join(format!("{id}.owner.lock"))).unwrap(),
        b""
    );
}
#[test]
fn active_resource_lease_prevents_deletion_until_worker_releases_it() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("fixture.resource");
    std::fs::write(&path, b"model fixture").unwrap();
    let pool = ResourceLeases::default();
    let lease = pool
        .acquire(&path, &resource_digest(b"model fixture"))
        .unwrap();
    assert!(matches!(pool.remove(&path), Err(Error::Conflict)));
    drop(lease);
    pool.remove(&path).unwrap();
    assert!(!path.exists());
}
#[test]
fn resume_discards_only_unacknowledged_tail_after_simulated_interruption() {
    use std::io::Write;
    let d = tempfile::tempdir().unwrap();
    let mut t = Transfer::begin(d.path(), descriptor(b"abcdef"), 6, true).unwrap();
    let id = t.id();
    t.append(0, b"abc").unwrap();
    drop(t);
    let path = d.path().join(format!("{id}.part"));
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"def")
        .unwrap();
    let mut t = Transfer::resume(d.path(), id).unwrap();
    assert_eq!(std::fs::metadata(path).unwrap().len(), 3);
    t.append(3, b"def").unwrap();
    t.finish().unwrap();
}

#[test]
fn worker_file_clone_keeps_resource_reserved_after_owner_is_dropped() {
    use std::io::{Read, Seek, SeekFrom};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("worker.resource");
    let bytes = b"synthetic worker resource";
    std::fs::write(&path, bytes).unwrap();
    let pool = ResourceLeases::default();
    let owner = pool.acquire(&path, &resource_digest(bytes)).unwrap();
    let mut worker = owner.file().unwrap();
    drop(owner);
    assert!(matches!(pool.remove(&path), Err(Error::Conflict)));
    let mut observed = Vec::new();
    worker.read_to_end(&mut observed).unwrap();
    assert_eq!(observed, bytes);
    worker.seek(SeekFrom::Start(0)).unwrap();
    drop(worker);
    pool.remove(&path).unwrap();
    assert!(!path.exists());
}

#[test]
fn cloned_worker_reservations_survive_thread_transfer_until_the_last_handle_retires() {
    use std::{io::Read, sync::mpsc, time::Duration};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("thread.resource");
    let bytes = b"synthetic threaded resource";
    std::fs::write(&path, bytes).unwrap();
    let pool = ResourceLeases::default();
    let owner = pool.acquire(&path, &resource_digest(bytes)).unwrap();
    let worker = owner.file().unwrap();
    let mut last = worker.file().unwrap();
    drop(owner);
    drop(worker);
    let (ready, waiting) = mpsc::channel();
    let (release, retire) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let mut observed = Vec::new();
        last.read_to_end(&mut observed).unwrap();
        assert_eq!(observed, bytes);
        ready.send(()).unwrap();
        retire.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(last);
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(pool.clone().remove(&path), Err(Error::Conflict)));
    release.send(()).unwrap();
    thread.join().unwrap();
    pool.remove(&path).unwrap();
    assert!(!path.exists());
}

#[test]
fn legacy_bounded_manifest_above_new_begin_budget_remains_resumable() {
    let root = tempfile::tempdir().unwrap();
    let bytes = b"legacy bounded manifest fixture";
    let transfer =
        Transfer::begin(root.path(), descriptor(bytes), bytes.len() as u64, true).unwrap();
    let id = transfer.id();
    drop(transfer);
    let path = root.path().join(format!("{id}.install.json"));
    let mut state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    state["descriptor"]["personal_use_evidence"] = serde_json::Value::String("x".repeat(62000));
    let encoded = serde_json::to_vec(&state).unwrap();
    assert!(encoded.len() < 65536);
    std::fs::write(path, encoded).unwrap();
    let mut resumed = Transfer::resume(root.path(), id).unwrap();
    resumed.append(0, bytes).unwrap();
    assert_eq!(std::fs::read(resumed.finish().unwrap()).unwrap(), bytes);
}

#[cfg(unix)]
#[test]
fn completed_transfer_rejects_a_symlink_target_without_adopting_external_bytes() {
    let root = tempfile::tempdir().unwrap();
    let models = root.path().join("models");
    let bytes = b"same hash still requires an owned regular target";
    let mut transfer =
        Transfer::begin(&models, descriptor(bytes), bytes.len() as u64, true).unwrap();
    let id = transfer.id();
    transfer.append(0, bytes).unwrap();
    let outside = root.path().join("external.resource");
    std::fs::write(&outside, bytes).unwrap();
    std::os::unix::fs::symlink(&outside, models.join(format!("{id}.resource"))).unwrap();
    assert!(matches!(transfer.finish(), Err(Error::Integrity)));
    assert_eq!(std::fs::read(outside).unwrap(), bytes);
}

#[test]
fn competing_resume_cannot_truncate_until_the_original_owner_releases() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    let mut owner = Transfer::begin(root.path(), descriptor(b"abcdef"), 6, true).unwrap();
    let id = owner.id();
    assert!(matches!(
        Transfer::resume(root.path(), id),
        Err(Error::ResourceLimit)
    ));
    owner.append(0, b"abc").unwrap(); // Atomic ledger replacement keeps the lock.
    let part = root.path().join(format!("{id}.part"));
    std::fs::OpenOptions::new()
        .append(true)
        .open(&part)
        .unwrap()
        .write_all(b"tail")
        .unwrap();
    assert!(matches!(
        Transfer::resume(root.path(), id),
        Err(Error::ResourceLimit)
    ));
    assert_eq!(std::fs::read(&part).unwrap(), b"abctail");
    drop(owner);
    let resumed = Transfer::resume(root.path(), id).unwrap();
    assert_eq!(resumed.committed_bytes(), 3);
    assert_eq!(std::fs::read(part).unwrap(), b"abc");
}
#[test]
fn transfer_ownership_excludes_another_process_and_releases_on_process_exit() {
    if let Some(root) = std::env::var_os("STEM_TRANSFER_LOCK_CHILD_ROOT") {
        let id =
            stem_core::Id::parse(&std::env::var("STEM_TRANSFER_LOCK_CHILD_ID").unwrap()).unwrap();
        let transfer = Transfer::resume(std::path::Path::new(&root), id);
        if std::env::var_os("STEM_TRANSFER_LOCK_CHILD_AVAILABLE").is_some() {
            assert_eq!(transfer.unwrap().committed_bytes(), 3);
            // Reacquire and exit without Rust destructors, exercising OS release.
            let transfer = Transfer::resume(std::path::Path::new(&root), id).unwrap();
            std::mem::forget(transfer);
            std::process::exit(0);
        } else {
            assert!(matches!(transfer, Err(Error::ResourceLimit)));
        }
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let mut owner = Transfer::begin(root.path(), descriptor(b"abcdef"), 6, true).unwrap();
    owner.append(0, b"abc").unwrap();
    let id = owner.id();
    let child = |available: bool| {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "transfer_ownership_excludes_another_process_and_releases_on_process_exit",
            ])
            .env("STEM_TRANSFER_LOCK_CHILD_ROOT", root.path())
            .env("STEM_TRANSFER_LOCK_CHILD_ID", id.to_string());
        if available {
            command.env("STEM_TRANSFER_LOCK_CHILD_AVAILABLE", "1");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    child(false);
    drop(owner);
    child(true);
    assert_eq!(
        Transfer::resume(root.path(), id).unwrap().committed_bytes(),
        3
    );
}
#[cfg(unix)]
#[test]
fn ownership_record_symlinks_and_nonempty_files_are_not_followed_or_truncated() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let owner = Transfer::begin(root.path(), descriptor(b"abc"), 3, true).unwrap();
    let id = owner.id();
    drop(owner);
    let lock = root.path().join(format!("{id}.owner.lock"));
    std::fs::remove_file(&lock).unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(outside.path(), b"external bytes").unwrap();
    symlink(outside.path(), &lock).unwrap();
    assert!(matches!(
        Transfer::resume(root.path(), id),
        Err(Error::Integrity)
    ));
    assert_eq!(std::fs::read(outside.path()).unwrap(), b"external bytes");
    std::fs::remove_file(&lock).unwrap();
    std::fs::write(&lock, b"unknown owner record").unwrap();
    assert!(matches!(
        Transfer::resume(root.path(), id),
        Err(Error::Integrity)
    ));
    assert_eq!(std::fs::read(lock).unwrap(), b"unknown owner record");
}
