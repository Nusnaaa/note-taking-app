mod common;
use common::*;
use std::time::Duration;
use stem_core::{
    content::{Content, MathForm, MathNode},
    ink::Transform,
    security::{Access, Scope},
    store::{Display, FailurePoint, Kind, Store, WritingMode},
    Error, Id,
};

#[test]
fn source_ink_is_lossless_after_reopen_transform_erase_and_history_retirement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ink.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let original = stroke(0);
    let b = s.append_ink(n.id, vec![original.clone()], &a).unwrap();
    let b = s
        .transform_block(
            b.id,
            0,
            Transform {
                tx: 100.,
                ..Transform::default()
            },
            &a,
        )
        .unwrap();
    let b = s
        .edit_content(b.id, b.revision, Content::Numeric("001".into()), &a)
        .unwrap();
    let b = s
        .erase_strokes(b.id, b.revision, &b.source_ids, &a)
        .unwrap();
    s.record_active_use(Duration::from_secs(73 * 3600), &a)
        .unwrap();
    s.prune_history(&a).unwrap();
    drop(s);
    let s = Store::open(path).unwrap();
    let restored = s.source_ink(b.id, &a).unwrap();
    assert_eq!(
        serde_json::to_vec(&restored).unwrap(),
        serde_json::to_vec(&vec![original]).unwrap()
    );
    assert!(s.block(b.id, &a).unwrap().visible_strokes.is_empty());
    assert_eq!(
        s.block(b.id, &a).unwrap().content,
        Content::Numeric("001".into())
    );
}
#[test]
fn bad_samples_duplicate_ids_and_unordered_strokes_are_rejected() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let mut invalid = stroke(0);
    invalid.points[1].x = f64::NAN;
    assert!(s.append_ink(n.id, vec![invalid], &a).is_err());
    let mut invalid = stroke(0);
    invalid.points[1].time_us = 1;
    assert!(s.append_ink(n.id, vec![invalid], &a).is_err());
    let first = stroke(3);
    let mut next = stroke(2);
    next.capture_session = first.capture_session;
    assert!(s.append_ink(n.id, vec![first.clone(), next], &a).is_err());
    assert!(s.append_ink(n.id, vec![first.clone(), first], &a).is_err());
    assert!(s.blocks(n.id, 0, 100, &a).unwrap().is_empty());
}
#[test]
fn interrupted_capture_rolls_back_block_strokes_and_document_revision() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.append_ink(n.id, vec![stroke(0)], &a).is_err());
    assert!(s.blocks(n.id, 0, 100, &a).unwrap().is_empty());
    assert_eq!(s.entity(n.id, &a).unwrap().revision, 0);
    s.check_integrity().unwrap();
}
#[test]
fn exact_code_and_incorrect_math_survive_edits_and_restore_as_new_revision() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let code = "\tif x == 001:\r\n  deliberate_syntax_error(\r\n";
    let b = s
        .append_content(
            n.id,
            Content::Code {
                text: code.into(),
                language: None,
            },
            &a,
        )
        .unwrap();
    let b = s
        .edit_content(b.id, 0, Content::Maths(MathNode::token("1+1=3")), &a)
        .unwrap();
    let restored = s.restore_block_revision(b.id, b.revision, 0, &a).unwrap();
    assert_eq!(restored.revision, 2);
    assert_eq!(restored.content.literal(), code);
    assert_eq!(s.history(b.id, &a).unwrap().len(), 3);
}
#[test]
fn math_subtree_edit_preserves_incomplete_fence_and_neighbours() {
    let token = MathNode::token("x");
    let id = token.id;
    let mut math = MathNode {
        id: Id::new(),
        form: MathForm::Fence {
            open: "(".into(),
            body: Box::new(token),
            close: None,
        },
    };
    math.replace(id, MathNode::token("α")).unwrap();
    assert_eq!(math.literal(), "α");
    assert!(matches!(math.form, MathForm::Fence { close: None, .. }));
    let duplicate = MathNode::token("x");
    let bad = MathNode {
        id: Id::new(),
        form: MathForm::Row(vec![duplicate.clone(), duplicate]),
    };
    assert!(bad.validate().is_err());
}
#[test]
fn stale_edits_and_view_toggles_never_mutate_source() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let original = s.source_ink(b.id, &a).unwrap();
    let b = s
        .edit_content(b.id, 0, Content::Text("corrected".into()), &a)
        .unwrap();
    assert!(matches!(
        s.edit_content(b.id, 0, Content::Text("stale".into()), &a),
        Err(Error::Conflict)
    ));
    s.set_display(b.id, b.revision, Display::Original, &a)
        .unwrap();
    assert_eq!(s.source_ink(b.id, &a).unwrap(), original);
    assert_eq!(s.block(b.id, &a).unwrap().content.literal(), "corrected");
}
#[test]
fn active_use_retirement_keeps_checkpoint_and_originals() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    s.edit_content(b.id, 0, Content::Text("first".into()), &a)
        .unwrap();
    assert_eq!(s.history_hours().unwrap(), 72.);
    assert_eq!(s.prune_history(&a).unwrap(), 0);
    s.record_active_use(Duration::from_secs(73 * 3600), &a)
        .unwrap();
    s.edit_content(b.id, 1, Content::Text("second".into()), &a)
        .unwrap();
    assert_eq!(s.prune_history(&a).unwrap(), 1);
    assert_eq!(s.history(b.id, &a).unwrap()[0].revision, 1);
    assert_eq!(s.source_ink(b.id, &a).unwrap().len(), 1);
    assert!(s.set_history_hours(169., &a).is_err());
    assert!(s.set_history_hours(0., &a).is_err());
    s.set_history_hours(0.5, &a).unwrap();
}
#[test]
fn search_matches_literal_accepted_content_never_raw_or_locked_data() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "Study", &a).unwrap();
    let ink = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let b = s
        .append_content(n.id, Content::Maths(MathNode::token("x+x")), &a)
        .unwrap();
    assert_eq!(s.search("x+x", &a).unwrap(), vec![(n.id, Some(b.id))]);
    assert!(s.search("2x", &a).unwrap().is_empty());
    assert!(s
        .search(&ink.source_ids[0].to_string(), &a)
        .unwrap()
        .is_empty());
    let auth = access([Scope::Block(b.id)]);
    s.set_block_lock(b.id, 0, true, &auth).unwrap();
    assert!(s.search("x+x", &a).unwrap().is_empty());
    assert!(s.block(b.id, &a).is_err());
    assert!(s.history(b.id, &a).is_err());
    assert_eq!(s.blocks(n.id, 0, 100, &a).unwrap().len(), 1);
}
#[test]
fn mode_defaults_and_inheritance_do_not_convert_existing_ink() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let book = s.create(Kind::Notebook, None, "b", &a).unwrap();
    let n = s.create(Kind::Note, Some(book.id), "n", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    assert_eq!(s.mode(n.id, &a).unwrap(), WritingMode::Manual);
    s.set_mode(book.id, WritingMode::InkOnly, &a).unwrap();
    assert_eq!(s.mode(n.id, &a).unwrap(), WritingMode::InkOnly);
    s.set_global_mode(WritingMode::Assisted, &a).unwrap();
    assert_eq!(s.mode(n.id, &a).unwrap(), WritingMode::InkOnly);
    assert_eq!(s.block(b.id, &a).unwrap(), b);
}
#[test]
fn restore_never_removes_newer_content_lock() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s
        .append_content(n.id, Content::Text("secret".into()), &a)
        .unwrap();
    let auth = access([Scope::Block(b.id)]);
    let locked = s.set_block_lock(b.id, 0, true, &auth).unwrap();
    s.restore_block_revision(b.id, locked.revision, 0, &auth)
        .unwrap();
    assert!(s.block(b.id, &a).is_err());
}

#[test]
fn unauthorised_history_retirement_skips_locked_content() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s
        .append_content(n.id, Content::Text("first".into()), &a)
        .unwrap();
    s.edit_content(b.id, 0, Content::Text("second".into()), &a)
        .unwrap();
    let auth = access([Scope::Block(b.id)]);
    s.set_block_lock(b.id, 1, true, &auth).unwrap();
    s.record_active_use(Duration::from_secs(73 * 3600), &a)
        .unwrap();
    assert_eq!(s.prune_history(&a).unwrap(), 0);
    assert_eq!(s.history(b.id, &auth).unwrap().len(), 3);
    assert_eq!(s.prune_history(&auth).unwrap(), 2);
}
