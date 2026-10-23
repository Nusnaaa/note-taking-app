mod common;
use common::*;
use stem_core::{
    security::{Access, Scope},
    store::{CaptureTargetState, FailurePoint, Kind, Store},
    Error,
};

#[test]
fn permanent_review_is_kind_generation_confirmation_and_transaction_bound() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "source", &a).unwrap();
    let source = stroke(0);
    let b = s.append_ink(n.id, vec![source.clone()], &a).unwrap();
    assert!(s.review_purge(n.id, &a).is_err());
    let review = s.review_deletion(n.id, &a).unwrap();
    assert!(matches!(
        s.purge_reviewed(review, true, &a),
        Err(Error::Invalid(_))
    ));
    let review = s.review_deletion(n.id, &a).unwrap();
    s.trash(review, true, &a).unwrap();
    let review = s.review_purge(n.id, &a).unwrap();
    assert!(matches!(s.trash(review, true, &a), Err(Error::Invalid(_))));
    let review = s.review_purge(n.id, &a).unwrap();
    assert!(matches!(
        s.purge_reviewed(review, false, &a),
        Err(Error::ReviewRequired)
    ));
    let review = s.review_purge(n.id, &a).unwrap();
    s.create(Kind::Note, None, "new state", &a).unwrap();
    assert!(matches!(
        s.purge_reviewed(review, true, &a),
        Err(Error::Conflict)
    ));
    let review = s.review_purge(n.id, &a).unwrap();
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.purge_reviewed(review, true, &a).is_err());
    s.restore(n.id, &a).unwrap();
    assert_eq!(s.source_ink(b.id, &a).unwrap(), vec![source]);
    let review = s.review_deletion(n.id, &a).unwrap();
    s.trash(review, true, &a).unwrap();
    let review = s.review_purge(n.id, &a).unwrap();
    s.purge_reviewed(review, true, &a).unwrap();
    assert!(s.trash_entries(0, 512, &a).unwrap().is_empty());
    assert!(s.source_ink(b.id, &a).is_err());
}

#[test]
fn trash_pages_count_only_authorised_entries_and_restore_requires_current_revision() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "public", &a).unwrap();
    let private = s.create(Kind::Note, None, "private title", &a).unwrap();
    let proof = access([Scope::Entity(private.id), Scope::App]);
    s.set_lock(private.id, private.revision, true, &proof)
        .unwrap();
    for id in [n.id, private.id] {
        let review = s.review_deletion(id, &proof).unwrap();
        s.trash(review, true, &proof).unwrap();
    }
    let public = s.trash_entries(0, 512, &a).unwrap();
    assert_eq!(public.len(), 1);
    assert_eq!(public[0].id, n.id);
    assert!(s.trash_entries(1, 1, &a).unwrap().is_empty());
    assert_eq!(s.trash_entries(0, 512, &proof).unwrap().len(), 2);
    assert!(s.trash_entries(0, 0, &a).unwrap().is_empty());
    assert!(matches!(
        s.trash_entries(0, 513, &a),
        Err(Error::ResourceLimit)
    ));
    assert!(matches!(
        s.restore_revision(n.id, public[0].revision - 1, &a),
        Err(Error::Conflict)
    ));
    s.restore_revision(n.id, public[0].revision, &a).unwrap();
    assert!(s.restore_revision(n.id, public[0].revision, &a).is_err());
    assert!(s.trash_entries(0, 512, &a).unwrap().is_empty());
    s.set_app_lock(true, &proof).unwrap();
    assert!(matches!(s.trash_entries(0, 512, &a), Err(Error::Denied)));
}

#[test]
fn container_retirement_handles_independently_trashed_and_already_purged_descendants() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let root = s.create(Kind::Notebook, None, "folder", &a).unwrap();
    let trashed = s
        .create(Kind::Note, Some(root.id), "trashed child", &a)
        .unwrap();
    let purged = s
        .create(Kind::Note, Some(root.id), "purged child", &a)
        .unwrap();
    let live = s
        .create(Kind::Note, Some(root.id), "live child", &a)
        .unwrap();
    for id in [trashed.id, purged.id] {
        let review = s.review_deletion(id, &a).unwrap();
        s.trash(review, true, &a).unwrap();
    }
    s.purge(purged.id, &a).unwrap();
    let review = s.review_deletion(root.id, &a).unwrap();
    s.trash(review, true, &a).unwrap();
    s.restore(root.id, &a).unwrap();
    assert!(s.entity(live.id, &a).is_ok());
    assert!(s.entity(trashed.id, &a).is_err());
    assert!(s.entity(purged.id, &a).is_err());
    let review = s.review_deletion(root.id, &a).unwrap();
    s.trash(review, true, &a).unwrap();
    let review = s.review_purge(root.id, &a).unwrap();
    s.purge_reviewed(review, true, &a).unwrap();
    let proof = access([Scope::Entity(live.id)]);
    assert_eq!(
        s.capture_recovery_access(live.id, &proof).unwrap(),
        CaptureTargetState::Purged
    );
    s.check_integrity().unwrap();
}

#[test]
fn capture_target_manifest_omits_purged_descendants_and_revalidation_checks_fresh_proof() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let root = s.create(Kind::Notebook, None, "root", &a).unwrap();
    let child = s.create(Kind::Note, Some(root.id), "child", &a).unwrap();
    let old = s.create(Kind::Note, Some(root.id), "purged", &a).unwrap();
    let review = s.review_deletion(old.id, &a).unwrap();
    s.trash(review, true, &a).unwrap();
    s.purge(old.id, &a).unwrap();
    let proof = access([Scope::Entity(child.id)]);
    s.set_lock(child.id, child.revision, true, &proof).unwrap();
    assert!(matches!(s.review_deletion(root.id, &a), Err(Error::Denied)));
    let review = s.review_deletion(root.id, &proof).unwrap();
    assert_eq!(review.capture_targets(), &[child.id]);
    assert!(matches!(
        s.validate_deletion_review(&review, &a),
        Err(Error::Denied)
    ));
    s.validate_deletion_review(&review, &proof).unwrap();
    s.trash(review, true, &proof).unwrap();
}
