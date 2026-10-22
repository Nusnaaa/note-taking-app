mod common;
use common::*;
use stem_core::{
    content::Content,
    ink::Transform,
    security::{Access, Scope},
    store::{FailurePoint, Kind, Store},
    Error,
};

#[test]
fn undo_redo_survives_reopen_and_never_rewinds_revisions() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("edit.db");
    let a = Access::public();
    let mut s = Store::open(&p).unwrap();
    let e = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s
        .append_content(e.id, Content::Text("before".into()), &a)
        .unwrap();
    s.edit_content(b.id, b.revision, Content::Text("after".into()), &a)
        .unwrap();
    s.undo(e.id, &a).unwrap();
    let old = s.block(b.id, &a).unwrap();
    assert_eq!(old.content.literal(), "before");
    assert_eq!(old.revision, 2);
    drop(s);
    let mut s = Store::open(&p).unwrap();
    s.redo(e.id, &a).unwrap();
    let b = s.block(b.id, &a).unwrap();
    assert_eq!(b.content.literal(), "after");
    assert_eq!(b.revision, 3);
}
#[test]
fn undo_creation_preserves_original_observations_and_redo_restores_visibility() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let e = s.create(Kind::Note, None, "n", &a).unwrap();
    let ink = stroke(0);
    let b = s.append_ink(e.id, vec![ink.clone()], &a).unwrap();
    s.undo(e.id, &a).unwrap();
    assert!(!s.block(b.id, &a).unwrap().visible);
    assert_eq!(s.source_ink(b.id, &a).unwrap(), vec![ink]);
    s.redo(e.id, &a).unwrap();
    assert!(s.block(b.id, &a).unwrap().visible);
}
#[test]
fn editing_after_undo_discards_redo_branch_but_keeps_source_history() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let e = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s
        .append_content(e.id, Content::Text("a".into()), &a)
        .unwrap();
    s.edit_content(b.id, 0, Content::Text("b".into()), &a)
        .unwrap();
    s.undo(e.id, &a).unwrap();
    s.edit_content(b.id, 2, Content::Text("c".into()), &a)
        .unwrap();
    assert!(matches!(s.redo(e.id, &a), Err(Error::Unavailable)));
    assert!(s
        .history(b.id, &a)
        .unwrap()
        .iter()
        .any(|h| h.block.content.literal() == "b"));
}
#[test]
fn lasso_action_undo_is_atomic_and_stale_or_interrupted_actions_leave_no_history() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let e = s.create(Kind::Canvas, None, "n", &a).unwrap();
    let b = s
        .append_content(e.id, Content::Text("a".into()), &a)
        .unwrap();
    let c = s
        .append_content(e.id, Content::Text("b".into()), &a)
        .unwrap();
    let t = Transform {
        tx: 4.,
        ..Default::default()
    };
    let before = s.edit_history_depth(e.id, &a).unwrap();
    assert!(s
        .transform_selection(&[(b.id, 0), (c.id, 9)], t, &a)
        .is_err());
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s
        .transform_selection(&[(b.id, 0), (c.id, 0)], t, &a)
        .is_err());
    assert_eq!(s.edit_history_depth(e.id, &a).unwrap(), before);
    assert_eq!(s.block(b.id, &a).unwrap().transform, Transform::default());
    s.transform_selection(&[(b.id, 0), (c.id, 0)], t, &a)
        .unwrap();
    s.undo(e.id, &a).unwrap();
    assert_eq!(s.block(b.id, &a).unwrap().transform, Transform::default());
    assert_eq!(s.block(c.id, &a).unwrap().transform, Transform::default());
}
#[test]
fn undo_requires_current_locks_and_cannot_remove_protection() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let e = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s
        .append_content(e.id, Content::Text("private".into()), &a)
        .unwrap();
    let granted = access([Scope::Block(b.id)]);
    s.set_block_lock(b.id, 0, true, &granted).unwrap();
    assert!(matches!(s.undo(e.id, &a), Err(Error::Denied)));
    s.undo(e.id, &granted).unwrap();
    assert!(s.block(b.id, &granted).unwrap().locked);
    assert!(matches!(s.block(b.id, &a), Err(Error::Denied)));
}
#[test]
fn whole_backup_restores_undo_and_redo_without_touching_live_library() {
    use stem_core::backup::BackupOptions;
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let e = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s
        .append_content(e.id, Content::Text("a".into()), &a)
        .unwrap();
    s.edit_content(b.id, 0, Content::Text("b".into()), &a)
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
    let encrypted = s
        .finish_backup(plan, "test strong passphrase", &mut TestAuth(true), true)
        .unwrap();
    let mut restored =
        Store::restore_backup_password(&encrypted.bytes, "test strong passphrase").unwrap();
    restored.undo(e.id, &a).unwrap();
    assert_eq!(restored.block(b.id, &a).unwrap().content.literal(), "a");
    assert_eq!(s.block(b.id, &a).unwrap().content.literal(), "b");
}

#[test]
fn pruning_without_protected_block_access_keeps_its_undo_history() {
    use std::time::Duration;
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let e = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s
        .append_content(e.id, Content::Text("private".into()), &a)
        .unwrap();
    let granted = access([Scope::Block(b.id)]);
    s.set_block_lock(b.id, 0, true, &granted).unwrap();
    let depth = s.edit_history_depth(e.id, &a).unwrap();
    s.record_active_use(Duration::from_secs(3600 * 100), &a)
        .unwrap();
    s.prune_history(&a).unwrap();
    assert_eq!(s.edit_history_depth(e.id, &a).unwrap(), depth);
}
