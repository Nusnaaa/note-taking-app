mod common;
use common::*;
use stem_core::{
    backup::BackupOptions,
    crypto::{self, Credential},
    portable::{ExportProtection, NativeDocument},
    security::{Access, Scope},
    sources::Availability,
    store::*,
    Error, Id,
};
fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
fn view(s: &mut Store, name: &str) -> ViewSettings {
    let a = Access::public();
    let n = s.create(Kind::Note, None, name, &a).unwrap();
    s.save_view(
        ViewSettings {
            id: Id::new(),
            entity: n.id,
            exercise: None,
            camera: Camera {
                centre_x: 12.,
                centre_y: -3.,
                zoom: 2.,
                rotation_rad: 0.75,
            },
            pen_width_mm: 0.6,
            colour_rgba: [1, 2, 3, 255],
            finger_ink: true,
            anchor: None,
            bookmark: None,
            revision: 0,
        },
        None,
        &a,
    )
    .unwrap()
}
fn pane(v: Id, tool: WorkspaceTool) -> WorkspacePane {
    WorkspacePane {
        slot: Id::new(),
        weight: 1,
        target: PaneTarget::View { view: v, tool },
    }
}
fn update(panes: Vec<WorkspacePane>) -> WorkspaceUpdate {
    WorkspaceUpdate {
        active: panes.last().map(|p| p.slot),
        panes,
        axis: SplitAxis::Vertical,
    }
}
fn backup(s: &Store) -> Vec<u8> {
    let a = Access::public();
    let options = BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    };
    s.finish_backup(
        s.prepare_backup(options, &a).unwrap(),
        "Workspace recovery example passphrase",
        &mut TestAuth(true),
        true,
    )
    .unwrap()
    .bytes
}
#[test]
fn independent_views_pdf_position_tool_and_active_pane_reopen_without_touching_observations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workspace.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let first = view(&mut s, "first");
    let second = view(&mut s, "second");
    let original = stroke(0);
    let b = s
        .append_ink(first.entity, vec![original.clone()], &a)
        .unwrap();
    let exercise = s
        .create_exercise(first.entity, "linked", &[b.id], &a)
        .unwrap();
    let mut linked = first.clone();
    linked.id = Id::new();
    linked.exercise = Some(exercise.id);
    linked.camera.zoom = 4.;
    let linked = s.save_view(linked, None, &a).unwrap();
    let source = s
        .link_pdf(
            first.entity,
            "content://synthetic/workspace",
            &digest(b"pdf"),
            &a,
        )
        .unwrap();
    let mut pdf = source.view;
    pdf.page = 4;
    pdf.zoom = 3.;
    s.set_pdf_view(source.id, pdf.clone(), &a).unwrap();
    let mut panes = vec![
        pane(first.id, WorkspaceTool::Eraser),
        pane(second.id, WorkspaceTool::Selection),
        pane(linked.id, WorkspaceTool::Text),
        WorkspacePane {
            slot: Id::new(),
            weight: 3,
            target: PaneTarget::Pdf {
                source: source.id,
                tool: WorkspaceTool::Pen,
            },
        },
    ];
    panes[0].weight = 2;
    let state = update(panes);
    assert_eq!(s.save_workspace(state.clone(), 0, &a).unwrap(), 1);
    let before = s.restore_workspace(&a).unwrap();
    drop(s);
    let s = Store::open(&path).unwrap();
    let restored = s.restore_workspace(&a).unwrap();
    assert_eq!(before, restored);
    assert_eq!(restored.active, state.active);
    assert_eq!(restored.axis, SplitAxis::Vertical);
    let RestoredPaneState::View { view, tool } = &restored.panes[0].state else {
        panic!()
    };
    assert_eq!(view.settings, first);
    assert_eq!(*tool, WorkspaceTool::Eraser);
    let RestoredPaneState::Pdf {
        source,
        requires_platform_validation,
        ..
    } = &restored.panes[3].state
    else {
        panic!()
    };
    assert_eq!(source.view, pdf);
    assert!(*requires_platform_validation);
    assert_eq!(s.source_ink(b.id, &a).unwrap(), vec![original]);
}
#[test]
fn fresh_protection_redacts_targets_and_keep_can_resize_only_existing_private_slots() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let public = view(&mut s, "public");
    let private = view(&mut s, "private-title");
    let source = s
        .link_pdf(
            private.entity,
            "content://private-locator",
            &digest(b"private"),
            &a,
        )
        .unwrap();
    let first = pane(public.id, WorkspaceTool::Pen);
    let second = pane(private.id, WorkspaceTool::Eraser);
    let third = WorkspacePane {
        slot: Id::new(),
        weight: 1,
        target: PaneTarget::Pdf {
            source: source.id,
            tool: WorkspaceTool::Shape,
        },
    };
    let mut state = update(vec![first.clone(), second.clone(), third.clone()]);
    s.save_workspace(state.clone(), 0, &a).unwrap();
    let auth = access([Scope::Entity(private.entity)]);
    s.set_lock(
        private.entity,
        s.entity(private.entity, &a).unwrap().revision,
        true,
        &auth,
    )
    .unwrap();
    let restored = s.restore_workspace(&a).unwrap();
    assert!(matches!(
        restored.panes[0].state,
        RestoredPaneState::View { .. }
    ));
    assert_eq!(restored.panes[1].state, RestoredPaneState::Locked);
    assert_eq!(restored.panes[2].state, RestoredPaneState::Locked);
    let json = serde_json::to_string(&restored).unwrap();
    for secret in [
        private.id.to_string(),
        private.entity.to_string(),
        source.id.to_string(),
        "private-title".into(),
        "private-locator".into(),
        "Eraser".into(),
        "Shape".into(),
    ] {
        assert!(!json.contains(&secret));
    }
    assert!(matches!(
        s.save_workspace(state.clone(), 1, &a),
        Err(Error::Denied)
    ));
    state.panes[1].target = PaneTarget::Keep;
    state.panes[1].weight = 5;
    state.panes[2].target = PaneTarget::Keep;
    s.save_workspace(state.clone(), 1, &a).unwrap();
    let restored = s.restore_workspace(&auth).unwrap();
    assert!(matches!(
        restored.panes[1].state,
        RestoredPaneState::View {
            tool: WorkspaceTool::Eraser,
            ..
        }
    ));
    let mut forged = state.clone();
    forged.panes[1].slot = Id::new();
    assert!(matches!(
        s.save_workspace(forged, 2, &a),
        Err(Error::Conflict)
    ));
    state.panes.truncate(1);
    state.active = None;
    s.save_workspace(state, 2, &a).unwrap();
    assert_eq!(s.restore_workspace(&a).unwrap().panes.len(), 1);
    let app = access([Scope::App]);
    s.set_app_lock(true, &app).unwrap();
    assert!(matches!(s.restore_workspace(&a), Err(Error::Denied)));
    assert!(s.restore_workspace(&app).is_ok());
}
#[test]
fn optimistic_layout_checks_and_rollback_preserve_the_previous_workspace() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let v = view(&mut s, "n");
    let good = update(vec![pane(v.id, WorkspaceTool::Text)]);
    s.save_workspace(good.clone(), 0, &a).unwrap();
    let before = s.restore_workspace(&a).unwrap();
    assert!(matches!(
        s.save_workspace(good.clone(), 0, &a),
        Err(Error::Conflict)
    ));
    let mut bad = good.clone();
    bad.panes[0].weight = 0;
    assert!(s.save_workspace(bad, 1, &a).is_err());
    let mut bad = good.clone();
    bad.panes.push(bad.panes[0].clone());
    assert!(s.save_workspace(bad, 1, &a).is_err());
    let mut bad = good.clone();
    bad.active = Some(Id::new());
    assert!(s.save_workspace(bad, 1, &a).is_err());
    let mut bad = good.clone();
    bad.panes = vec![pane(v.id, WorkspaceTool::Pen); 9];
    assert!(s.save_workspace(bad, 1, &a).is_err());
    let mut alias = good.clone();
    alias.panes[0].slot = v.id;
    alias.active = Some(v.id);
    assert!(s.save_workspace(alias, 1, &a).is_err());
    let mut alias_view = v.clone();
    alias_view.id = good.panes[0].slot;
    assert!(s.save_view(alias_view, None, &a).is_err());
    assert_eq!(s.restore_workspace(&a).unwrap(), before);
    let mut alias_stroke = stroke(0);
    alias_stroke.id = good.panes[0].slot;
    assert!(s.append_ink(v.entity, vec![alias_stroke], &a).is_err());
    let empty = update(vec![]);
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.save_workspace(empty.clone(), 1, &a).is_err());
    s.inject_failure(FailurePoint::None);
    assert_eq!(s.restore_workspace(&a).unwrap(), before);
    s.save_workspace(empty, 1, &a).unwrap();
    let empty = s.restore_workspace(&a).unwrap();
    assert!(empty.panes.is_empty());
    assert_eq!(empty.active, None);
}
#[test]
fn trash_restore_and_purge_preserve_slots_but_retire_private_target_references_atomically() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let v = view(&mut s, "retire");
    let source = s
        .link_pdf(v.entity, "content://retire", &digest(b"x"), &a)
        .unwrap();
    let mut state = update(vec![pane(v.id, WorkspaceTool::Eraser)]);
    state.panes.push(WorkspacePane {
        slot: Id::new(),
        weight: 1,
        target: PaneTarget::Pdf {
            source: source.id,
            tool: WorkspaceTool::Shape,
        },
    });
    s.save_workspace(state.clone(), 0, &a).unwrap();
    let review = s.review_deletion(v.entity, &a).unwrap();
    s.trash(review, true, &a).unwrap();
    assert!(s
        .restore_workspace(&a)
        .unwrap()
        .panes
        .iter()
        .all(|p| p.state == RestoredPaneState::Unavailable));
    s.restore(v.entity, &a).unwrap();
    assert!(matches!(
        s.restore_workspace(&a).unwrap().panes[0].state,
        RestoredPaneState::View { .. }
    ));
    let review = s.review_deletion(v.entity, &a).unwrap();
    s.trash(review, true, &a).unwrap();
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.purge(v.entity, &a).is_err());
    s.inject_failure(FailurePoint::None);
    assert_eq!(s.restore_workspace(&a).unwrap().revision, 1);
    s.purge(v.entity, &a).unwrap();
    let restored = s.restore_workspace(&a).unwrap();
    assert_eq!(restored.revision, 3);
    assert_eq!(restored.active, state.active);
    assert!(restored
        .panes
        .iter()
        .all(|p| p.state == RestoredPaneState::Unavailable));
    assert!(matches!(
        s.save_workspace(state, 1, &a),
        Err(Error::Conflict)
    ));
    let bytes = backup(&s);
    let plain = crypto::decrypt(
        &bytes,
        Credential::Password("Workspace recovery example passphrase"),
    )
    .unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    let rows = &doc["tables"]["workspace_panes"];
    let json = rows.to_string();
    assert!(!json.contains(&v.id.to_string()));
    assert!(!json.contains(&source.id.to_string()));
    assert!(!json.contains("Eraser"));
    assert!(!json.contains("Shape"));
}
#[test]
fn encrypted_workspace_restore_revokes_pdf_readiness_and_rejects_malformed_or_downgraded_rows() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let v = view(&mut s, "n");
    let source = s
        .link_pdf(v.entity, "content://backup", &digest(b"x"), &a)
        .unwrap();
    let mut state = update(vec![pane(v.id, WorkspaceTool::Eraser)]);
    state.panes.push(WorkspacePane {
        slot: Id::new(),
        weight: 1,
        target: PaneTarget::Pdf {
            source: source.id,
            tool: WorkspaceTool::Pen,
        },
    });
    s.save_workspace(state, 0, &a).unwrap();
    let bytes = backup(&s);
    let password = "Workspace recovery example passphrase";
    let restored = Store::restore_backup_password(&bytes, password).unwrap();
    let workspace = restored.restore_workspace(&a).unwrap();
    assert_eq!(workspace.revision, 1);
    let RestoredPaneState::Pdf {
        source,
        requires_platform_validation,
        ..
    } = &workspace.panes[1].state
    else {
        panic!()
    };
    assert_eq!(source.availability, Availability::Missing);
    assert!(*requires_platform_validation);
    let plain = crypto::decrypt(&bytes, Credential::Password(password)).unwrap();
    let original: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    let mut malformed = original.clone();
    malformed["tables"]["workspace_panes"][0][1]["value"] = 7.into();
    let bad = crypto::encrypt(&serde_json::to_vec(&malformed).unwrap(), password).unwrap();
    assert!(Store::restore_backup_password(&bad.bytes, password).is_err());
    let mut legacy = original;
    legacy["schema"] = 11.into();
    let bad = crypto::encrypt(&serde_json::to_vec(&legacy).unwrap(), password).unwrap();
    assert!(Store::restore_backup_password(&bad.bytes, password).is_err());
    legacy["tables"]
        .as_object_mut()
        .unwrap()
        .remove("workspace_panes");
    legacy["tables"]
        .as_object_mut()
        .unwrap()
        .remove("workspace_state");
    let bytes = crypto::encrypt(&serde_json::to_vec(&legacy).unwrap(), password).unwrap();
    let restored = Store::restore_backup_password(&bytes.bytes, password).unwrap();
    assert_eq!(restored.schema_version().unwrap(), 13);
    assert_eq!(restored.restore_workspace(&a).unwrap().revision, 0);
    assert!(restored.restore_workspace(&a).unwrap().panes.is_empty());
    assert_eq!(restored.view(v.id, &a).unwrap(), v);
}
#[test]
fn native_copy_preserves_selected_views_without_activating_a_workspace_in_the_destination() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let v = view(&mut s, "n");
    s.save_workspace(update(vec![pane(v.id, WorkspaceTool::Eraser)]), 0, &a)
        .unwrap();
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
    assert_eq!(doc.attachments.views.len(), 1);
    assert!(doc.omissions.iter().any(|s| s.contains("workspace")));
    let mut target = Store::memory().unwrap();
    target.import_native(&bytes, &a).unwrap();
    assert!(target.restore_workspace(&a).unwrap().panes.is_empty());
    let existing = view(&mut target, "already open");
    target
        .save_workspace(update(vec![pane(existing.id, WorkspaceTool::Text)]), 0, &a)
        .unwrap();
    let before = target.restore_workspace(&a).unwrap();
    target.import_native(&bytes, &a).unwrap();
    assert_eq!(target.restore_workspace(&a).unwrap(), before);
    assert_eq!(s.restore_workspace(&a).unwrap().panes.len(), 1);
}
#[test]
fn schema11_upgrade_creates_an_empty_workspace_without_rewriting_saved_view_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let v = view(&mut s, "legacy");
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    let before: String = db
        .query_row(
            "SELECT payload FROM view_settings WHERE id=?1",
            [v.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    db.execute_batch("DROP TRIGGER view_workspace_identity; DROP TRIGGER stroke_workspace_identity; DROP TABLE workspace_state; DROP TABLE workspace_panes;")
        .unwrap();
    db.pragma_update(None, "user_version", 11).unwrap();
    drop(db);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 13);
    assert!(s.restore_workspace(&a).unwrap().panes.is_empty());
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
    assert_eq!(before, after);
}
