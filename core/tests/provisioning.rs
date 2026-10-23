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
    t.cancel().unwrap();
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0);
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
