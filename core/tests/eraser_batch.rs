mod common;
use common::*;
use stem_core::{
    ink::{EraserGesture, EraserGestureTarget},
    security::{Access, Scope},
    store::{FailurePoint, Kind, Store},
    Error,
};
fn target(block: &stem_core::store::Block) -> EraserGestureTarget {
    EraserGestureTarget {
        block: block.id,
        revision: block.revision,
        gesture: EraserGesture {
            coordinate_space: "synthetic-local-units".into(),
            path: vec![[0.1, 0.]],
            radius: 0.5,
        },
    }
}
#[test]
fn several_blocks_erase_and_restore_as_one_operation_including_shared_exercise_views() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "compound", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let c = s.append_ink(n.id, vec![stroke(1)], &a).unwrap();
    let exercise = s
        .create_exercise(n.id, "shared", &[b.id, c.id], &a)
        .unwrap();
    let before = s.edit_history_depth(n.id, &a).unwrap();
    let results = s.erase_gestures(&[target(&b), target(&c)], &a).unwrap();
    assert!(results.iter().all(|b| b.visible_strokes.is_empty()));
    assert_eq!(s.edit_history_depth(n.id, &a).unwrap(), (before.0 + 1, 0));
    s.undo(n.id, &a).unwrap();
    assert_eq!(
        s.block(b.id, &a).unwrap().visible_strokes,
        b.visible_strokes
    );
    assert_eq!(
        s.block(c.id, &a).unwrap().visible_strokes,
        c.visible_strokes
    );
    assert!(s
        .exercise_blocks(exercise.id, 0, 10, &a)
        .unwrap()
        .iter()
        .all(|b| !b.visible_strokes.is_empty()));
    s.redo(n.id, &a).unwrap();
    assert!(s.block(b.id, &a).unwrap().visible_strokes.is_empty());
    assert!(s.block(c.id, &a).unwrap().visible_strokes.is_empty());
}
#[test]
fn late_stale_denied_cross_master_duplicate_and_failure_never_publish_a_prefix() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "one", &a).unwrap();
    let other = s.create(Kind::Note, None, "two", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let c = s.append_ink(n.id, vec![stroke(1)], &a).unwrap();
    let d = s.append_ink(other.id, vec![stroke(2)], &a).unwrap();
    let mut stale = target(&c);
    stale.revision += 1;
    assert!(matches!(
        s.erase_gestures(&[target(&b), stale], &a),
        Err(Error::Conflict)
    ));
    assert!(s.erase_gestures(&[target(&b), target(&d)], &a).is_err());
    assert!(s.erase_gestures(&[target(&b), target(&b)], &a).is_err());
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.erase_gestures(&[target(&b), target(&c)], &a).is_err());
    assert_eq!(s.block(b.id, &a).unwrap(), b);
    assert_eq!(s.block(c.id, &a).unwrap(), c);
    let c = s
        .set_block_lock(c.id, c.revision, true, &access([Scope::Block(c.id)]))
        .unwrap();
    assert!(matches!(
        s.erase_gestures(&[target(&b), target(&c)], &a),
        Err(Error::Denied)
    ));
    assert_eq!(s.block(b.id, &a).unwrap(), b);
}
#[test]
fn all_misses_preserve_generation_history_and_each_revision_while_mixed_hits_leave_misses_unchanged(
) {
    let a = Access::public();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("library.db");
    let mut s = Store::open(&path).unwrap();
    let inspector =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let generation = || {
        inspector
            .query_row(
                "SELECT value FROM settings WHERE key='generation'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
    };
    let n = s.create(Kind::Note, None, "miss", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let c = s.append_ink(n.id, vec![stroke(1)], &a).unwrap();
    let mut miss = target(&b);
    miss.gesture.path = vec![[100., 100.]];
    let mut other = target(&c);
    other.gesture.path = miss.gesture.path.clone();
    let before = s.edit_history_depth(n.id, &a).unwrap();
    let before_generation = generation();
    let result = s.erase_gestures(&[miss.clone(), other], &a).unwrap();
    assert_eq!(result, vec![b.clone(), c.clone()]);
    assert_eq!(s.edit_history_depth(n.id, &a).unwrap(), before);
    assert_eq!(generation(), before_generation);
    let result = s.erase_gestures(&[miss, target(&c)], &a).unwrap();
    assert_eq!(result[0], b);
    assert!(result[1].visible_strokes.is_empty());
    s.undo(n.id, &a).unwrap();
    assert_eq!(
        s.block(c.id, &a).unwrap().visible_strokes,
        c.visible_strokes
    );
}
#[test]
fn pair_budget_is_shared_across_the_compound_operation_not_reset_per_block() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "budget", &a).unwrap();
    let mut raw = stroke(0);
    raw.points = vec![raw.points[0].clone(); 1000];
    let b = s.append_ink(n.id, vec![raw.clone()], &a).unwrap();
    raw.id = stem_core::Id::new();
    raw.capture_session = stem_core::Id::new();
    let c = s.append_ink(n.id, vec![raw], &a).unwrap();
    let mut one = target(&b);
    one.gesture.path = vec![[0.1, 0.]; 600];
    let mut two = target(&c);
    two.gesture.path = one.gesture.path.clone();
    assert!(matches!(
        s.erase_gestures(&[one, two], &a),
        Err(Error::ResourceLimit)
    ));
    assert_eq!(s.block(b.id, &a).unwrap(), b);
    assert_eq!(s.block(c.id, &a).unwrap(), c);
}
