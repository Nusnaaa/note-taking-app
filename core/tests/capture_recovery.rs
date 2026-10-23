mod common;
use common::*;
use stem_core::{
    security::{Access, Scope},
    store::{CaptureTargetState, Kind, Store, WritingTarget},
    Error, Id,
};

#[test]
fn retired_capture_evidence_requires_target_authority_and_cannot_reopen_documents() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "original", &a).unwrap();
    let proof = access([Scope::Entity(n.id)]);
    assert_eq!(
        s.capture_recovery_access(n.id, &a).unwrap(),
        CaptureTargetState::Active
    );
    let review = s.review_deletion(n.id, &a).unwrap();
    s.trash(review, true, &a).unwrap();
    assert!(matches!(
        s.capture_recovery_access(n.id, &a),
        Err(Error::Denied)
    ));
    assert_eq!(
        s.capture_recovery_access(n.id, &proof).unwrap(),
        CaptureTargetState::Trashed
    );
    assert!(matches!(s.entity(n.id, &proof), Err(Error::Unavailable)));
    assert!(s
        .append_capture(
            Id::new(),
            WritingTarget::Entity(n.id),
            vec![stroke(0)],
            &proof
        )
        .is_err());
    s.restore(n.id, &proof).unwrap();
    assert_eq!(
        s.capture_recovery_access(n.id, &a).unwrap(),
        CaptureTargetState::Active
    );
    let review = s.review_deletion(n.id, &a).unwrap();
    s.trash(review, true, &a).unwrap();
    s.purge(n.id, &proof).unwrap();
    assert!(matches!(
        s.capture_recovery_access(n.id, &a),
        Err(Error::Denied)
    ));
    assert_eq!(
        s.capture_recovery_access(n.id, &proof).unwrap(),
        CaptureTargetState::Purged
    );
    assert!(matches!(s.entity(n.id, &proof), Err(Error::Unavailable)));
    assert!(s
        .append_capture(
            Id::new(),
            WritingTarget::Entity(n.id),
            vec![stroke(0)],
            &proof
        )
        .is_err());
    assert!(s.restore(n.id, &proof).is_err());
    assert!(s.capture_recovery_access(Id::new(), &proof).is_err());
}

#[test]
fn recovery_authority_preserves_inherited_and_app_locks_across_purge_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recovery.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let parent = s.create(Kind::Module, None, "private", &a).unwrap();
    let n = s
        .create(Kind::Canvas, Some(parent.id), "draft", &a)
        .unwrap();
    let proof = access([Scope::Entity(parent.id), Scope::Entity(n.id), Scope::App]);
    s.set_lock(parent.id, parent.revision, true, &proof)
        .unwrap();
    assert!(matches!(
        s.capture_recovery_access(n.id, &a),
        Err(Error::Denied)
    ));
    assert!(matches!(
        s.capture_recovery_access(parent.id, &a),
        Err(Error::Denied)
    ));
    let target_only = access([Scope::Entity(n.id)]);
    assert!(matches!(
        s.capture_recovery_access(n.id, &target_only),
        Err(Error::Denied)
    ));
    let review = s.review_deletion(parent.id, &proof).unwrap();
    s.trash(review, true, &proof).unwrap();
    assert_eq!(
        s.capture_recovery_access(n.id, &proof).unwrap(),
        CaptureTargetState::Trashed
    );
    s.purge(parent.id, &proof).unwrap();
    s.set_app_lock(true, &proof).unwrap();
    drop(s);
    let s = Store::open(&path).unwrap();
    let ancestors = access([Scope::Entity(parent.id), Scope::Entity(n.id)]);
    assert!(matches!(
        s.capture_recovery_access(n.id, &ancestors),
        Err(Error::Denied)
    ));
    let parent_only = access([Scope::Entity(parent.id), Scope::App]);
    assert!(matches!(
        s.capture_recovery_access(n.id, &parent_only),
        Err(Error::Denied)
    ));
    assert_eq!(
        s.capture_recovery_access(n.id, &proof).unwrap(),
        CaptureTargetState::Purged
    );
    assert!(matches!(
        s.capture_recovery_access(parent.id, &proof),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn subtree_purge_preflight_and_commit_failure_do_not_partially_erase_protected_ink() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let parent = s.create(Kind::Notebook, None, "parent", &a).unwrap();
    let n = s.create(Kind::Note, Some(parent.id), "child", &a).unwrap();
    let original = stroke(0);
    let b = s.append_ink(n.id, vec![original.clone()], &a).unwrap();
    let proof = access([
        Scope::Entity(parent.id),
        Scope::Entity(n.id),
        Scope::Block(b.id),
    ]);
    s.set_block_lock(b.id, b.revision, true, &proof).unwrap();
    let review = s.review_deletion(parent.id, &proof).unwrap();
    s.trash(review, true, &proof).unwrap();
    assert!(matches!(s.purge(parent.id, &a), Err(Error::Denied)));
    assert_eq!(
        s.capture_recovery_access(n.id, &proof).unwrap(),
        CaptureTargetState::Trashed
    );
    s.restore(parent.id, &proof).unwrap();
    assert_eq!(s.source_ink(b.id, &proof).unwrap(), vec![original.clone()]);
    let review = s.review_deletion(parent.id, &proof).unwrap();
    s.trash(review, true, &proof).unwrap();
    s.inject_failure(stem_core::store::FailurePoint::BeforeCommit);
    assert!(s.purge(parent.id, &proof).is_err());
    assert_eq!(
        s.capture_recovery_access(n.id, &proof).unwrap(),
        CaptureTargetState::Trashed
    );
    s.restore(parent.id, &proof).unwrap();
    assert_eq!(s.source_ink(b.id, &proof).unwrap(), vec![original]);
    let review = s.review_deletion(parent.id, &proof).unwrap();
    s.trash(review, true, &proof).unwrap();
    s.purge(parent.id, &proof).unwrap();
    assert_eq!(
        s.capture_recovery_access(n.id, &proof).unwrap(),
        CaptureTargetState::Purged
    );
    assert!(s.source_ink(b.id, &proof).is_err());
    s.check_integrity().unwrap();
}
