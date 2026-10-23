mod common;
use common::*;
use std::time::Duration;
use stem_core::{
    capture::*,
    ink::Tool,
    security::Access,
    store::{FailurePoint, Kind, SaveState, Store, WritingTarget},
    Error, Id,
};
#[test]
fn palm_navigation_and_stylus_cancellation_do_not_commit_accidental_ink() {
    let p = stroke(0).points.remove(0);
    let mut r = InputRouter::new("document", true, 20).unwrap();
    assert_eq!(
        r.begin(1, PointerKind::Palm, p.clone()).unwrap(),
        Routing::Ignored
    );
    assert_eq!(
        r.begin(2, PointerKind::Touch, p.clone()).unwrap(),
        Routing::Drawing
    );
    r.begin(3, PointerKind::Pen, p.clone()).unwrap();
    assert!(r.finish(2).is_err());
    assert_eq!(
        r.begin(4, PointerKind::Touch, p.clone()).unwrap(),
        Routing::Ignored
    );
    assert!(r.cancel(3));
    assert_eq!(r.active_count(), 0);
    let mut r = InputRouter::new("document", false, 20).unwrap();
    assert_eq!(
        r.begin(2, PointerKind::Touch, p).unwrap(),
        Routing::Navigation
    );
    assert_eq!(r.active_count(), 0);
}
#[test]
fn observed_samples_are_exact_and_invalid_events_leave_capture_recoverable() {
    let s = stroke(0);
    let mut r = InputRouter::new("document", false, 2).unwrap();
    r.begin(1, PointerKind::Pen, s.points[0].clone()).unwrap();
    let mut bad = s.points[1].clone();
    bad.x = f64::NAN;
    assert!(r.sample(1, bad).is_err());
    r.sample(1, s.points[1].clone()).unwrap();
    assert!(r.sample(1, s.points[1].clone()).is_err());
    let captured = r.finish(1).unwrap();
    assert_eq!(captured.points, s.points);
    assert_eq!(captured.tool, Tool::Pen);
}
#[test]
fn full_queue_retains_old_work_and_failed_persistence_can_retry() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    let c = PendingCapture {
        ticket: Id::new(),
        target: WritingTarget::Entity(n.id),
        strokes: vec![stroke(0)],
    };
    let mut q = PersistenceQueue::new(100000, 1).unwrap();
    q.enqueue(&c).unwrap();
    let other = PendingCapture {
        ticket: Id::new(),
        ..c.clone()
    };
    assert!(q.enqueue(&other).is_err());
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(q.persist_next(&mut s, &a).is_err());
    assert_eq!(q.pending_count(), 1);
    assert_eq!(
        s.document_save_state(n.id, &a).unwrap(),
        SaveState::NeedsAttention
    );
    assert!(s.blocks(n.id, 0, 20, &a).unwrap().is_empty());
    let b = q.persist_next(&mut s, &a).unwrap().unwrap();
    assert_eq!(q.pending_count(), 0);
    assert_eq!(s.source_ink(b.id, &a).unwrap(), c.strokes);
}
#[test]
fn capture_receipt_survives_restart_and_rejects_changed_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("capture.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    let ticket = Id::new();
    let ink = vec![stroke(0)];
    let b = s
        .append_capture(ticket, WritingTarget::Entity(n.id), ink.clone(), &a)
        .unwrap();
    drop(s);
    let mut s = Store::open(&path).unwrap();
    let retry = s
        .append_capture(ticket, WritingTarget::Entity(n.id), ink, &a)
        .unwrap();
    assert_eq!(retry.id, b.id);
    assert_eq!(s.blocks(n.id, 0, 20, &a).unwrap().len(), 1);
    assert!(matches!(
        s.append_capture(ticket, WritingTarget::Entity(n.id), vec![stroke(1)], &a),
        Err(Error::Conflict)
    ));
}
#[test]
fn capture_cancels_background_work_without_faking_resource_release() {
    let scheduler = Scheduler::new(100).unwrap();
    let work = scheduler
        .admit(WorkClass::Recognition, 80, Duration::from_secs(30))
        .unwrap();
    let capture = scheduler.begin_capture().unwrap();
    assert!(matches!(work.control.check(), Err(Error::Cancelled)));
    assert!(scheduler
        .admit(WorkClass::Thumbnail, 1, Duration::from_secs(1))
        .is_err());
    drop(capture);
    assert_eq!(scheduler.reserved_bytes().unwrap(), 80);
    assert!(scheduler
        .admit(WorkClass::Export, 30, Duration::from_secs(1))
        .is_err());
    drop(work);
    assert_eq!(scheduler.reserved_bytes().unwrap(), 0);
    assert!(scheduler
        .admit(WorkClass::PdfRender, 100, Duration::from_secs(1))
        .is_ok());
}
#[test]
fn scheduler_leases_work_across_threads_and_unwind() {
    let scheduler = Scheduler::new(100).unwrap();
    let other = scheduler.clone();
    let result = std::thread::spawn(move || {
        let _lease = other
            .admit(WorkClass::Index, 90, Duration::from_secs(30))
            .unwrap();
        panic!("synthetic worker failure")
    })
    .join();
    assert!(result.is_err());
    assert_eq!(scheduler.reserved_bytes().unwrap(), 0);
}
#[test]
fn emergency_reserve_is_written_released_and_never_overwrites_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reserve");
    let reserve = EmergencyReserve::create(&path, 131072).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), vec![0; 131072]);
    assert!(EmergencyReserve::create(&path, 10).is_err());
    assert!(EmergencyReserve::low_space(100, 90, 11));
    assert_eq!(reserve.release().unwrap(), 131072);
    assert!(!path.exists());
}

#[test]
fn capture_receipt_probe_checks_current_scope_target_and_original_stroke() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "receipt", &a).unwrap();
    let ticket = Id::new();
    assert!(!s.capture_committed(ticket, n.id, &a).unwrap());
    let block = s
        .append_capture(ticket, WritingTarget::Entity(n.id), vec![stroke(0)], &a)
        .unwrap();
    assert!(s.capture_committed(ticket, n.id, &a).unwrap());
    let other = s.create(Kind::Note, None, "other", &a).unwrap();
    assert!(matches!(
        s.capture_committed(ticket, other.id, &a),
        Err(Error::Conflict)
    ));
    s.set_lock(
        n.id,
        s.entity(n.id, &a).unwrap().revision,
        true,
        &access([stem_core::security::Scope::Entity(n.id)]),
    )
    .unwrap();
    assert!(matches!(
        s.capture_committed(ticket, n.id, &a),
        Err(Error::Denied)
    ));
    assert!(s.source_ink(block.id, &a).is_err());
}
