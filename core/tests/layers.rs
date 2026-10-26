mod common;
use common::*;
use stem_core::{
    backup::BackupOptions,
    content::Content,
    crypto::{self, Credential},
    export::Selection,
    objects::Rect,
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
fn canvas(s: &mut Store) -> (Id, Vec<Block>) {
    let a = Access::public();
    let c = s.create(Kind::Canvas, None, "canvas", &a).unwrap();
    let mut blocks = vec![];
    for i in 0..3 {
        blocks.push(
            s.append_content(c.id, Content::Text(format!("item {i}")), &a)
                .unwrap(),
        )
    }
    (c.id, blocks)
}
fn state(canvas: Id, b: Id) -> CanvasLayers {
    CanvasLayers {
        canvas,
        revision: 0,
        base_visible: false,
        layers: vec![CanvasLayer {
            id: Id::new(),
            name: "visible".into(),
            visible: true,
            blocks: vec![b],
        }],
    }
}
fn ids(scene: CanvasScene) -> Vec<Id> {
    scene
        .items
        .into_iter()
        .filter_map(|e| match e.item {
            CanvasSceneContent::Block {
                item: SurfaceItem::Visible(b),
                ..
            } => Some(b.id),
            _ => None,
        })
        .collect()
}
fn blob(s: &Store) -> Vec<u8> {
    let a = Access::public();
    s.finish_backup(
        s.prepare_backup(
            BackupOptions {
                include_original_ink: true,
                include_authorised_source_assets: false,
            },
            &a,
        )
        .unwrap(),
        "Canvas layer recovery passphrase",
        &mut TestAuth(true),
        true,
    )
    .unwrap()
    .bytes
}
fn native(s: &Store, roots: &[Id]) -> Vec<u8> {
    let a = Access::public();
    s.finish_native_export(
        s.prepare_native_export(roots, true, &a).unwrap(),
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
fn defaults_and_independent_embed_overrides_preserve_source_and_report_missing_layers() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let (c, b) = canvas(&mut s);
    let value = s.set_canvas_layers(state(c, b[0].id), 0, &a).unwrap();
    let custom = LayerRef::Custom(value.layers[0].id);
    assert_eq!(
        ids(s.canvas_scene(c, None, 0, 10, &a).unwrap()),
        vec![b[0].id]
    );
    let host = s.create(Kind::Note, None, "host", &a).unwrap();
    let first = s.create_embed(host.id, c, rect(), &a).unwrap();
    let second = s.create_embed(host.id, c, rect(), &a).unwrap();
    s.set_embed_layers(first.id, 0, Some(vec![LayerRef::Base]), &a)
        .unwrap();
    assert_eq!(
        ids(s.canvas_scene(c, Some(first.id), 0, 10, &a).unwrap()),
        vec![b[1].id, b[2].id]
    );
    assert_eq!(
        ids(s.canvas_scene(c, Some(second.id), 0, 10, &a).unwrap()),
        vec![b[0].id]
    );
    assert_eq!(s.canvas_layers(c, &a).unwrap(), value);
    let duplicate = s.duplicate_embed(first.id, &a).unwrap();
    assert_eq!(duplicate.visible_layers, Some(vec![LayerRef::Base]));
    s.set_embed_layers(second.id, 0, Some(vec![custom]), &a)
        .unwrap();
    let mut removed = value.clone();
    removed.layers.clear();
    removed.base_visible = true;
    s.set_canvas_layers(removed, 1, &a).unwrap();
    let stale = s.canvas_scene(c, Some(second.id), 0, 10, &a).unwrap();
    assert!(stale.items.is_empty());
    assert_eq!(stale.unresolved, vec![custom]);
    s.undo(c, &a).unwrap();
    assert_eq!(
        ids(s.canvas_scene(c, Some(second.id), 0, 10, &a).unwrap()),
        vec![b[0].id]
    );
    s.set_embed_layers(second.id, 1, Some(vec![]), &a).unwrap();
    assert!(s
        .canvas_scene(c, Some(second.id), 0, 10, &a)
        .unwrap()
        .items
        .is_empty());
    s.set_embed_layers(second.id, 2, None, &a).unwrap();
    assert_eq!(
        ids(s.canvas_scene(c, Some(second.id), 0, 10, &a).unwrap()),
        vec![b[0].id]
    );
    for old in b {
        assert_eq!(s.block(old.id, &a).unwrap(), old)
    }
}
#[test]
fn layer_undo_redo_and_failure_are_atomic_and_revisions_never_reset_to_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("layers.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let (c, b) = canvas(&mut s);
    let original = stroke(0);
    let ink = s.append_ink(c, vec![original.clone()], &a).unwrap();
    let value = s.set_canvas_layers(state(c, b[0].id), 0, &a).unwrap();
    s.undo(c, &a).unwrap();
    assert_eq!(s.canvas_layers(c, &a).unwrap().revision, 2);
    assert_eq!(ids(s.canvas_scene(c, None, 0, 10, &a).unwrap()).len(), 4);
    assert!(matches!(
        s.set_canvas_layers(value.clone(), 0, &a),
        Err(Error::Conflict)
    ));
    s.redo(c, &a).unwrap();
    assert_eq!(s.canvas_layers(c, &a).unwrap().revision, 3);
    let before = s.canvas_layers(c, &a).unwrap();
    let mut change = before.clone();
    change.base_visible = true;
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.set_canvas_layers(change, 3, &a).is_err());
    assert_eq!(s.canvas_layers(c, &a).unwrap(), before);
    drop(s);
    let mut s = Store::open(&path).unwrap();
    s.undo(c, &a).unwrap();
    assert_eq!(s.canvas_layers(c, &a).unwrap().revision, 4);
    assert_eq!(s.source_ink(ink.id, &a).unwrap(), vec![original]);
    s.redo(c, &a).unwrap();
    assert_eq!(s.canvas_layers(c, &a).unwrap().revision, 5);
}
#[test]
fn malformed_membership_aliases_and_fresh_protection_cannot_leak_or_change_layers() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let (c, b) = canvas(&mut s);
    let mut value = state(c, b[0].id);
    value.layers[0].blocks.push(b[1].id);
    let value = s.set_canvas_layers(value, 0, &a).unwrap();
    let mut bad = value.clone();
    bad.layers.push(bad.layers[0].clone());
    assert!(s.set_canvas_layers(bad, 1, &a).is_err());
    let mut bad = value.clone();
    bad.layers[0].id = b[0].id;
    assert!(s.set_canvas_layers(bad, 1, &a).is_err());
    let other = s.create(Kind::Note, None, "other", &a).unwrap();
    let foreign = s
        .append_content(other.id, Content::Text("foreign".into()), &a)
        .unwrap();
    let mut bad = value.clone();
    bad.layers[0].blocks.push(foreign.id);
    assert!(s.set_canvas_layers(bad, 1, &a).is_err());
    assert_eq!(s.canvas_layers(c, &a).unwrap(), value);
    let auth = access([Scope::Block(b[0].id)]);
    s.edit_content(b[0].id, 0, Content::Text("private text".into()), &a)
        .unwrap();
    s.set_block_lock(b[0].id, 1, true, &auth).unwrap();
    assert!(matches!(s.canvas_layers(c, &a), Err(Error::Denied)));
    assert!(matches!(
        s.set_canvas_layers(value.clone(), 1, &a),
        Err(Error::Denied)
    ));
    assert!(matches!(s.undo(c, &a), Err(Error::Denied)));
    let scene = s.canvas_scene(c, None, 0, 10, &a).unwrap();
    assert!(!serde_json::to_string(&scene)
        .unwrap()
        .contains("private text"));
    assert!(matches!(
        scene.items[0].item,
        CanvasSceneContent::Block {
            item: SurfaceItem::Protected { .. },
            ..
        }
    ));
    assert!(s.canvas_layers(c, &auth).is_ok());
    assert!(s.canvas_scene(c, None, 0, 513, &auth).is_err());
}
#[test]
fn scene_orders_layers_then_z_and_paginates_without_dropping_nested_reference_placeholders() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let (c, b) = canvas(&mut s);
    let mut value = state(c, b[0].id);
    value.layers[0].blocks.push(b[1].id);
    s.set_canvas_layers(value, 0, &a).unwrap();
    s.set_layout(b[0].id, None, rect(), 5, &a).unwrap();
    s.set_layout(b[1].id, None, rect(), -2, &a).unwrap();
    assert_eq!(
        ids(s.canvas_scene(c, None, 0, 1, &a).unwrap()),
        vec![b[1].id]
    );
    assert_eq!(
        ids(s.canvas_scene(c, None, 1, 1, &a).unwrap()),
        vec![b[0].id]
    );
    let child = s.create(Kind::Canvas, None, "child", &a).unwrap();
    let e = s.create_embed(c, child.id, rect(), &a).unwrap();
    let mut value = s.canvas_layers(c, &a).unwrap();
    value.base_visible = true;
    s.set_canvas_layers(value, 1, &a).unwrap();
    assert!(s
        .canvas_scene(c, None, 0, 10, &a)
        .unwrap()
        .items
        .iter()
        .any(|e| matches!(e.item, CanvasSceneContent::Embed(_))));
    let auth = access([Scope::Entity(child.id)]);
    s.set_lock(child.id, 0, true, &auth).unwrap();
    assert!(s
        .canvas_scene(c, None, 0, 10, &a)
        .unwrap()
        .items
        .iter()
        .any(|e| matches!(e.item, CanvasSceneContent::ProtectedEmbed(_))));
    s.set_lock(child.id, 1, false, &auth).unwrap();
    let review = s.review_deletion(child.id, &a).unwrap();
    s.trash(review, true, &a).unwrap();
    s.purge(child.id, &a).unwrap();
    assert!(s
        .canvas_scene(c, None, 0, 10, &a)
        .unwrap()
        .items
        .iter()
        .any(|i| i.item == CanvasSceneContent::UnavailableEmbed(e.id)));
}
#[test]
fn native_copy_remaps_layer_and_embed_overrides_and_derivatives_respect_default_visibility() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let (c, b) = canvas(&mut s);
    let value = s.set_canvas_layers(state(c, b[0].id), 0, &a).unwrap();
    let plan = s
        .prepare_derivative(Selection::Document(c), false, &a)
        .unwrap();
    let bundle = s
        .finish_derivative(plan, &mut TestAuth(true), true)
        .unwrap();
    assert!(bundle.markdown.contains("item 0"));
    assert!(!bundle.markdown.contains("item 1"));
    assert!(bundle.warnings.iter().any(|w| w.contains("canvas layer")));
    let host = s.create(Kind::Note, None, "host", &a).unwrap();
    let e = s.create_embed(host.id, c, rect(), &a).unwrap();
    s.set_embed_layers(
        e.id,
        0,
        Some(vec![LayerRef::Custom(value.layers[0].id)]),
        &a,
    )
    .unwrap();
    let bytes = native(&s, &[c, host.id]);
    let mut doc = NativeDocument::decode(&bytes).unwrap();
    assert_eq!(doc.version, 10);
    doc.version = 7;
    assert!(doc.validate().is_err());
    let mut target = Store::memory().unwrap();
    let roots = target.import_native(&bytes, &a).unwrap();
    let copied = NativeDocument::decode(&native(&target, &roots)).unwrap();
    let state = &copied.attachments.layers[0];
    let embed = &copied.attachments.embeds[0];
    assert_ne!(state.canvas, c);
    assert_ne!(state.layers[0].id, value.layers[0].id);
    assert_ne!(embed.id, e.id);
    assert_eq!(embed.source, state.canvas);
    assert_eq!(
        embed.visible_layers,
        Some(vec![LayerRef::Custom(state.layers[0].id)])
    );
    assert_eq!(
        ids(target
            .canvas_scene(state.canvas, Some(embed.id), 0, 10, &a)
            .unwrap())
        .len(),
        1
    );
}
#[test]
fn encrypted_backup_preserves_layer_undo_and_rejects_downgraded_current_and_historical_extensions()
{
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let (c, b) = canvas(&mut s);
    s.set_canvas_layers(state(c, b[0].id), 0, &a).unwrap();
    let bytes = blob(&s);
    let password = "Canvas layer recovery passphrase";
    let mut restored = Store::restore_backup_password(&bytes, password).unwrap();
    assert_eq!(
        ids(restored.canvas_scene(c, None, 0, 10, &a).unwrap()),
        vec![b[0].id]
    );
    restored.undo(c, &a).unwrap();
    assert_eq!(
        ids(restored.canvas_scene(c, None, 0, 10, &a).unwrap()).len(),
        3
    );
    let plain = crypto::decrypt(&bytes, Credential::Password(password)).unwrap();
    let mut old: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    old["tables"]
        .as_object_mut()
        .unwrap()
        .remove("engine_resource_actions");
    old["tables"]
        .as_object_mut()
        .unwrap()
        .remove("engine_reclamations");
    old["schema"] = 13.into();
    old["tables"]
        .as_object_mut()
        .unwrap()
        .remove("canvas_layers");
    let bad = crypto::encrypt(&serde_json::to_vec(&old).unwrap(), password).unwrap();
    assert!(Store::restore_backup_password(&bad.bytes, password).is_err());
    let mut s = Store::memory().unwrap();
    let (c, _) = canvas(&mut s);
    let h = s.create(Kind::Note, None, "host", &a).unwrap();
    let e = s.create_embed(h.id, c, rect(), &a).unwrap();
    s.set_embed_layers(e.id, 0, Some(vec![LayerRef::Base]), &a)
        .unwrap();
    s.undo(h.id, &a).unwrap();
    assert!(s.embed(e.id, &a).unwrap().visible_layers.is_none());
    let bytes = blob(&s);
    let plain = crypto::decrypt(&bytes, Credential::Password(password)).unwrap();
    let mut old: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    old["tables"]
        .as_object_mut()
        .unwrap()
        .remove("engine_resource_actions");
    old["tables"]
        .as_object_mut()
        .unwrap()
        .remove("engine_reclamations");
    old["schema"] = 13.into();
    old["tables"]
        .as_object_mut()
        .unwrap()
        .remove("canvas_layers");
    let bad = crypto::encrypt(&serde_json::to_vec(&old).unwrap(), password).unwrap();
    assert!(Store::restore_backup_password(&bad.bytes, password).is_err());
}
#[test]
fn legacy_canvas_migrates_to_an_implicit_base_without_rewriting_original_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let c = s.create(Kind::Canvas, None, "canvas", &a).unwrap();
    let source = stroke(0);
    let b = s.append_ink(c.id, vec![source.clone()], &a).unwrap();
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    let before: (String, String) = db
        .query_row("SELECT payload,digest FROM strokes", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    db.execute_batch("DROP TRIGGER workspace_layer_identity;DROP TRIGGER stroke_layer_identity;DROP TRIGGER view_layer_identity;DROP TABLE canvas_layers;").unwrap();
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
    db.pragma_update(None, "user_version", 13).unwrap();
    drop(db);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 18);
    assert_eq!(s.canvas_layers(c.id, &a).unwrap().revision, 0);
    assert_eq!(
        ids(s.canvas_scene(c.id, None, 0, 10, &a).unwrap()),
        vec![b.id]
    );
    assert_eq!(s.source_ink(b.id, &a).unwrap(), vec![source]);
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
fn scene_byte_pages_have_explicit_continuations_and_never_load_denied_payloads_into_results() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let c = s.create(Kind::Canvas, None, "large", &a).unwrap();
    let mut blocks = vec![];
    for _ in 0..3 {
        blocks.push(
            s.append_content(c.id, Content::Text("x".repeat(3_000_000)), &a)
                .unwrap(),
        );
    }
    let page = s.canvas_scene(c.id, None, 0, 100, &a).unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.next_offset, Some(2));
    assert!(serde_json::to_vec(&page).unwrap().len() <= 8 * 1024 * 1024);
    let next = s
        .canvas_scene(c.id, None, page.next_offset.unwrap(), 100, &a)
        .unwrap();
    assert_eq!(next.items.len(), 1);
    assert_eq!(next.next_offset, None);
    assert_eq!(next.entity_revision, page.entity_revision);
    let auth = access([Scope::Block(blocks[2].id)]);
    s.set_block_lock(blocks[2].id, 0, true, &auth).unwrap();
    let protected = s.canvas_scene(c.id, None, 2, 1, &a).unwrap();
    assert!(matches!(
        protected.items[0].item,
        CanvasSceneContent::Block {
            item: SurfaceItem::Protected { .. },
            ..
        }
    ));
    assert!(serde_json::to_vec(&protected).unwrap().len() < 1024);
    assert!(protected.entity_revision > page.entity_revision);
}
