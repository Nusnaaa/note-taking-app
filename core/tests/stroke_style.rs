mod common;
use common::*;
use stem_core::{
    backup::BackupOptions,
    capture::{InputRouter, PointerKind},
    content::Content,
    crypto::{self, Credential},
    export::Selection,
    ink::{Interruption, StrokeStyle},
    portable::{ExportProtection, NativeDocument},
    security::Access,
    store::{Kind, Store, WritingTarget},
    Id,
};
fn style() -> StrokeStyle {
    StrokeStyle {
        colour_rgba: [17, 34, 255, 128],
        width: 7.25,
    }
}
fn export(s: &Store, id: Id) -> Vec<u8> {
    let a = Access::public();
    s.finish_native_export(
        s.prepare_native_export(&[id], true, &a).unwrap(),
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
fn style_is_snapshotted_on_begin_and_configuration_changes_do_not_rebind_active_ink() {
    let mut router = InputRouter::new("chosen-source-space", false, 10).unwrap();
    let samples = stroke(0).points;
    router.set_style(Some(style())).unwrap();
    router
        .begin(1, PointerKind::Pen, samples[0].clone())
        .unwrap();
    let mut next = style();
    next.width = 0.12345678901234567;
    router.set_style(Some(next)).unwrap();
    router.sample(1, samples[1].clone()).unwrap();
    router
        .begin(2, PointerKind::Eraser, samples[0].clone())
        .unwrap();
    let active = router.checkpoint();
    assert_eq!(active[0].style, Some(style()));
    assert_eq!(active[1].style, Some(next));
    let interrupted = router.interrupt(Interruption::Lifecycle);
    assert_eq!(interrupted[0].points, samples);
    assert_eq!(interrupted[0].style, Some(style()));
    for width in [0., -1., f64::NAN, f64::INFINITY, 1_000_001.] {
        assert!(router
            .set_style(Some(StrokeStyle { width, ..style() }))
            .is_err());
    }
    router
        .begin(3, PointerKind::Pen, interrupted[0].points[0].clone())
        .unwrap();
    assert_eq!(router.finish(3).unwrap().style, Some(next));
}
#[test]
fn source_style_and_receipt_survive_edits_reopen_native_copy_and_legacy_format_validation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("style.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let ticket = Id::new();
    let mut original = stroke(0);
    original.style = Some(style());
    let b = s
        .append_capture(
            ticket,
            WritingTarget::Entity(n.id),
            vec![original.clone()],
            &a,
        )
        .unwrap();
    let mut changed = original.clone();
    changed.style.as_mut().unwrap().colour_rgba[0] = 200;
    assert!(s
        .append_capture(ticket, WritingTarget::Entity(n.id), vec![changed], &a)
        .is_err());
    s.edit_content(b.id, 0, Content::Text("correction".into()), &a)
        .unwrap();
    drop(s);
    let mut s = Store::open(&path).unwrap();
    s.append_capture(
        ticket,
        WritingTarget::Entity(n.id),
        vec![original.clone()],
        &a,
    )
    .unwrap();
    assert_eq!(s.source_ink(b.id, &a).unwrap(), vec![original.clone()]);
    let bytes = export(&s, n.id);
    let mut doc = NativeDocument::decode(&bytes).unwrap();
    assert_eq!(doc.version, 9);
    doc.version = 6;
    assert!(doc.validate().is_err());
    doc.blocks[0].strokes[0].style = None;
    doc.validate().unwrap();
    let mut target = Store::memory().unwrap();
    let ids = target.import_native(&bytes, &a).unwrap();
    let copied = target.blocks(ids[0], 0, 10, &a).unwrap().remove(0);
    let mut actual = target.source_ink(copied.id, &a).unwrap().remove(0);
    actual.id = original.id;
    assert_eq!(actual, original);
}
#[test]
fn encrypted_style_backup_retains_exact_metadata_and_cannot_be_disguised_as_schema12() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let mut original = stroke(0);
    original.style = Some(style());
    let b = s.append_ink(n.id, vec![original.clone()], &a).unwrap();
    let options = BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    };
    let password = "Chosen stroke style recovery passphrase";
    let bytes = s
        .finish_backup(
            s.prepare_backup(options, &a).unwrap(),
            password,
            &mut TestAuth(true),
            true,
        )
        .unwrap()
        .bytes;
    let restored = Store::restore_backup_password(&bytes, password).unwrap();
    assert_eq!(restored.source_ink(b.id, &a).unwrap(), vec![original]);
    let plain = crypto::decrypt(&bytes, Credential::Password(password)).unwrap();
    let mut doc: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    doc["schema"] = 12.into();
    doc["tables"]
        .as_object_mut()
        .unwrap()
        .remove("canvas_layers");
    let bad = crypto::encrypt(&serde_json::to_vec(&doc).unwrap(), password).unwrap();
    assert!(Store::restore_backup_password(&bad.bytes, password).is_err());
}
#[test]
fn legacy_source_payload_hash_and_receipt_are_not_rewritten_by_schema13_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let source = stroke(0);
    let ticket = Id::new();
    s.append_capture(
        ticket,
        WritingTarget::Entity(n.id),
        vec![source.clone()],
        &a,
    )
    .unwrap();
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    let before: (String, String) = db
        .query_row(
            "SELECT payload,digest FROM strokes WHERE id=?1",
            [source.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(!before.0.contains("style"));
    db.execute_batch("DROP TRIGGER workspace_layer_identity; DROP TRIGGER stroke_layer_identity; DROP TRIGGER view_layer_identity; DROP TABLE canvas_layers;").unwrap();
    db.pragma_update(None, "user_version", 12).unwrap();
    drop(db);
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 15);
    s.append_capture(
        ticket,
        WritingTarget::Entity(n.id),
        vec![source.clone()],
        &a,
    )
    .unwrap();
    assert_eq!(
        s.source_ink(s.blocks(n.id, 0, 10, &a).unwrap()[0].id, &a)
            .unwrap(),
        vec![source]
    );
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    let after: (String, String) = db
        .query_row("SELECT payload,digest FROM strokes", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(before, after);
}
#[test]
fn original_svg_preserves_rgba_width_and_single_sample_dots_and_reports_unknown_appearance() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let mut dot = stroke(0);
    dot.points.truncate(1);
    dot.style = Some(style());
    let b = s.append_ink(n.id, vec![dot.clone()], &a).unwrap();
    let plan = s
        .prepare_derivative(Selection::Blocks(vec![b.id]), true, &a)
        .unwrap();
    let bundle = s
        .finish_derivative(plan, &mut TestAuth(true), true)
        .unwrap();
    let svg = String::from_utf8(bundle.assets.values().next().unwrap().clone()).unwrap();
    assert!(svg.contains("<circle"));
    assert!(svg.contains("#1122ff"));
    assert!(svg.contains("r=\"3.625\""));
    assert!(svg.contains("fill-opacity=\"0.5019607843137255\""));
    assert!(!bundle.warnings.iter().any(|w| w.contains("Legacy ink")));
    assert_eq!(s.source_ink(b.id, &a).unwrap(), vec![dot]);
    let mut line = stroke(1);
    line.style = Some(style());
    let b = s.append_ink(n.id, vec![line], &a).unwrap();
    let bundle = s
        .finish_derivative(
            s.prepare_derivative(Selection::Blocks(vec![b.id]), true, &a)
                .unwrap(),
            &mut TestAuth(true),
            true,
        )
        .unwrap();
    let svg = String::from_utf8(bundle.assets.values().next().unwrap().clone()).unwrap();
    assert!(svg.contains("stroke-width=\"7.25\""));
    assert!(bundle.warnings.iter().any(|w| w.contains("pressure/tilt")));
    let b = s.append_ink(n.id, vec![stroke(2)], &a).unwrap();
    let bundle = s
        .finish_derivative(
            s.prepare_derivative(Selection::Blocks(vec![b.id]), true, &a)
                .unwrap(),
            &mut TestAuth(true),
            true,
        )
        .unwrap();
    assert!(bundle.warnings.iter().any(|w| w.contains("Legacy ink")));
}
