mod common;
use common::*;
use stem_core::{
    backup::BackupOptions,
    content::{Content, MathForm, MathNode},
    crypto::{self, Credential},
    objects::{Cell, CellValue, Table},
    portable::{ExportProtection, NativeDocument},
    security::{Access, Scope},
    store::*,
    Error, Id,
};
fn point(b: &Block, target: CaretTarget) -> CaretPoint {
    CaretPoint {
        block: b.id,
        revision: b.revision,
        region: None,
        target,
    }
}
fn text(b: &Block, offset: u32) -> CaretPoint {
    point(b, CaretTarget::Inline(InlinePoint::Text { offset }))
}
fn view(entity: Id, anchor: CaretPoint, focus: CaretPoint) -> ViewSettings {
    ViewSettings {
        id: Id::new(),
        entity,
        exercise: None,
        camera: Camera::default(),
        pen_width_mm: 0.5,
        colour_rgba: [0, 0, 0, 255],
        finger_ink: false,
        anchor: None,
        authoring: None,
        bookmark: Some(SelectionBookmark { anchor, focus }),
        revision: 0,
    }
}
fn export(s: &Store, entity: Id) -> Vec<u8> {
    let a = Access::public();
    let plan = s.prepare_native_export(&[entity], true, &a).unwrap();
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
fn unicode_selection_reopens_and_changed_content_requires_explicit_fresh_bookmark() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("positions.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s
        .append_content(n.id, Content::Text("A😀e\u{301}".into()), &a)
        .unwrap();
    let v = s
        .save_view(view(n.id, text(&b, 4), text(&b, 1)), None, &a)
        .unwrap();
    assert_eq!(s.block(b.id, &a).unwrap(), b);
    drop(s);
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.restore_view(v.id, &a).unwrap().settings, v);
    assert_eq!(
        s.restore_view(v.id, &a).unwrap().bookmark_state,
        BookmarkState::Current
    );
    let changed = s
        .edit_content(b.id, 0, Content::Text("B😀e\u{301}".into()), &a)
        .unwrap();
    assert_eq!(
        s.restore_view(v.id, &a).unwrap().bookmark_state,
        BookmarkState::NeedsReview
    );
    assert!(matches!(
        s.save_view(v.clone(), Some(0), &a),
        Err(Error::Conflict)
    ));
    let mut fresh = v.clone();
    fresh.bookmark = Some(SelectionBookmark {
        anchor: text(&changed, 4),
        focus: text(&changed, 4),
    });
    let saved = s.save_view(fresh, Some(0), &a).unwrap();
    assert_eq!(saved.revision, 1);
    let mut invalid = saved.clone();
    invalid.bookmark.as_mut().unwrap().anchor = text(&changed, 5);
    assert!(s.save_view(invalid, Some(1), &a).is_err());
    assert_eq!(s.view(v.id, &a).unwrap(), saved);
    let mut future = saved.clone();
    future.bookmark.as_mut().unwrap().focus.revision += 1;
    assert!(s.save_view(future, Some(1), &a).is_err());
    let mut clear = saved.clone();
    clear.bookmark = None;
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.save_view(clear.clone(), Some(1), &a).is_err());
    s.inject_failure(FailurePoint::None);
    assert_eq!(s.view(v.id, &a).unwrap(), saved);
    s.save_view(clear, Some(1), &a).unwrap();
    assert_eq!(
        s.restore_view(v.id, &a).unwrap().bookmark_state,
        BookmarkState::None
    );
}
#[test]
fn structured_math_and_component_offsets_validate_without_interpreting_or_repairing_content() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let leaf = MathNode {
        id: Id::new(),
        form: MathForm::Unparsed("x+😀?".into()),
    };
    let root = MathNode {
        id: Id::new(),
        form: MathForm::Row(vec![leaf.clone()]),
    };
    let b = s
        .append_content(n.id, Content::Maths(root.clone()), &a)
        .unwrap();
    let start = point(
        &b,
        CaretTarget::Inline(InlinePoint::Maths {
            node: root.id,
            offset: 1,
        }),
    );
    let end = point(
        &b,
        CaretTarget::Inline(InlinePoint::Maths {
            node: leaf.id,
            offset: 4,
        }),
    );
    let saved = s
        .save_view(view(n.id, start.clone(), end.clone()), None, &a)
        .unwrap();
    assert_eq!(
        s.restore_view(saved.id, &a).unwrap().bookmark_state,
        BookmarkState::Current
    );
    let wrong = point(
        &b,
        CaretTarget::Inline(InlinePoint::Maths {
            node: root.id,
            offset: 2,
        }),
    );
    assert!(s
        .save_view(view(n.id, wrong, end.clone()), None, &a)
        .is_err());
    let absent = point(
        &b,
        CaretTarget::Inline(InlinePoint::Maths {
            node: Id::new(),
            offset: 0,
        }),
    );
    assert!(s.save_view(view(n.id, start, absent), None, &a).is_err());
    let cell = Cell {
        id: Id::new(),
        value: CellValue::Maths(leaf),
    };
    let table = s
        .append_content(
            n.id,
            Content::Table(Table {
                rows: vec![vec![cell.clone()]],
                header_rows: 0,
            }),
            &a,
        )
        .unwrap();
    let cell_point = point(
        &table,
        CaretTarget::Cell {
            cell: cell.id,
            point: InlinePoint::Maths {
                node: match &cell.value {
                    CellValue::Maths(m) => m.id,
                    _ => unreachable!(),
                },
                offset: 0,
            },
        },
    );
    s.save_view(view(n.id, cell_point.clone(), cell_point), None, &a)
        .unwrap();
    let wrong = point(
        &table,
        CaretTarget::Cell {
            cell: cell.id,
            point: InlinePoint::Text { offset: 0 },
        },
    );
    assert!(s
        .save_view(view(n.id, wrong.clone(), wrong), None, &a)
        .is_err());
    assert_eq!(s.block(b.id, &a).unwrap().content, Content::Maths(root));
}
#[test]
fn exercise_membership_and_current_block_protection_gate_bookmark_metadata_and_replacement() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s
        .append_content(n.id, Content::Text("secret".into()), &a)
        .unwrap();
    let first = s.create_exercise(n.id, "one", &[b.id], &a).unwrap();
    let second = s.create_exercise(n.id, "two", &[], &a).unwrap();
    let master = s
        .save_view(view(n.id, text(&b, 0), text(&b, 6)), None, &a)
        .unwrap();
    let mut exercise = master.clone();
    exercise.id = Id::new();
    exercise.exercise = Some(first.id);
    let exercise = s.save_view(exercise, None, &a).unwrap();
    let plan = s
        .plan_reassignment(n.id, &[b.id], Some(second.id), &a)
        .unwrap();
    s.commit_reassignment(plan, true, &a).unwrap();
    assert_eq!(
        s.restore_view(exercise.id, &a).unwrap().bookmark_state,
        BookmarkState::NeedsReview
    );
    assert_eq!(
        s.restore_view(master.id, &a).unwrap().bookmark_state,
        BookmarkState::Current
    );
    let protected = access([Scope::Block(b.id)]);
    s.set_block_lock(b.id, 0, true, &protected).unwrap();
    assert!(matches!(s.restore_view(master.id, &a), Err(Error::Denied)));
    let mut clear = master.clone();
    clear.bookmark = None;
    assert!(matches!(
        s.save_view(clear.clone(), Some(0), &a),
        Err(Error::Denied)
    ));
    assert_eq!(
        s.restore_view(master.id, &protected)
            .unwrap()
            .bookmark_state,
        BookmarkState::NeedsReview
    );
    s.save_view(clear, Some(0), &protected).unwrap();
    assert_eq!(
        s.restore_view(master.id, &a).unwrap().bookmark_state,
        BookmarkState::None
    );
    assert_ne!(master.id, exercise.id);
}
#[test]
fn observed_ink_region_bookmarks_copy_global_ids_and_old_format_tags_cannot_hide_them() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let source = stroke(0);
    let b = s.append_ink(n.id, vec![source.clone()], &a).unwrap();
    let b = s
        .partition_regions(b.id, 0, vec![vec![source.id]], true, &a)
        .unwrap();
    let mut caret = point(
        &b,
        CaretTarget::Ink {
            stroke: source.id,
            sample: 1,
        },
    );
    caret.region = Some(b.regions[0].id);
    let mut invalid = caret.clone();
    invalid.target = CaretTarget::Ink {
        stroke: source.id,
        sample: 99,
    };
    assert!(s
        .save_view(view(n.id, invalid.clone(), invalid), None, &a)
        .is_err());
    let v = s
        .save_view(view(n.id, caret.clone(), caret), None, &a)
        .unwrap();
    let bytes = export(&s, n.id);
    let mut doc = NativeDocument::decode(&bytes).unwrap();
    assert_eq!(doc.version, 9);
    doc.version = 5;
    assert!(doc.validate().is_err());
    doc.attachments.views[0].bookmark = None;
    doc.validate().unwrap();
    let mut target = Store::memory().unwrap();
    let roots = target.import_native(&bytes, &a).unwrap();
    let copied = NativeDocument::decode(&export(&target, roots[0])).unwrap();
    let saved = &copied.attachments.views[0];
    assert_ne!(saved.id, v.id);
    let selected = &saved.bookmark.as_ref().unwrap().anchor;
    assert_ne!(selected.block, b.id);
    assert_ne!(selected.region, Some(b.regions[0].id));
    let CaretTarget::Ink { stroke, sample } = selected.target else {
        panic!()
    };
    assert_ne!(stroke, source.id);
    assert_eq!(sample, 1);
    assert_eq!(
        target.restore_view(saved.id, &a).unwrap().bookmark_state,
        BookmarkState::Current
    );
    let mut actual = target.source_ink(selected.block, &a).unwrap().remove(0);
    actual.id = source.id;
    assert_eq!(actual, source);
    let mut stale = s.block(b.id, &a).unwrap();
    stale = s
        .edit_region(
            stale.id,
            stale.revision,
            b.regions[0].id,
            Content::Text("edited".into()),
            &a,
        )
        .unwrap();
    assert!(stale.revision > b.revision);
    let stale_bytes = export(&s, n.id);
    let stale_ids = target.import_native(&stale_bytes, &a).unwrap();
    let copied = NativeDocument::decode(&export(&target, stale_ids[0])).unwrap();
    assert_eq!(
        target
            .restore_view(copied.attachments.views[0].id, &a)
            .unwrap()
            .bookmark_state,
        BookmarkState::NeedsReview
    );
}
#[test]
fn encrypted_backup_retains_stale_bookmarks_and_schema10_defaults_without_rewriting_views() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s
        .append_content(n.id, Content::Text("abc".into()), &a)
        .unwrap();
    let v = s
        .save_view(view(n.id, text(&b, 1), text(&b, 2)), None, &a)
        .unwrap();
    s.edit_content(b.id, 0, Content::Text("changed".into()), &a)
        .unwrap();
    let options = BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    };
    let password = "Selection recovery example passphrase";
    let backup = s
        .finish_backup(
            s.prepare_backup(options, &a).unwrap(),
            password,
            &mut TestAuth(true),
            true,
        )
        .unwrap();
    let restored = Store::restore_backup_password(&backup.bytes, password).unwrap();
    assert_eq!(restored.view(v.id, &a).unwrap(), v);
    assert_eq!(
        restored.restore_view(v.id, &a).unwrap().bookmark_state,
        BookmarkState::NeedsReview
    );
    let plain = crypto::decrypt(&backup.bytes, Credential::Password(password)).unwrap();
    let mut old: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    old["schema"] = 10.into();
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
    let forged = crypto::encrypt(&serde_json::to_vec(&old).unwrap(), password).unwrap();
    assert!(Store::restore_backup_password(&forged.bytes, password).is_err());
    for row in old["tables"]["view_settings"].as_array_mut().unwrap() {
        let mut payload: serde_json::Value =
            serde_json::from_str(row[2]["value"].as_str().unwrap()).unwrap();
        payload.as_object_mut().unwrap().remove("bookmark");
        row[2]["value"] = serde_json::to_string(&payload).unwrap().into();
    }
    let legacy = crypto::encrypt(&serde_json::to_vec(&old).unwrap(), password).unwrap();
    let restored = Store::restore_backup_password(&legacy.bytes, password).unwrap();
    assert_eq!(restored.schema_version().unwrap(), 15);
    assert!(restored.view(v.id, &a).unwrap().bookmark.is_none());
}
#[test]
fn rich_labels_and_multiblock_ranges_preserve_content_and_reject_foreign_owners() {
    use stem_core::objects::{Axis, Diagram, DiagramPart, Graph, Image, Rect, Series, Shape};
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Canvas, None, "n", &a).unwrap();
    let axis = Axis {
        label: "α😀".into(),
        unit: Some("m".into()),
        min: 0.,
        max: 1.,
    };
    let series = Series {
        id: Id::new(),
        label: "01?".into(),
        points: vec![],
    };
    let graph = s
        .append_content(
            n.id,
            Content::Graph(Graph {
                x: axis.clone(),
                y: axis,
                series: vec![series.clone()],
            }),
            &a,
        )
        .unwrap();
    let part = DiagramPart {
        id: Id::new(),
        shape: Shape::Rectangle(Rect {
            x: 0.,
            y: 0.,
            width: 2.,
            height: 1.,
        }),
        label: "x?".into(),
    };
    let diagram = s
        .append_content(
            n.id,
            Content::Diagram(Diagram {
                parts: vec![part.clone()],
            }),
            &a,
        )
        .unwrap();
    let asset = s
        .import_image(n.id, "image/png", b"\x89PNG\r\n\x1a\nfixture", true, &a)
        .unwrap();
    let image = s
        .append_content(
            n.id,
            Content::Image(Image {
                asset,
                caption: "😀?".into(),
                crop: None,
            }),
            &a,
        )
        .unwrap();
    let code = s
        .append_content(
            n.id,
            Content::Code {
                text: "x = ?".into(),
                language: None,
            },
            &a,
        )
        .unwrap();
    let targets = vec![
        point(
            &graph,
            CaretTarget::GraphLabel {
                label: GraphLabel::XLabel,
                offset: 2,
            },
        ),
        point(
            &graph,
            CaretTarget::GraphLabel {
                label: GraphLabel::XUnit,
                offset: 1,
            },
        ),
        point(
            &graph,
            CaretTarget::GraphLabel {
                label: GraphLabel::YLabel,
                offset: 2,
            },
        ),
        point(
            &graph,
            CaretTarget::GraphLabel {
                label: GraphLabel::YUnit,
                offset: 1,
            },
        ),
        point(
            &graph,
            CaretTarget::GraphLabel {
                label: GraphLabel::Series(series.id),
                offset: 3,
            },
        ),
        point(
            &diagram,
            CaretTarget::DiagramLabel {
                part: part.id,
                offset: 2,
            },
        ),
        point(&image, CaretTarget::ImageCaption { offset: 2 }),
        point(&image, CaretTarget::Boundary { after: true }),
        text(&code, 5),
    ];
    for target in &targets {
        s.save_view(view(n.id, target.clone(), targets[0].clone()), None, &a)
            .unwrap();
    }
    let invalid = point(&image, CaretTarget::ImageCaption { offset: 3 });
    assert!(s
        .save_view(view(n.id, invalid.clone(), invalid), None, &a)
        .is_err());
    let foreign = s.create(Kind::Note, None, "foreign", &a).unwrap();
    let other = s
        .append_content(foreign.id, Content::Text("x".into()), &a)
        .unwrap();
    assert!(s
        .save_view(view(n.id, text(&other, 0), targets[0].clone()), None, &a)
        .is_err());
    for b in [&graph, &diagram, &image, &code] {
        assert_eq!(&s.block(b.id, &a).unwrap(), b);
    }
}
#[test]
fn schema10_upgrade_keeps_legacy_view_payload_byte_for_byte_and_absent_bookmark_is_not_invented() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s
        .append_content(n.id, Content::Text("abc".into()), &a)
        .unwrap();
    let mut legacy = view(n.id, text(&b, 0), text(&b, 0));
    legacy.bookmark = None;
    s.save_view(legacy.clone(), None, &a).unwrap();
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    let before: String = db
        .query_row(
            "SELECT payload FROM view_settings WHERE id=?1",
            [legacy.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!before.contains("bookmark"));
    db.execute_batch("DROP TRIGGER IF EXISTS workspace_layer_identity; DROP TRIGGER IF EXISTS stroke_layer_identity; DROP TRIGGER IF EXISTS view_layer_identity; DROP TABLE canvas_layers; DROP TRIGGER view_workspace_identity; DROP TRIGGER stroke_workspace_identity; DROP TABLE workspace_state; DROP TABLE workspace_panes;")
        .unwrap();
    db.pragma_update(None, "user_version", 10).unwrap();
    drop(db);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 15);
    assert_eq!(s.view(legacy.id, &a).unwrap(), legacy);
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    let after: String = db
        .query_row(
            "SELECT payload FROM view_settings WHERE id=?1",
            [legacy.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(before, after);
}

#[test]
fn view_discovery_reopens_saved_positions_and_filters_fresh_block_protection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("discover.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "saved views", &a).unwrap();
    let b = s
        .append_content(n.id, Content::Text("abc".into()), &a)
        .unwrap();
    let private = s
        .save_view(view(n.id, text(&b, 0), text(&b, 2)), None, &a)
        .unwrap();
    let mut plain = private.clone();
    plain.id = Id::new();
    plain.bookmark = None;
    let plain = s.save_view(plain, None, &a).unwrap();
    let mut expected = vec![private.id, plain.id];
    expected.sort();
    drop(s);
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.views(n.id, None, 0, 512, &a).unwrap(), expected);
    assert_eq!(s.views(n.id, None, 1, 1, &a).unwrap(), expected[1..]);
    assert!(s.views(n.id, None, 0, 0, &a).unwrap().is_empty());
    let protected = access([Scope::Block(b.id)]);
    s.set_block_lock(b.id, 0, true, &protected).unwrap();
    assert_eq!(s.views(n.id, None, 0, 512, &a).unwrap(), vec![plain.id]);
    assert!(s.views(n.id, None, 1, 512, &a).unwrap().is_empty());
    assert_eq!(s.views(n.id, None, 0, 512, &protected).unwrap(), expected);
    assert!(matches!(s.restore_view(private.id, &a), Err(Error::Denied)));
    assert!(matches!(
        s.views(n.id, None, 0, 513, &a),
        Err(Error::ResourceLimit)
    ));
    let review = s.review_deletion(n.id, &protected).unwrap();
    s.trash(review, true, &protected).unwrap();
    assert!(s.views(n.id, None, 0, 512, &a).is_err());
}

#[test]
fn view_restore_and_discovery_reject_misbound_persistent_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("view-identity.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s
        .append_content(n.id, Content::Text("a".into()), &a)
        .unwrap();
    let v = s
        .save_view(view(n.id, text(&b, 0), text(&b, 1)), None, &a)
        .unwrap();
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE view_settings SET payload=json_set(payload,'$.id',?1) WHERE id=?2",
        [Id::new().to_string(), v.id.to_string()],
    )
    .unwrap();
    drop(db);
    let s = Store::open(&path).unwrap();
    assert!(matches!(s.restore_view(v.id, &a), Err(Error::Integrity)));
    assert!(matches!(
        s.views(n.id, None, 0, 512, &a),
        Err(Error::Integrity)
    ));
}

#[test]
fn view_discovery_separates_master_and_exercise_and_rechecks_app_lock() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let other = s.create(Kind::Note, None, "other", &a).unwrap();
    let b = s
        .append_content(n.id, Content::Text("a".into()), &a)
        .unwrap();
    let exercise = s.create_exercise(n.id, "part", &[b.id], &a).unwrap();
    let master = s
        .save_view(view(n.id, text(&b, 0), text(&b, 1)), None, &a)
        .unwrap();
    let mut child = master.clone();
    child.id = Id::new();
    child.exercise = Some(exercise.id);
    let child = s.save_view(child, None, &a).unwrap();
    assert_eq!(s.views(n.id, None, 0, 10, &a).unwrap(), vec![master.id]);
    assert_eq!(
        s.views(n.id, Some(exercise.id), 0, 10, &a).unwrap(),
        vec![child.id]
    );
    assert!(matches!(
        s.views(other.id, Some(exercise.id), 0, 10, &a),
        Err(Error::Invalid(_))
    ));
    assert!(s.views(other.id, None, 0, 10, &a).unwrap().is_empty());
    let app = access([Scope::App]);
    s.set_app_lock(true, &app).unwrap();
    assert!(matches!(s.views(n.id, None, 0, 10, &a), Err(Error::Denied)));
    assert_eq!(s.views(n.id, None, 0, 10, &app).unwrap(), vec![master.id]);
}
