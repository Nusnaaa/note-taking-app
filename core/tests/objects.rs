mod common;
use common::*;
use stem_core::{
    content::Content,
    ink::Transform,
    objects::*,
    portable::{ExportProtection, NativeDocument},
    security::{Access, Scope},
    store::*,
    Error, Id,
};
fn rect() -> Rect {
    Rect {
        x: 0.,
        y: 0.,
        width: 100.,
        height: 50.,
    }
}
fn table() -> Table {
    Table {
        rows: vec![vec![
            Cell {
                id: Id::new(),
                value: CellValue::Text("force".into()),
            },
            Cell {
                id: Id::new(),
                value: CellValue::Numeric("000.20".into()),
            },
        ]],
        header_rows: 0,
    }
}
#[test]
fn table_cell_edit_preserves_identity_neighbours_and_source_ink() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    let t = table();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let source = s.source_ink(b.id, &a).unwrap();
    let b = s
        .edit_content(b.id, 0, Content::Table(t.clone()), &a)
        .unwrap();
    let edited = s
        .edit_cell(
            b.id,
            b.revision,
            t.rows[0][0].id,
            CellValue::Text("F".into()),
            &a,
        )
        .unwrap();
    let Content::Table(new) = edited.content else {
        panic!()
    };
    assert_eq!(new.rows[0][1], t.rows[0][1]);
    assert_eq!(new.rows[0][0].id, t.rows[0][0].id);
    assert_eq!(s.source_ink(b.id, &a).unwrap(), source);
    assert!(matches!(
        s.edit_cell(
            b.id,
            b.revision,
            t.rows[0][0].id,
            CellValue::Text("stale".into()),
            &a
        ),
        Err(Error::Conflict)
    ));
}
#[test]
fn invalid_objects_reject_duplicate_ids_ragged_cells_and_nonfinite_axes() {
    let mut t = table();
    t.rows.push(vec![]);
    assert!(t.validate().is_err());
    let mut t = table();
    t.rows[0][1].id = t.rows[0][0].id;
    assert!(t.validate().is_err());
    let axis = Axis {
        label: "x".into(),
        unit: None,
        min: 0.,
        max: f64::INFINITY,
    };
    assert!(Graph {
        x: axis.clone(),
        y: axis,
        series: vec![]
    }
    .validate()
    .is_err());
    assert!(Rect {
        width: f64::NAN,
        ..rect()
    }
    .validate()
    .is_err());
}
#[test]
fn grouped_transform_is_atomic_and_never_changes_original_points() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Canvas, None, "canvas", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let c = s.append_content(n.id, Content::Table(table()), &a).unwrap();
    let g = s.create_group(n.id, &[b.id, c.id], &a).unwrap();
    let raw = s.source_ink(b.id, &a).unwrap();
    s.inject_failure(FailurePoint::BeforeCommit);
    let delta = Transform {
        tx: 17.,
        ty: -23.,
        ..Default::default()
    };
    assert!(s.transform_group(g.id, 0, delta, &a).is_err());
    assert_eq!(s.block(b.id, &a).unwrap().transform, Transform::default());
    s.transform_group(g.id, 0, delta, &a).unwrap();
    assert_eq!(s.block(c.id, &a).unwrap().transform, delta);
    assert_eq!(s.source_ink(b.id, &a).unwrap(), raw);
    s.ungroup(g.id, 1, &a).unwrap();
    assert!(s.block(b.id, &a).is_ok());
}
#[test]
fn canvas_embeds_keep_independent_pinned_cameras_and_reject_cycles() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "host", &a).unwrap();
    let c = s.create(Kind::Canvas, None, "canvas", &a).unwrap();
    let e = s.create_embed(n.id, c.id, rect(), &a).unwrap();
    let camera = Camera {
        centre_x: 13.,
        zoom: 2.,
        ..Default::default()
    };
    let e = s.edit_embed(e.id, 0, camera, rect(), true, &a).unwrap();
    let d = s.duplicate_embed(e.id, &a).unwrap();
    let changed = Camera {
        centre_x: 99.,
        ..camera
    };
    let e = s
        .edit_embed(
            e.id,
            1,
            changed,
            Rect {
                width: 150.,
                ..rect()
            },
            true,
            &a,
        )
        .unwrap();
    assert_eq!(e.camera, camera);
    assert_eq!(s.embed(d.id, &a).unwrap().frame, rect());
    let other = s.create(Kind::Canvas, None, "other", &a).unwrap();
    s.create_embed(c.id, other.id, rect(), &a).unwrap();
    assert!(s.create_embed(other.id, c.id, rect(), &a).is_err());
    let review = s.review_deletion(c.id, &a).unwrap();
    assert!(review.has_dependencies);
    s.remove_embed(e.id, &a).unwrap();
    assert!(s.entity(c.id, &a).is_ok());
}
#[test]
fn protected_canvas_cannot_leak_through_embed_or_group() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "host", &a).unwrap();
    let c = s.create(Kind::Canvas, None, "canvas", &a).unwrap();
    let b = s
        .append_content(c.id, Content::Text("secret".into()), &a)
        .unwrap();
    let g = s.create_group(c.id, &[b.id], &a).unwrap();
    let e = s.create_embed(n.id, c.id, rect(), &a).unwrap();
    s.set_block_lock(b.id, 0, true, &access([Scope::Block(b.id)]))
        .unwrap();
    assert!(s.embed(e.id, &a).is_err());
    assert!(s.group(g.id, &a).is_err());
    s.remove_embed(e.id, &a).unwrap();
    assert!(s.entity(c.id, &a).is_ok());
}
#[test]
fn images_cannot_bypass_locks_even_after_replacing_visible_content() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    let bytes = b"\x89PNG\r\n\x1a\nsynthetic signature fixture";
    let asset = s.import_image(n.id, "image/png", bytes, true, &a).unwrap();
    let b = s
        .append_content(
            n.id,
            Content::Image(Image {
                asset,
                caption: "image".into(),
                crop: None,
            }),
            &a,
        )
        .unwrap();
    let auth = access([Scope::Block(b.id)]);
    let b = s.set_block_lock(b.id, 0, true, &auth).unwrap();
    s.edit_content(b.id, b.revision, Content::Text("replaced".into()), &auth)
        .unwrap();
    assert!(s.image_bytes(asset, &a).is_err());
    assert_eq!(s.image_bytes(asset, &auth).unwrap().1, bytes);
    let other = s.create(Kind::Note, None, "other", &a).unwrap();
    assert!(s
        .append_content(
            other.id,
            Content::Image(Image {
                asset,
                caption: String::new(),
                crop: None
            }),
            &a
        )
        .is_err());
}
#[test]
fn view_settings_and_protected_placeholders_reopen_without_secret_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("views.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let b = s
        .append_content(n.id, Content::Text("secret".into()), &a)
        .unwrap();
    let e = s
        .create_exercise(n.id, "secret exercise", &[b.id], &a)
        .unwrap();
    let v = ViewSettings {
        id: Id::new(),
        entity: n.id,
        exercise: Some(e.id),
        camera: Camera {
            rotation_rad: 1.,
            ..Default::default()
        },
        pen_width_mm: 0.5,
        colour_rgba: [0, 1, 2, 255],
        finger_ink: false,
        anchor: Some(b.id),
        bookmark: None,
        revision: 0,
    };
    s.save_view(v.clone(), None, &a).unwrap();
    assert_eq!(
        s.open_in_master(e.id, Some(b.id), &a).unwrap().block,
        Some(b.id)
    );
    let auth = access([Scope::Exercise(e.id)]);
    s.set_exercise_lock(e.id, 0, true, &auth).unwrap();
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.view(v.id, &auth).unwrap(), v);
    assert!(s.view(v.id, &a).is_err());
    let items = s.surface_items(n.id, 0, 20, &a).unwrap();
    assert!(matches!(items[0], SurfaceItem::Protected { .. }));
    assert!(!format!("{items:?}").contains("secret"));
}
#[test]
fn native_v3_roundtrip_preserves_assets_layout_groups_and_embeds_with_new_ids() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    let c = s.create(Kind::Canvas, None, "canvas", &a).unwrap();
    let asset = s
        .import_image(n.id, "image/png", b"\x89PNG\r\n\x1a\nfixture", true, &a)
        .unwrap();
    let b = s
        .append_content(
            n.id,
            Content::Image(Image {
                asset,
                caption: "preserved".into(),
                crop: None,
            }),
            &a,
        )
        .unwrap();
    s.set_layout(b.id, None, rect(), 2, &a).unwrap();
    s.create_group(n.id, &[b.id], &a).unwrap();
    s.create_embed(n.id, c.id, rect(), &a).unwrap();
    assert!(matches!(
        s.prepare_native_export(&[n.id], true, &a),
        Err(Error::ReviewRequired)
    ));
    let plan = s.prepare_native_export(&[n.id, c.id], true, &a).unwrap();
    let bytes = s
        .finish_native_export(
            plan,
            ExportProtection::Plaintext {
                acknowledge_protected: false,
            },
            &mut TestAuth(true),
            true,
        )
        .unwrap()
        .bytes;
    let d = NativeDocument::decode(&bytes).unwrap();
    assert_eq!(d.version, 8);
    assert_eq!(d.attachments.assets.len(), 1);
    assert_eq!(d.attachments.embeds.len(), 1);
    let mut target = Store::memory().unwrap();
    let imported = target.import_native(&bytes, &a).unwrap();
    let block = target.blocks(imported[0], 0, 20, &a).unwrap().remove(0);
    let Content::Image(image) = block.content else {
        panic!()
    };
    assert_ne!(image.asset, asset);
    assert_ne!(block.id, b.id);
    assert_eq!(
        target.image_bytes(image.asset, &a).unwrap().1,
        d.attachments.assets[0].bytes
    );
    assert_eq!(target.layout(block.id, &a).unwrap().unwrap().bounds, rect());
    target.check_integrity().unwrap();
}
