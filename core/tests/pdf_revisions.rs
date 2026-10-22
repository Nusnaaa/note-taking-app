mod common;
use common::*;
use sha2::{Digest, Sha256};
use stem_core::{
    objects::Rect,
    security::{Access, Scope},
    sources::*,
    store::{FailurePoint, Kind, Store},
    Error,
};
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn bounds() -> Rect {
    Rect {
        x: 0.1,
        y: 0.2,
        width: 0.3,
        height: 0.4,
    }
}
#[test]
fn source_revision_review_preserves_work_and_requires_anchor_reconnection() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let old = b"%PDF-old";
    let source = s.link_pdf(n.id, "content://old", &hash(old), &a).unwrap();
    let live = s
        .anchor_pdf(source.id, b.id, 4, bounds(), false, &a)
        .unwrap();
    let pinned = s
        .anchor_pdf(source.id, b.id, 8, bounds(), true, &a)
        .unwrap();
    let cache = s
        .retain_source_bytes(
            source.id,
            old,
            RetentionChoice {
                class: AssetClass::ManagedCopy,
                authority: None,
                export_allowed: false,
                consented: true,
            },
            &a,
        )
        .unwrap();
    let review = s
        .review_pdf_revision(source.id, "content://new", &hash(b"%PDF-new"), &a)
        .unwrap();
    assert_eq!(review.affected_anchors, 2);
    s.replace_pdf_revision(review, true, &a).unwrap();
    assert_eq!(
        s.pdf_anchor(live.id, &a).unwrap().health,
        AnchorHealth::NeedsReview
    );
    assert_eq!(s.pdf_anchor(live.id, &a).unwrap().page, 4);
    assert_eq!(
        s.pdf_anchor(pinned.id, &a).unwrap().health,
        AnchorHealth::Historical
    );
    assert!(s.source_bytes(cache, false, &a).is_err());
    assert_eq!(s.source_ink(b.id, &a).unwrap().len(), 1);
    let resolved = s
        .reconnect_pdf_anchor(live.id, 7, bounds(), true, &a)
        .unwrap();
    assert_eq!(resolved.health, AnchorHealth::Current);
    assert_eq!(resolved.source_revision, hash(b"%PDF-new"));
}
#[test]
fn revision_change_is_atomic_stale_safe_and_cannot_change_locked_annotations() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let old = hash(b"old");
    let source = s.link_pdf(n.id, "file://old", &old, &a).unwrap();
    let anchor = s
        .anchor_pdf(source.id, b.id, 1, bounds(), false, &a)
        .unwrap();
    let review = s
        .review_pdf_revision(source.id, "file://new", &hash(b"new"), &a)
        .unwrap();
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.replace_pdf_revision(review, true, &a).is_err());
    assert_eq!(s.source(source.id, &a).unwrap().revision, old);
    assert_eq!(
        s.pdf_anchor(anchor.id, &a).unwrap().health,
        AnchorHealth::Current
    );
    let review = s
        .review_pdf_revision(source.id, "file://new", &hash(b"new"), &a)
        .unwrap();
    s.set_block_lock(b.id, 0, true, &access([Scope::Block(b.id)]))
        .unwrap();
    assert!(matches!(
        s.replace_pdf_revision(review, true, &a),
        Err(Error::Conflict)
    ));
    let review = s
        .review_pdf_revision(source.id, "file://new", &hash(b"new"), &a)
        .unwrap();
    assert!(matches!(
        s.replace_pdf_revision(review, true, &a),
        Err(Error::Denied)
    ));
}
#[test]
fn revoked_source_requires_new_grant_and_exact_revision() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    let old = hash(b"old");
    let source = s.link_pdf(n.id, "file://old", &old, &a).unwrap();
    s.revoke_source(source.id, &a).unwrap();
    assert!(s
        .reacquire_pdf(source.id, "file://new", &old, false, &a)
        .is_err());
    assert!(s
        .reacquire_pdf(source.id, "file://new", &hash(b"different"), true, &a)
        .is_err());
    assert_eq!(
        s.source(source.id, &a).unwrap().availability,
        Availability::Revoked
    );
    s.reacquire_pdf(source.id, "file://new", &old, true, &a)
        .unwrap();
    assert_eq!(
        s.source(source.id, &a).unwrap().availability,
        Availability::Available
    );
}
