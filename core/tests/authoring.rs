mod common;
use common::*;
use stem_core::{
    backup::BackupOptions,
    content::Content,
    crypto::{self, Credential},
    objects::Rect,
    portable::{ExportProtection, NativeDocument},
    security::{Access, Scope},
    store::*,
    Error, Id,
};
fn view(s: &mut Store) -> ViewSettings {
    let a = Access::public();
    let n = s.create(Kind::Note, None, "authoring", &a).unwrap();
    s.save_view(
        ViewSettings {
            id: Id::new(),
            entity: n.id,
            exercise: None,
            camera: Camera {
                centre_x: 3.,
                centre_y: -2.,
                zoom: 2.,
                rotation_rad: 0.5,
            },
            pen_width_mm: 0.5,
            colour_rgba: [0, 0, 0, 255],
            finger_ink: false,
            anchor: None,
            bookmark: None,
            authoring: None,
            revision: 0,
        },
        None,
        &a,
    )
    .unwrap()
}
fn config() -> AuthoringSettings {
    let first = Id::new();
    AuthoringSettings {
        pressure_response: None,
        eraser: None,
        presets: vec![
            PenPreset {
                pressure_response: None,
                id: first,
                name: "fine blue".into(),
                width_mm: 0.6,
                colour_rgba: [17, 128, 255, 64],
            },
            PenPreset {
                pressure_response: None,
                id: Id::new(),
                name: "wide".into(),
                width_mm: 1.2,
                colour_rgba: [255, 0, 0, 255],
            },
        ],
        selected_preset: Some(first),
        background: Some(GuideBackground {
            kind: GuideKind::Graph,
            spacing_mm: 5.,
            line_width_mm: 0.2,
            origin_mm: [0., 0.],
            colour_rgba: [100, 100, 100, 128],
            paper_rgba: [255, 255, 255, 255],
            include_in_export: false,
        }),
    }
}
fn bounds() -> Rect {
    Rect {
        x: -10.,
        y: -5.,
        width: 100.,
        height: 80.,
    }
}
fn backup(s: &Store) -> Vec<u8> {
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
        "Authoring recovery example passphrase",
        &mut TestAuth(true),
        true,
    )
    .unwrap()
    .bytes
}
#[test]
fn presets_calibrate_and_reopen_without_moving_view_or_rewriting_observations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("author.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let v = view(&mut s);
    let original = stroke(0);
    let b = s.append_ink(v.entity, vec![original.clone()], &a).unwrap();
    let c = config();
    let wide = c.presets[1].id;
    let updated = s.configure_view(v.id, 0, Some(c.clone()), &a).unwrap();
    assert_eq!(updated.camera, v.camera);
    assert_eq!(updated.pen_width_mm, 0.6);
    let snapshot = s.view_pen(v.id, "device-pixels", 4., &a).unwrap();
    assert_eq!(snapshot.style.width, 2.4);
    assert_eq!(snapshot.style.colour_rgba, c.presets[0].colour_rgba);
    let selected = s.apply_view_preset(v.id, 1, wide, &a).unwrap();
    assert_eq!(selected.pen_width_mm, 1.2);
    assert!(matches!(
        s.apply_view_preset(v.id, 1, wide, &a),
        Err(Error::Conflict)
    ));
    assert!(s
        .view_pen(v.id, "device-pixels", f64::INFINITY, &a)
        .is_err());
    assert!(s.view_pen(v.id, "device-pixels", 1_000_000., &a).is_err());
    assert_eq!(s.source_ink(b.id, &a).unwrap(), vec![original.clone()]);
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.view(v.id, &a).unwrap(), selected);
    assert_eq!(s.source_ink(b.id, &a).unwrap(), vec![original]);
}
#[test]
fn preferences_preserve_stale_selection_and_do_not_consume_document_undo() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let mut v = view(&mut s);
    let b = s
        .append_content(v.entity, Content::Text("first".into()), &a)
        .unwrap();
    let point = CaretPoint {
        block: b.id,
        revision: 0,
        region: None,
        target: CaretTarget::Boundary { after: false },
    };
    v.bookmark = Some(SelectionBookmark {
        anchor: point.clone(),
        focus: point,
    });
    let v = s.save_view(v, Some(0), &a).unwrap();
    s.edit_content(b.id, 0, Content::Text("changed".into()), &a)
        .unwrap();
    assert_eq!(
        s.restore_view(v.id, &a).unwrap().bookmark_state,
        BookmarkState::NeedsReview
    );
    let configured = s
        .configure_view(v.id, v.revision, Some(config()), &a)
        .unwrap();
    assert_eq!(configured.bookmark, v.bookmark);
    assert_eq!(
        s.restore_view(v.id, &a).unwrap().bookmark_state,
        BookmarkState::NeedsReview
    );
    s.undo(v.entity, &a).unwrap();
    assert_eq!(
        s.block(b.id, &a).unwrap().content,
        Content::Text("first".into())
    );
    assert_eq!(s.view(v.id, &a).unwrap().authoring, configured.authoring);
    let proof = access([Scope::Block(b.id)]);
    let revision = s.block(b.id, &a).unwrap().revision;
    s.set_block_lock(b.id, revision, true, &proof).unwrap();
    assert!(matches!(
        s.view_pen(v.id, "space", 1., &a),
        Err(Error::Denied)
    ));
    assert!(matches!(
        s.configure_view(v.id, configured.revision, None, &a),
        Err(Error::Denied)
    ));
}
#[test]
fn guide_kinds_are_bounded_escaped_and_excluded_from_export_only_by_explicit_choice() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let v = view(&mut s);
    let absent = s
        .view_guides(v.id, bounds(), "space", 4., false, &a)
        .unwrap();
    assert!(!absent.configured);
    assert!(absent.svg.is_none());
    let mut c = config();
    let mut revision = 0;
    for kind in [
        GuideKind::Blank,
        GuideKind::Ruled,
        GuideKind::Dotted,
        GuideKind::Graph,
    ] {
        c.background.as_mut().unwrap().kind = kind;
        revision = s
            .configure_view(v.id, revision, Some(c.clone()), &a)
            .unwrap()
            .revision;
        let svg = s
            .view_guides(v.id, bounds(), "a\"<script>", 4., false, &a)
            .unwrap()
            .svg
            .unwrap();
        assert!(!svg.contains("<script>"));
        assert!(svg.contains("&quot;&lt;script&gt;"));
        assert!(svg.contains("fill-opacity=\"1\""));
        assert_eq!(svg.contains("<pattern"), kind != GuideKind::Blank);
        let omitted = s
            .view_guides(v.id, bounds(), "space", 4., true, &a)
            .unwrap();
        assert!(omitted.omitted_for_export);
        assert!(omitted.svg.is_none());
    }
    c.background.as_mut().unwrap().include_in_export = true;
    s.configure_view(v.id, revision, Some(c.clone()), &a)
        .unwrap();
    assert!(s
        .view_guides(v.id, bounds(), "space", 4., true, &a)
        .unwrap()
        .svg
        .is_some());
    c.background.as_mut().unwrap().kind = GuideKind::Dotted;
    c.background.as_mut().unwrap().spacing_mm = 0.000_001;
    c.background.as_mut().unwrap().line_width_mm = 0.000_000_1;
    s.configure_view(v.id, revision + 1, Some(c), &a).unwrap();
    assert!(matches!(
        s.view_guides(v.id, bounds(), "space", 1., false, &a),
        Err(Error::ResourceLimit)
    ));
}
#[test]
fn invalid_or_dangling_presets_reject_atomically_without_silent_selection_repair() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let v = view(&mut s);
    let mut c = config();
    c.selected_preset = Some(Id::new());
    assert!(s.configure_view(v.id, 0, Some(c), &a).is_err());
    assert_eq!(s.view(v.id, &a).unwrap(), v);
    let mut c = config();
    c.presets[1].id = c.presets[0].id;
    assert!(s.configure_view(v.id, 0, Some(c), &a).is_err());
    let c = config();
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.configure_view(v.id, 0, Some(c), &a).is_err());
    assert_eq!(s.view(v.id, &a).unwrap(), v);
}
#[test]
fn native_copy_and_schema15_backup_keep_scoped_presets_and_reject_downgraded_extensions() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let v = view(&mut s);
    let c = config();
    s.configure_view(v.id, 0, Some(c.clone()), &a).unwrap();
    let plan = s.prepare_native_export(&[v.entity], true, &a).unwrap();
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
    let doc = NativeDocument::decode(&bytes).unwrap();
    assert_eq!(doc.version, 10);
    let mut old = doc.clone();
    old.version = 8;
    assert!(old.validate().is_err());
    old.attachments.views[0].authoring = None;
    old.validate().unwrap();
    let mut copied = Store::memory().unwrap();
    let imported = copied.import_native(&bytes, &a).unwrap();
    let ids = copied.views(imported[0], None, 0, 10, &a).unwrap();
    let saved = copied.view(ids[0], &a).unwrap();
    assert_ne!(saved.id, v.id);
    assert_eq!(saved.authoring, Some(Box::new(c)));
    let blob = backup(&s);
    let opened = crypto::decrypt(
        &blob,
        Credential::Password("Authoring recovery example passphrase"),
    )
    .unwrap();
    let mut snapshot: serde_json::Value = serde_json::from_slice(&opened).unwrap();
    assert_eq!(snapshot["schema"], 16);
    snapshot["schema"] = 14.into();
    let wrong = crypto::encrypt(
        &serde_json::to_vec(&snapshot).unwrap(),
        "Authoring recovery example passphrase",
    )
    .unwrap();
    assert!(
        Store::restore_backup_password(&wrong.bytes, "Authoring recovery example passphrase")
            .is_err()
    );
    let payload = &mut snapshot["tables"]["view_settings"][0][2]["value"];
    let mut legacy_view: serde_json::Value =
        serde_json::from_str(payload.as_str().unwrap()).unwrap();
    legacy_view.as_object_mut().unwrap().remove("authoring");
    *payload = legacy_view.to_string().into();
    let legacy = crypto::encrypt(
        &serde_json::to_vec(&snapshot).unwrap(),
        "Authoring recovery example passphrase",
    )
    .unwrap();
    let legacy_restored =
        Store::restore_backup_password(&legacy.bytes, "Authoring recovery example passphrase")
            .unwrap();
    assert!(legacy_restored.view(v.id, &a).unwrap().authoring.is_none());
    assert_eq!(legacy_restored.view(v.id, &a).unwrap().pen_width_mm, 0.6);
    let restored =
        Store::restore_backup_password(&blob, "Authoring recovery example passphrase").unwrap();
    assert_eq!(
        restored.view(v.id, &a).unwrap().authoring,
        s.view(v.id, &a).unwrap().authoring
    );
}

#[test]
fn schema14_upgrade_keeps_unconfigured_view_payload_exactly_without_inventing_presets_or_guides() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let v = view(&mut s);
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    let original: String = db
        .query_row(
            "SELECT payload FROM view_settings WHERE id=?1",
            [v.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!original.contains("authoring"));
    db.pragma_update(None, "user_version", 14).unwrap();
    drop(db);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 16);
    assert_eq!(s.view(v.id, &a).unwrap(), v);
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    let after: String = db
        .query_row(
            "SELECT payload FROM view_settings WHERE id=?1",
            [v.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(original.as_bytes(), after.as_bytes());
}
