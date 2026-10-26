mod common;
use std::{io::Write, time::Duration};
use stem_core::{
    host::Host,
    processing::*,
    provisioning::{resource_digest, RecoveryState, Transfer},
    security::Access,
    store::*,
    Error, Id,
};
fn descriptor(bytes: &[u8]) -> Descriptor {
    Descriptor {
        id: "synthetic-import".into(),
        version: "1".into(),
        adapter_version: "1".into(),
        family: Family::Recogniser,
        input: InputContract::OrderedStrokesV1,
        tasks: vec![Task::Prose],
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some("app-owned synthetic import fixture; no real model".into()),
        redistributable_resources: false,
        resource_sha256: Some(resource_digest(bytes)),
        runtime_contract: "stem-processing-v1".into(),
    }
}
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(30)).unwrap()
}
fn begin(s: &Store, root: &std::path::Path) -> EngineImportTransfer {
    s.engine_import_plan(descriptor(b"model"), 5, true, &Access::public())
        .unwrap()
        .begin(root, &control())
        .unwrap()
}
#[test]
fn discovery_never_truncates_or_publishes_and_redacts_unbound_and_corrupt_records() {
    let root = tempfile::tempdir().unwrap();
    let s = Store::memory().unwrap();
    let mut transfer = begin(&s, root.path());
    transfer.append(0, b"mo", &control()).unwrap();
    let id = transfer.id();
    let part = root.path().join(format!("{id}.part"));
    std::fs::OpenOptions::new()
        .append(true)
        .open(&part)
        .unwrap()
        .write_all(b"unacknowledged tail")
        .unwrap();
    let before = std::fs::read(&part).unwrap();
    let legacy = Transfer::begin(root.path(), descriptor(b"model"), 5, true).unwrap();
    let corrupt = Id::new();
    std::fs::write(
        root.path().join(format!("{corrupt}.install.json")),
        b"private malformed provider metadata",
    )
    .unwrap();
    let foreign_store = Store::memory().unwrap();
    let foreign = begin(&foreign_store, root.path());
    let page = s
        .engine_recovery_plan(&Access::public())
        .unwrap()
        .scan(root.path(), None, 128, &control())
        .unwrap();
    assert_eq!(page.entries.len(), 3);
    assert!(page.entries.iter().all(|row| row.id != foreign.id()));
    let row = page.entries.iter().find(|row| row.id == id).unwrap();
    assert_eq!(row.state, RecoveryState::Interrupted);
    assert_eq!(row.committed, Some(2));
    assert_eq!(row.total, Some(5));
    assert!(row.consent_ms.is_some());
    let row = page
        .entries
        .iter()
        .find(|row| row.id == legacy.id())
        .unwrap();
    assert_eq!(row.state, RecoveryState::ReviewRequired);
    assert!(row.descriptor.is_none());
    let row = page.entries.iter().find(|row| row.id == corrupt).unwrap();
    assert_eq!(row.state, RecoveryState::Invalid);
    assert!(row.descriptor.is_none());
    assert_eq!(std::fs::read(part).unwrap(), before);
    let json = serde_json::to_string(&page).unwrap();
    assert!(!json.contains("private malformed"));
    assert!(!json.contains(&root.path().display().to_string()));
    assert!(matches!(
        s.engine_version(id, &Access::public()),
        Err(Error::Unavailable)
    ));
}
#[test]
fn discovery_pages_eligible_records_without_foreign_library_cursors() {
    let root = tempfile::tempdir().unwrap();
    let s = Store::memory().unwrap();
    let foreign_store = Store::memory().unwrap();
    let mut ids = Vec::new();
    for _ in 0..5 {
        ids.push(begin(&s, root.path()).id());
        begin(&foreign_store, root.path());
    }
    ids.sort();
    let mut after = None;
    let mut found = Vec::new();
    loop {
        let page = s
            .engine_recovery_plan(&Access::public())
            .unwrap()
            .scan(root.path(), after, 2, &control())
            .unwrap();
        assert!(page.entries.len() <= 2);
        found.extend(page.entries.iter().map(|row| row.id));
        if let Some(next) = page.next {
            assert_eq!(Some(next), found.last().copied());
            after = Some(next);
        } else {
            break;
        }
    }
    assert_eq!(found, ids);
}
#[test]
fn completed_ledger_is_not_a_claim_of_current_hash_or_runtime_verification() {
    let root = tempfile::tempdir().unwrap();
    let s = Store::memory().unwrap();
    let mut transfer = begin(&s, root.path());
    transfer.append(0, b"model", &control()).unwrap();
    let id = transfer.id();
    let _prepared = transfer.finish(&control()).unwrap();
    std::fs::write(root.path().join(format!("{id}.resource")), b"other").unwrap();
    let page = s
        .engine_recovery_plan(&Access::public())
        .unwrap()
        .scan(root.path(), None, 1, &control())
        .unwrap();
    assert_eq!(page.entries[0].state, RecoveryState::RecordedComplete);
    let transfer = s
        .engine_import_plan(descriptor(b"model"), 5, true, &Access::public())
        .unwrap()
        .resume(root.path(), id, &control())
        .unwrap();
    assert!(matches!(transfer.finish(&control()), Err(Error::Integrity)));
}
#[test]
fn missing_depot_is_not_created_and_invalid_bounds_or_cancel_fail() {
    let root = tempfile::tempdir().unwrap();
    let depot = root.path().join("not-created");
    let s = Store::memory().unwrap();
    let a = Access::public();
    assert!(s
        .engine_recovery_plan(&a)
        .unwrap()
        .scan(&depot, None, 1, &control())
        .unwrap()
        .entries
        .is_empty());
    assert!(!depot.exists());
    assert!(matches!(
        s.engine_recovery_plan(&a)
            .unwrap()
            .scan(&depot, None, 0, &control()),
        Err(Error::ResourceLimit)
    ));
    let cancelled = control();
    cancelled.cancel();
    assert!(matches!(
        s.engine_recovery_plan(&a)
            .unwrap()
            .scan(&depot, None, 1, &cancelled),
        Err(Error::Cancelled)
    ));
    assert!(!depot.exists());
}
#[cfg(unix)]
#[test]
fn symlink_depot_and_ledger_are_never_followed_and_oversized_ledger_is_redacted() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let s = Store::memory().unwrap();
    let a = Access::public();
    let outside = tempfile::tempdir().unwrap();
    let linked = root.path().join("linked");
    symlink(outside.path(), &linked).unwrap();
    assert!(matches!(
        s.engine_recovery_plan(&a)
            .unwrap()
            .scan(&linked, None, 1, &control()),
        Err(Error::Integrity)
    ));
    let id = Id::new();
    let outside_file = outside.path().join("private");
    std::fs::write(&outside_file, b"private").unwrap();
    symlink(
        &outside_file,
        root.path().join(format!("{id}.install.json")),
    )
    .unwrap();
    let large = Id::new();
    std::fs::write(
        root.path().join(format!("{large}.install.json")),
        vec![b'x'; 65537],
    )
    .unwrap();
    let page = s
        .engine_recovery_plan(&a)
        .unwrap()
        .scan(root.path(), None, 128, &control())
        .unwrap();
    assert_eq!(page.entries.len(), 2);
    assert!(page
        .entries
        .iter()
        .all(|row| row.state == RecoveryState::Invalid && row.descriptor.is_none()));
    assert_eq!(std::fs::read(outside_file).unwrap(), b"private");
}
#[test]
fn host_recovery_reservation_is_cancelled_by_capture_and_relock() {
    let root = tempfile::tempdir().unwrap();
    let h = Host::open(root.path().join("library.db"), 8).unwrap();
    let depot = root.path().join("not-created");
    let plan = h.engine_recovery_plan().unwrap();
    let pen = Id::new();
    h.set_capture_priority(pen, true).unwrap();
    assert!(matches!(
        h.engine_recovery_plan(),
        Err(Error::ResourceLimit)
    ));
    h.set_capture_priority(pen, false).unwrap();
    assert!(matches!(
        plan.scan(&depot, None, 1, &control()),
        Err(Error::Cancelled)
    ));
    let plan = h.engine_recovery_plan().unwrap();
    let reply = h
        .request("{\"version\":1,\"command\":{\"op\":\"Relock\"}}")
        .unwrap();
    assert!(reply.ok);
    assert!(plan.scan(&depot, None, 1, &control()).is_err());
    assert!(!depot.exists());
    h.close().unwrap();
}

#[test]
fn recovery_requires_current_app_access_and_bounded_host_reservations() {
    use stem_core::security::Scope;
    let mut s = Store::memory().unwrap();
    let access = Access::authenticate(
        &mut common::TestAuth(true),
        [Scope::App].into_iter().collect(),
    )
    .unwrap();
    s.set_app_lock(true, &access).unwrap();
    assert!(matches!(
        s.engine_recovery_plan(&Access::public()),
        Err(Error::Denied)
    ));
    let root = tempfile::tempdir().unwrap();
    let h = Host::open(root.path().join("library.db"), 2).unwrap();
    let first = h.engine_recovery_plan().unwrap();
    let second = h.engine_recovery_plan().unwrap();
    assert!(matches!(
        h.engine_recovery_plan(),
        Err(Error::ResourceLimit)
    ));
    drop(first);
    drop(second);
    // Drop queues cleanup; admission is released by the retirement worker,
    // not by the caller's destructor. Observe the real capacity acknowledgement.
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while h.recognition_status().reserved != 0 {
        assert!(
            std::time::Instant::now() < until,
            "recovery owners did not retire"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(h.engine_recovery_plan().unwrap());
    h.authenticate(
        [Scope::App].into_iter().collect(),
        Box::new(common::TestAuth(true)),
    )
    .unwrap();
    assert!(
        h.request("{\"version\":1,\"command\":{\"op\":\"SetAppLock\",\"enabled\":true}}")
            .unwrap()
            .ok
    );
    assert!(
        h.request("{\"version\":1,\"command\":{\"op\":\"Relock\"}}")
            .unwrap()
            .ok
    );
    assert!(matches!(h.engine_recovery_plan(), Err(Error::Denied)));
    h.close().unwrap();
}
#[test]
fn descriptor_bytes_limit_pages_without_losing_the_continuation() {
    let root = tempfile::tempdir().unwrap();
    let s = Store::memory().unwrap();
    let first = begin(&s, root.path());
    let id = first.id();
    let mut ledger: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.path().join(format!("{id}.install.json"))).unwrap(),
    )
    .unwrap();
    std::fs::remove_file(root.path().join(format!("{id}.install.json"))).unwrap();
    ledger["descriptor"]["personal_use_evidence"] = serde_json::Value::String("e".repeat(60000));
    let mut ids = Vec::new();
    for _ in 0..72 {
        let id = Id::new();
        ids.push(id);
        ledger["id"] = serde_json::Value::String(id.to_string());
        std::fs::write(
            root.path().join(format!("{id}.install.json")),
            serde_json::to_vec(&ledger).unwrap(),
        )
        .unwrap();
        std::fs::write(root.path().join(format!("{id}.part")), []).unwrap();
    }
    ids.sort();
    let first = s
        .engine_recovery_plan(&Access::public())
        .unwrap()
        .scan(root.path(), None, 128, &control())
        .unwrap();
    assert!(first.next.is_some());
    assert!(first.entries.len() < 72);
    let charge: usize = first
        .entries
        .iter()
        .map(|row| serde_json::to_vec(row).unwrap().len())
        .sum();
    assert!(charge <= 4 * 1024 * 1024);
    let second = s
        .engine_recovery_plan(&Access::public())
        .unwrap()
        .scan(root.path(), first.next, 128, &control())
        .unwrap();
    assert!(second.next.is_none());
    assert_eq!(
        first
            .entries
            .iter()
            .chain(second.entries.iter())
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        ids
    );
}
