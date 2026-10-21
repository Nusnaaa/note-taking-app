mod common;
use common::*;
use sha2::{Digest, Sha256};
use stem_core::{
    security::{Access, Scope},
    sources::*,
    store::{Kind, Store},
    Error,
};
fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
#[test]
fn link_default_managed_copy_requires_consent_and_matching_bytes() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "notes", &a).unwrap();
    let bytes = b"%PDF-1.7 synthetic source fixture";
    let source = s
        .link_pdf(n.id, "content://test/pdf", &hash(bytes), &a)
        .unwrap();
    assert_eq!(source.acquisition, Acquisition::ExternalLink);
    assert!(s
        .retain_source_bytes(
            source.id,
            bytes,
            RetentionChoice {
                class: AssetClass::ManagedCopy,
                authority: None,
                export_allowed: false,
                consented: false
            },
            &a
        )
        .is_err());
    assert!(s
        .retain_source_bytes(
            source.id,
            b"different",
            RetentionChoice {
                class: AssetClass::ManagedCopy,
                authority: None,
                export_allowed: false,
                consented: true
            },
            &a
        )
        .is_err());
    let asset = s
        .retain_source_bytes(
            source.id,
            bytes,
            RetentionChoice {
                class: AssetClass::ManagedCopy,
                authority: Some("owner authorised offline copy and export"),
                export_allowed: true,
                consented: true,
            },
            &a,
        )
        .unwrap();
    assert_eq!(s.source_bytes(asset, true, &a).unwrap().bytes, bytes);
    assert_eq!(
        s.source(source.id, &a).unwrap().acquisition,
        Acquisition::ManagedCopy
    );
}
#[test]
fn q383_revocation_removes_caches_preserves_authored_work_and_independent_snapshot() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "authored", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let source = s
        .link_pdf(n.id, "content://test/pdf", &hash(b"pdf"), &a)
        .unwrap();
    let cache = s
        .retain_source_bytes(
            source.id,
            b"cached preview",
            RetentionChoice {
                class: AssetClass::LiveCache,
                authority: None,
                export_allowed: false,
                consented: true,
            },
            &a,
        )
        .unwrap();
    let snapshot = s
        .retain_source_bytes(
            source.id,
            b"authorised excerpt",
            RetentionChoice {
                class: AssetClass::HistoricalSnapshot,
                authority: Some("independent continuing retention"),
                export_allowed: false,
                consented: true,
            },
            &a,
        )
        .unwrap();
    s.revoke_source(source.id, &a).unwrap();
    assert!(s.source_bytes(cache, false, &a).is_err());
    let retained = s.source_bytes(snapshot, false, &a).unwrap();
    assert!(retained.historical);
    assert_eq!(retained.availability, Availability::Revoked);
    assert!(s.source_bytes(snapshot, true, &a).is_err());
    assert_eq!(s.source_ink(b.id, &a).unwrap().len(), 1);
    s.revoke_retention(snapshot, &a).unwrap();
    assert!(s.source_bytes(snapshot, false, &a).is_err());
}
#[test]
fn missing_pdf_view_reopens_and_relink_never_matches_by_filename() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "notes", &a).unwrap();
    let source = s
        .link_pdf(n.id, "file:///old/same.pdf", &hash(b"original"), &a)
        .unwrap();
    let view = PdfView {
        page: 12,
        zoom: 2.,
        pan_x: 100.,
        pan_y: 200.,
        divider: 0.6,
    };
    s.set_pdf_view(source.id, view.clone(), &a).unwrap();
    s.mark_source_missing(source.id, &a).unwrap();
    drop(s);
    let mut s = Store::open(path).unwrap();
    assert_eq!(s.source(source.id, &a).unwrap().view, view);
    assert!(s.entity(n.id, &a).is_ok());
    assert!(matches!(
        s.relink_pdf(source.id, "file:///new/same.pdf", &hash(b"other"), true, &a),
        Err(Error::ReviewRequired)
    ));
    s.relink_pdf(
        source.id,
        "file:///new/same.pdf",
        &hash(b"original"),
        true,
        &a,
    )
    .unwrap();
    assert_eq!(s.source(source.id, &a).unwrap().id, source.id);
}
#[test]
fn source_link_never_bypasses_note_lock_or_creates_network_action() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    assert!(s
        .link_pdf(n.id, "https://example.invalid/send-ink", &hash(b"data"), &a)
        .is_err());
    let source = s
        .link_pdf(n.id, "content://local/pdf", &hash(b"data"), &a)
        .unwrap();
    let cache = s
        .retain_source_bytes(
            source.id,
            b"secret",
            RetentionChoice {
                class: AssetClass::LiveCache,
                authority: None,
                export_allowed: false,
                consented: true,
            },
            &a,
        )
        .unwrap();
    s.set_lock(n.id, 0, true, &access([Scope::Entity(n.id)]))
        .unwrap();
    assert!(s.source(source.id, &a).is_err());
    assert!(s.source_bytes(cache, false, &a).is_err());
}
