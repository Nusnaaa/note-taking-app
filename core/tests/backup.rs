mod common;
use common::*;
use sha2::{Digest, Sha256};
use stem_core::{
    backup::BackupOptions,
    content::Content,
    crypto,
    security::{Access, Scope},
    sources::*,
    store::*,
    Error,
};
fn options() -> BackupOptions {
    BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    }
}
const PASSWORD: &str = "Local recovery example passphrase";
#[test]
fn whole_library_backup_retains_trash_history_membership_undo_and_source_metadata() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let e = s.create_exercise(n.id, "exercise", &[b.id], &a).unwrap();
    let p = s.plan_reassignment(n.id, &[b.id], None, &a).unwrap();
    let op = s.commit_reassignment(p, true, &a).unwrap();
    s.edit_content(b.id, 0, Content::Text("newer working".into()), &a)
        .unwrap();
    let hash = format!("{:x}", Sha256::digest(b"%PDF-external"));
    let source = s.link_pdf(n.id, "content://external", &hash, &a).unwrap();
    let review = s.review_deletion(n.id, &a).unwrap();
    s.trash(review, true, &a).unwrap();
    let original_library = s.library_identity(&a).unwrap();
    let backup = s
        .finish_backup(
            s.prepare_backup(options(), &a).unwrap(),
            PASSWORD,
            &mut TestAuth(true),
            true,
        )
        .unwrap();
    let mut restored = Store::restore_backup_password(&backup.bytes, PASSWORD).unwrap();
    assert_ne!(restored.library_identity(&a).unwrap(), original_library);
    assert!(restored.entity(n.id, &a).is_err());
    restored.restore(n.id, &a).unwrap();
    assert_eq!(
        restored.source(source.id, &a).unwrap().availability,
        Availability::Missing
    );
    assert_eq!(
        restored.block(b.id, &a).unwrap().content,
        Content::Text("newer working".into())
    );
    assert_eq!(restored.source_ink(b.id, &a).unwrap().len(), 1);
    assert_eq!(restored.history(b.id, &a).unwrap().len(), 2);
    restored.undo_reassignment(op.id, &a).unwrap();
    assert_eq!(
        restored.exercise_blocks(e.id, 0, 20, &a).unwrap()[0].id,
        b.id
    );
    assert!(s.entity(n.id, &a).is_err());
    restored.check_integrity().unwrap();
}
#[test]
fn backup_requires_deliberate_ink_scope_and_fresh_protected_authentication() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "private", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    assert!(matches!(
        s.prepare_backup(
            BackupOptions {
                include_original_ink: false,
                ..options()
            },
            &a
        ),
        Err(Error::ReviewRequired)
    ));
    let auth = access([Scope::Block(b.id)]);
    s.set_block_lock(b.id, 0, true, &auth).unwrap();
    assert!(s.prepare_backup(options(), &a).is_err());
    let p = s.prepare_backup(options(), &auth).unwrap();
    assert!(matches!(
        s.finish_backup(p, PASSWORD, &mut TestAuth(false), true),
        Err(Error::Denied)
    ));
    let p = s.prepare_backup(options(), &auth).unwrap();
    let encrypted = s
        .finish_backup(p, PASSWORD, &mut TestAuth(true), true)
        .unwrap();
    let restored =
        Store::restore_backup_recovery(&encrypted.bytes, &encrypted.recovery_key).unwrap();
    assert!(restored.block(b.id, &a).is_err());
    assert!(restored.block(b.id, &auth).is_ok());
    let dir = tempfile::tempdir().unwrap();
    assert!(restored
        .persist_restored_library(
            &dir.path().join("blocked.db"),
            &a,
            &mut TestAuth(true),
            true
        )
        .is_err());
    assert!(!dir.path().join("blocked.db").exists());
}
#[test]
fn external_asset_backup_is_explicit_authorised_and_revocation_safe() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    let bytes = b"%PDF-independent snapshot";
    let hash = format!("{:x}", Sha256::digest(bytes));
    let source = s.link_pdf(n.id, "file://external", &hash, &a).unwrap();
    let asset = s
        .retain_source_bytes(
            source.id,
            bytes,
            RetentionChoice {
                class: AssetClass::HistoricalSnapshot,
                authority: Some("Synthetic independent retention consent"),
                export_allowed: true,
                consented: true,
            },
            &a,
        )
        .unwrap();
    s.revoke_source(source.id, &a).unwrap();
    let p = s.prepare_backup(options(), &a).unwrap();
    assert_eq!(p.source_assets, 0);
    assert!(!p.omissions.is_empty());
    let p = s
        .prepare_backup(
            BackupOptions {
                include_authorised_source_assets: true,
                ..options()
            },
            &a,
        )
        .unwrap();
    assert_eq!(p.source_assets, 1);
    let encrypted = s
        .finish_backup(p, PASSWORD, &mut TestAuth(true), true)
        .unwrap();
    let restored = Store::restore_backup_password(&encrypted.bytes, PASSWORD).unwrap();
    assert_eq!(
        restored.source(source.id, &a).unwrap().availability,
        Availability::Revoked
    );
    assert_eq!(restored.source_bytes(asset, true, &a).unwrap().bytes, bytes);
}
#[test]
fn restore_publication_never_overwrites_live_database_and_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("restored.db");
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "restored", &a).unwrap();
    let encrypted = s
        .finish_backup(
            s.prepare_backup(options(), &a).unwrap(),
            PASSWORD,
            &mut TestAuth(true),
            true,
        )
        .unwrap();
    let restored = Store::restore_backup_password(&encrypted.bytes, PASSWORD).unwrap();
    restored
        .persist_restored_library(&path, &a, &mut TestAuth(true), true)
        .unwrap();
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.entity(n.id, &a).unwrap().title, "restored");
    drop(reopened);
    let before = std::fs::read(&path).unwrap();
    assert!(restored
        .persist_restored_library(&path, &a, &mut TestAuth(true), true)
        .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
#[test]
fn malformed_authenticated_backup_is_rejected_before_exposing_restored_store() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let encrypted = s
        .finish_backup(
            s.prepare_backup(options(), &a).unwrap(),
            PASSWORD,
            &mut TestAuth(true),
            true,
        )
        .unwrap();
    let plain = crypto::decrypt(
        &encrypted.bytes,
        crypto::Credential::Recovery(&encrypted.recovery_key),
    )
    .unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    value["tables"]["entities"][0][2]["value"] = serde_json::json!("{}");
    let bad = crypto::encrypt(&serde_json::to_vec(&value).unwrap(), PASSWORD).unwrap();
    assert!(Store::restore_backup_recovery(&bad.bytes, &bad.recovery_key).is_err());
    assert_eq!(s.entity(n.id, &a).unwrap().title, "note");
}
#[test]
fn backup_plan_cannot_cross_library_or_survive_a_write() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let other = Store::memory().unwrap();
    let plan = s.prepare_backup(options(), &a).unwrap();
    assert!(matches!(
        other.finish_backup(plan, PASSWORD, &mut TestAuth(true), true),
        Err(Error::Conflict)
    ));
    let plan = s.prepare_backup(options(), &a).unwrap();
    s.create(Kind::Note, None, "changed", &a).unwrap();
    assert!(matches!(
        s.finish_backup(plan, PASSWORD, &mut TestAuth(true), true),
        Err(Error::Conflict)
    ));
}

#[test]
fn rich_library_restores_objects_views_anchors_proposals_profiles_and_capture_receipts() {
    use std::{io::Cursor, time::Duration};
    use stem_core::{objects::*, processing::*, Id, Result};

    // A deterministic compiled adapter exercises provenance, never model accuracy.
    struct Synthetic;
    impl Recogniser for Synthetic {
        fn probe(&self) -> Result<()> {
            Ok(())
        }
        fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
            Ok(RecognitionOutput {
                candidates: vec![Content::Text("reviewed transcription".into())],
                raw_output: "synthetic output".into(),
                confidence: None,
                diagnostics: vec![],
            })
        }
    }
    let a = Access::public();
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "technical note", &a).unwrap();
    let c = s.create(Kind::Canvas, None, "linked canvas", &a).unwrap();
    let bounds = Rect {
        x: 0.,
        y: 0.,
        width: 100.,
        height: 50.,
    };
    let ticket = Id::new();
    let strokes = vec![stroke(0)];
    let ink = s
        .append_capture(ticket, WritingTarget::Entity(n.id), strokes.clone(), &a)
        .unwrap();
    let graph = Content::Graph(Graph {
        x: Axis {
            label: "time".into(),
            unit: Some("s".into()),
            min: 0.,
            max: 2.,
        },
        y: Axis {
            label: "measured force".into(),
            unit: Some("N".into()),
            min: -1.,
            max: 5.,
        },
        series: vec![Series {
            id: Id::new(),
            label: "authored observations".into(),
            points: vec![
                Position { x: 0.1, y: -0.0 },
                Position {
                    x: 1.2345678901234567,
                    y: 4.2,
                },
            ],
        }],
    });
    let gb = s.append_content(c.id, graph.clone(), &a).unwrap();
    s.set_layout(gb.id, None, bounds, 4, &a).unwrap();
    let group = s.create_group(c.id, &[gb.id], &a).unwrap();
    let embed = s.create_embed(n.id, c.id, bounds, &a).unwrap();
    let image_bytes = b"\x89PNG\r\n\x1a\nsynthetic signature only";
    let asset = s
        .import_image(n.id, "image/png", image_bytes, true, &a)
        .unwrap();
    s.append_content(
        n.id,
        Content::Image(Image {
            asset,
            caption: "stored image".into(),
            crop: None,
        }),
        &a,
    )
    .unwrap();
    let view = ViewSettings {
        id: Id::new(),
        entity: n.id,
        exercise: None,
        camera: Camera::default(),
        pen_width_mm: 0.7,
        colour_rgba: [10, 20, 30, 255],
        finger_ink: true,
        anchor: Some(ink.id),
        revision: 0,
    };
    s.save_view(view.clone(), None, &a).unwrap();
    let pdf = s
        .link_pdf(
            n.id,
            "content://fixture",
            &format!("{:x}", Sha256::digest(b"%PDF-fixture")),
            &a,
        )
        .unwrap();
    let anchor = s
        .anchor_pdf(
            pdf.id,
            ink.id,
            3,
            Rect {
                width: 0.5,
                height: 0.5,
                ..bounds
            },
            true,
            &a,
        )
        .unwrap();
    let d = Descriptor {
        id: "backup-fixture".into(),
        version: "1".into(),
        adapter_version: "1".into(),
        family: Family::Recogniser,
        input: InputContract::OrderedStrokesV1,
        tasks: vec![Task::Prose],
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some("app-owned synthetic test fixture".into()),
        redistributable_resources: false,
        resource_sha256: Some(format!("{:x}", Sha256::digest(b"fixture resource"))),
        runtime_contract: "stem-processing-v1".into(),
    };
    let version = s
        .stage_engine(
            d,
            Some(&mut Cursor::new(b"fixture resource")),
            dir.path(),
            100,
            true,
            &a,
        )
        .unwrap();
    let mut manager = Manager::new();
    s.activate_engine(version.id, &mut manager, Box::new(Synthetic), 100, &a)
        .unwrap();
    s.select_engine(Task::Prose, "backup-fixture", &mut manager, &a)
        .unwrap();
    let proposal = s
        .propose(
            manager
                .recognise(
                    s.ink_snapshot(ink.id, &a).unwrap(),
                    Task::Prose,
                    None,
                    &JobControl::new(Duration::from_secs(1)).unwrap(),
                )
                .unwrap(),
            &a,
        )
        .unwrap();
    s.set_history_hours(24., &a).unwrap();

    let encrypted = s
        .finish_backup(
            s.prepare_backup(options(), &a).unwrap(),
            PASSWORD,
            &mut TestAuth(true),
            true,
        )
        .unwrap();
    let mut restored =
        Store::restore_backup_recovery(&encrypted.bytes, &encrypted.recovery_key).unwrap();
    assert_eq!(restored.block(gb.id, &a).unwrap().content, graph);
    assert_eq!(restored.layout(gb.id, &a).unwrap().unwrap().z, 4);
    assert_eq!(restored.group(group.id, &a).unwrap(), group);
    assert_eq!(restored.embed(embed.id, &a).unwrap(), embed);
    assert_eq!(restored.image_bytes(asset, &a).unwrap().1, image_bytes);
    assert_eq!(restored.view(view.id, &a).unwrap(), view);
    assert_eq!(restored.pdf_anchor(anchor.id, &a).unwrap(), anchor);
    assert_eq!(restored.history_hours().unwrap(), 24.);
    assert_eq!(
        restored.engine_defaults(&a).unwrap(),
        vec![(Task::Prose, "backup-fixture".into())]
    );
    assert!(restored
        .engine_version(version.id, &a)
        .unwrap()
        .local_resource
        .is_none());
    let mut fresh = Manager::new();
    assert!(restored
        .activate_engine(version.id, &mut fresh, Box::new(Synthetic), 100, &a)
        .is_err());
    assert_eq!(fresh.readiness("backup-fixture"), Readiness::Missing);
    assert_eq!(
        restored
            .append_capture(ticket, WritingTarget::Entity(n.id), strokes.clone(), &a)
            .unwrap()
            .id,
        ink.id
    );
    restored.accept_proposal(proposal.id, 0, &a).unwrap();
    assert_eq!(restored.source_ink(ink.id, &a).unwrap(), strokes);
    assert_eq!(
        s.proposal(proposal.id, &a).unwrap().state,
        ProposalState::Pending
    );
    restored.check_integrity().unwrap();
}
