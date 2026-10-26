mod common;
use common::*;
use stem_core::{
    capture::{InputRouter, PendingCapture, PersistenceQueue, PointerKind},
    ink::{Interruption, StrokeEnd},
    portable::{ExportProtection, NativeDocument},
    security::Access,
    store::{Kind, Store, WritingTarget},
    Error, Id,
};
fn export(s: &Store, note: Id) -> Vec<u8> {
    let plan = s
        .prepare_native_export(&[note], true, &Access::public())
        .unwrap();
    s.finish_native_export(
        plan,
        ExportProtection::Plaintext {
            acknowledge_protected: false,
        },
        &mut TestAuth(true),
        true,
    )
    .unwrap()
    .bytes
}
#[test]
fn checkpoints_preserve_stable_begin_order_and_interruption_never_invents_samples() {
    let mut router = InputRouter::new("fixed-space", false, 3).unwrap();
    let points = stroke(0).points;
    router
        .begin(9, PointerKind::Pen, points[0].clone())
        .unwrap();
    router
        .begin(1, PointerKind::Eraser, points[0].clone())
        .unwrap();
    let first = router.checkpoint();
    assert_eq!(
        first.iter().map(|s| s.pointer_id).collect::<Vec<_>>(),
        vec![9, 1]
    );
    assert!(first.iter().all(|s| s.end == StrokeEnd::InProgress));
    router.sample(9, points[1].clone()).unwrap();
    assert!(matches!(
        router.sample(1, points[1].clone()),
        Err(Error::ResourceLimit)
    ));
    let snapshot = router.checkpoint();
    assert_eq!(snapshot[0].id, first[0].id);
    assert_eq!(first[0].points, vec![points[0].clone()]);
    let interrupted = router.interrupt(Interruption::Lifecycle);
    assert_eq!(router.active_count(), 0);
    for (actual, observed) in interrupted.iter().zip(&snapshot) {
        assert_eq!(actual.id, observed.id);
        assert_eq!(actual.sequence, observed.sequence);
        assert_eq!(actual.points, observed.points);
        assert_eq!(actual.end, StrokeEnd::Interrupted(Interruption::Lifecycle));
        assert_eq!(actual.points[0].y.to_bits(), (-0.0f64).to_bits());
    }
    router
        .begin(2, PointerKind::Pen, points[1].clone())
        .unwrap();
    let active = router.checkpoint().pop().unwrap();
    let finished = router.finish(2).unwrap();
    assert_eq!(active.id, finished.id);
    assert_eq!(finished.end, StrokeEnd::Completed);
    assert_eq!(finished.sequence, 2);
}
#[test]
fn unfinished_prefix_cannot_enter_store_queue_or_portable_document_and_sealed_retry_is_exact() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "pending", &a).unwrap();
    let mut prefix = stroke(0);
    prefix.end = StrokeEnd::InProgress;
    let ticket = Id::new();
    assert!(s.append_ink(n.id, vec![prefix.clone()], &a).is_err());
    assert!(s
        .append_capture(
            ticket,
            WritingTarget::Entity(n.id),
            vec![prefix.clone()],
            &a
        )
        .is_err());
    let mut queue = PersistenceQueue::new(1_000_000, 4).unwrap();
    assert!(queue
        .enqueue(&PendingCapture {
            ticket,
            target: WritingTarget::Entity(n.id),
            strokes: vec![prefix.clone()]
        })
        .is_err());
    assert!(s.blocks(n.id, 0, 10, &a).unwrap().is_empty());
    prefix.end = StrokeEnd::Interrupted(Interruption::Recovery);
    let block = s
        .append_capture(
            ticket,
            WritingTarget::Entity(n.id),
            vec![prefix.clone()],
            &a,
        )
        .unwrap();
    assert_eq!(
        s.append_capture(
            ticket,
            WritingTarget::Entity(n.id),
            vec![prefix.clone()],
            &a
        )
        .unwrap()
        .id,
        block.id
    );
    let mut completed = prefix.clone();
    completed.end = StrokeEnd::Completed;
    assert!(matches!(
        s.append_capture(ticket, WritingTarget::Entity(n.id), vec![completed], &a),
        Err(Error::Conflict)
    ));
    let mut native = NativeDocument::decode(&export(&s, n.id)).unwrap();
    native.blocks[0].strokes[0].end = StrokeEnd::InProgress;
    assert!(native.validate().is_err());
    assert_eq!(s.source_ink(block.id, &a).unwrap(), vec![prefix]);
}
#[test]
fn schema9_upgrade_preserves_original_payload_hash_and_capture_receipt_byte_for_byte() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("old.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "legacy", &a).unwrap();
    let original = stroke(0);
    let ticket = Id::new();
    let b = s
        .append_capture(
            ticket,
            WritingTarget::Entity(n.id),
            vec![original.clone()],
            &a,
        )
        .unwrap();
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    let before: (String, String) = db
        .query_row(
            "SELECT payload,digest FROM strokes WHERE id=?1",
            [original.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(!before.0.contains("\"end\""));
    db.execute_batch("DROP TRIGGER IF EXISTS workspace_layer_identity; DROP TRIGGER IF EXISTS stroke_layer_identity; DROP TRIGGER IF EXISTS view_layer_identity; DROP TABLE canvas_layers; DROP TRIGGER view_workspace_identity; DROP TRIGGER stroke_workspace_identity; DROP TABLE workspace_state; DROP TABLE workspace_panes;")
        .unwrap();
    // Remove the schema17-only empty action table from this legacy layout.
    assert_eq!(
        db.query_row("SELECT count(*) FROM engine_resource_actions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM engine_reclamations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    db.execute_batch("DROP TABLE engine_reclamations; DROP TABLE engine_resource_actions;")
        .unwrap();
    common::remove_linked_history_extension(&db);
    db.pragma_update(None, "user_version", 9).unwrap();
    drop(db);
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 22);
    assert_eq!(
        s.append_capture(
            ticket,
            WritingTarget::Entity(n.id),
            vec![original.clone()],
            &a
        )
        .unwrap()
        .id,
        b.id
    );
    assert_eq!(s.source_ink(b.id, &a).unwrap(), vec![original.clone()]);
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    let after: (String, String) = db
        .query_row(
            "SELECT payload,digest FROM strokes WHERE id=?1",
            [original.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(before, after);
}
#[test]
fn native_v5_preserves_interruption_provenance_and_legacy_tags_cannot_hide_it() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "interrupted", &a).unwrap();
    let mut original = stroke(0);
    original.end = StrokeEnd::Interrupted(Interruption::Lifecycle);
    s.append_ink(n.id, vec![original.clone()], &a).unwrap();
    let bytes = export(&s, n.id);
    let doc = NativeDocument::decode(&bytes).unwrap();
    assert_eq!(doc.version, 13);
    let mut imported = Store::memory().unwrap();
    let new = imported.import_native(&bytes, &a).unwrap();
    let b = imported.blocks(new[0], 0, 10, &a).unwrap().pop().unwrap();
    let copy = imported.source_ink(b.id, &a).unwrap().pop().unwrap();
    assert_ne!(copy.id, original.id);
    assert_eq!(copy.end, original.end);
    assert_eq!(copy.points, original.points);
    for version in 1..=4 {
        let mut old = doc.clone();
        old.version = version;
        assert!(old.validate().is_err());
    }
    let mut legacy = doc.clone();
    legacy.blocks[0].strokes[0].end = StrokeEnd::Completed;
    for version in 1..=4 {
        legacy.version = version;
        legacy.modes.clear();
        assert!(legacy.validate().is_ok(), "v{version}");
    }
}
#[test]
fn encrypted_schema10_retains_interruption_and_old_schema_cannot_claim_it() {
    use stem_core::{
        backup::BackupOptions,
        crypto::{self, Credential},
    };
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "recovery", &a).unwrap();
    let mut original = stroke(0);
    original.end = StrokeEnd::Interrupted(Interruption::Recovery);
    let b = s.append_ink(n.id, vec![original.clone()], &a).unwrap();
    let plan = s
        .prepare_backup(
            BackupOptions {
                include_original_ink: true,
                include_authorised_source_assets: false,
            },
            &a,
        )
        .unwrap();
    let password = "independent capture recovery passphrase";
    let backup = s
        .finish_backup(plan, password, &mut TestAuth(true), true)
        .unwrap();
    let restored = Store::restore_backup_password(&backup.bytes, password).unwrap();
    assert_eq!(restored.schema_version().unwrap(), 22);
    assert_eq!(restored.source_ink(b.id, &a).unwrap(), vec![original]);
    let plain = crypto::decrypt(&backup.bytes, Credential::Password(password)).unwrap();
    let mut downgraded: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    assert_eq!(downgraded["schema"], 22);
    downgraded["tables"]
        .as_object_mut()
        .unwrap()
        .remove("engine_resource_actions");
    downgraded["tables"]
        .as_object_mut()
        .unwrap()
        .remove("engine_reclamations");
    downgraded["schema"] = 9.into();
    downgraded["tables"]
        .as_object_mut()
        .unwrap()
        .remove("canvas_layers");
    downgraded["tables"]
        .as_object_mut()
        .unwrap()
        .remove("workspace_panes");
    downgraded["tables"]
        .as_object_mut()
        .unwrap()
        .remove("workspace_state");
    let forged = crypto::encrypt(&serde_json::to_vec(&downgraded).unwrap(), password).unwrap();
    assert!(Store::restore_backup_password(&forged.bytes, password).is_err());
}
#[test]
fn legacy_schema9_backup_defaults_to_completed_without_rewriting_source_or_receipt() {
    use stem_core::{
        backup::BackupOptions,
        crypto::{self, Credential},
    };
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "old recovery", &a).unwrap();
    let original = stroke(0);
    let ticket = Id::new();
    let b = s
        .append_capture(
            ticket,
            WritingTarget::Entity(n.id),
            vec![original.clone()],
            &a,
        )
        .unwrap();
    let plan = s
        .prepare_backup(
            BackupOptions {
                include_original_ink: true,
                include_authorised_source_assets: false,
            },
            &a,
        )
        .unwrap();
    let password = "legacy capture recovery passphrase";
    let backup = s
        .finish_backup(plan, password, &mut TestAuth(true), true)
        .unwrap();
    let plain = crypto::decrypt(&backup.bytes, Credential::Password(password)).unwrap();
    let mut old: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    old["tables"]
        .as_object_mut()
        .unwrap()
        .remove("engine_resource_actions");
    old["tables"]
        .as_object_mut()
        .unwrap()
        .remove("engine_reclamations");
    old["schema"] = 9.into();
    old["tables"]
        .as_object_mut()
        .unwrap()
        .remove("canvas_layers");
    old["tables"]
        .as_object_mut()
        .unwrap()
        .remove("workspace_panes");
    old["tables"]
        .as_object_mut()
        .unwrap()
        .remove("workspace_state");
    let bytes = crypto::encrypt(&serde_json::to_vec(&old).unwrap(), password).unwrap();
    let mut restored = Store::restore_backup_password(&bytes.bytes, password).unwrap();
    assert_eq!(
        restored
            .append_capture(
                ticket,
                WritingTarget::Entity(n.id),
                vec![original.clone()],
                &a
            )
            .unwrap()
            .id,
        b.id
    );
    assert_eq!(restored.source_ink(b.id, &a).unwrap(), vec![original]);
}
