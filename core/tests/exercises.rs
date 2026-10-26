mod common;
use common::*;
use stem_core::{
    content::Content,
    portable::{ExportProtection, NativeDocument},
    security::{Access, Scope},
    store::{Exercise, FailurePoint, Kind, Order, SaveState, Store, WritingMode, WritingTarget},
    Error, Id,
};

fn text(s: &mut Store, master: Id, value: &str) -> stem_core::store::Block {
    s.append_content(master, Content::Text(value.into()), &Access::public())
        .unwrap()
}
fn ids(s: &Store, e: Id, a: &Access) -> Vec<Id> {
    s.exercise_blocks(e, 0, 512, a)
        .unwrap()
        .iter()
        .map(|b| b.id)
        .collect()
}
fn extract(s: &mut Store, master: Id) -> Vec<Exercise> {
    let p = s.plan_extraction(master, &Access::public()).unwrap();
    s.accept_extraction(p, true, &Access::public()).unwrap()
}
fn export(s: &Store, master: Id, a: &Access) -> Vec<u8> {
    s.finish_native_export(
        s.prepare_native_export(&[master], true, a).unwrap(),
        ExportProtection::Plaintext {
            acknowledge_protected: true,
        },
        &mut TestAuth(true),
        true,
    )
    .unwrap()
    .bytes
}

#[test]
fn extracting_a_protected_heading_does_not_disclose_its_title() {
    let mut s = Store::memory().unwrap();
    let public = Access::public();
    let n = s.create(Kind::Note, None, "master", &public).unwrap();
    let heading = text(&mut s, n.id, "Exercise confidential title");
    let heading = s.set_heading(heading.id, 0, true, &public).unwrap();
    let authorised = access([Scope::Block(heading.id)]);
    s.set_block_lock(heading.id, heading.revision, true, &authorised)
        .unwrap();
    text(&mut s, n.id, "new work");
    assert!(s
        .plan_extraction(n.id, &public)
        .unwrap()
        .candidates()
        .is_empty());
    let plan = s.plan_extraction(n.id, &authorised).unwrap();
    let e = s
        .accept_extraction(plan, true, &authorised)
        .unwrap()
        .remove(0);
    assert!(e.locked);
    assert!(s.exercises(n.id, true, &public).unwrap().is_empty());
    assert!(s.search("confidential", &public).unwrap().is_empty());
    assert!(matches!(s.exercise(e.id, &public), Err(Error::Denied)));
    assert!(matches!(
        s.copy_exercise_independent(e.id, &public),
        Err(Error::Denied)
    ));
    let authorised = access([Scope::Block(heading.id), Scope::Exercise(e.id)]);
    assert_eq!(
        s.exercise(e.id, &authorised).unwrap().title,
        "Exercise confidential title"
    );
    assert_eq!(
        s.exercise_blocks(e.id, 0, 10, &authorised).unwrap().len(),
        2
    );
}

#[test]
fn duplicate_titles_keep_distinct_identity_and_metadata_failures_share_status() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let first = text(&mut s, n.id, "Exercise 1");
    s.set_heading(first.id, 0, true, &a).unwrap();
    let second = text(&mut s, n.id, "Exercise 1");
    s.set_heading(second.id, 0, true, &a).unwrap();
    let es = extract(&mut s, n.id);
    assert_eq!(es.len(), 2);
    assert_ne!(es[0].id, es[1].id);
    assert_eq!(es[0].title, es[1].title);
    assert_eq!(ids(&s, es[0].id, &a), vec![first.id]);
    assert_eq!(ids(&s, es[1].id, &a), vec![second.id]);
    let revision = s.entity(n.id, &a).unwrap().revision;
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.rename(n.id, revision, "unsaved title", &a).is_err());
    assert_eq!(s.entity(n.id, &a).unwrap().title, "master");
    for e in &es {
        assert_eq!(
            s.exercise_save_state(e.id, &a).unwrap(),
            SaveState::NeedsAttention
        );
    }
    s.rename(n.id, revision, "saved title", &a).unwrap();
    assert_eq!(
        s.exercise_save_state(es[0].id, &a).unwrap(),
        SaveState::SavedLocally
    );
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.set_mode(n.id, WritingMode::InkOnly, &a).is_err());
    assert_eq!(
        s.exercise_save_state(es[0].id, &a).unwrap(),
        SaveState::NeedsAttention
    );
    assert_ne!(s.exercise_mode(es[0].id, &a).unwrap(), WritingMode::InkOnly);
}

#[test]
fn only_confirmed_matching_headings_extract_previously_unassigned_work() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let intro = text(&mut s, n.id, "intro with Exercise in prose");
    text(&mut s, n.id, "Exercise incidental unconfirmed line");
    let h = text(&mut s, n.id, "Exercise 1");
    s.set_heading(h.id, 0, true, &a).unwrap();
    let b = text(&mut s, n.id, "working");
    let stop = text(&mut s, n.id, "Discussion");
    s.set_heading(stop.id, 0, true, &a).unwrap();
    text(&mut s, n.id, "unassigned after other heading");
    let p = s.plan_extraction(n.id, &a).unwrap();
    assert_eq!(p.candidates().len(), 1);
    assert_eq!(p.candidates()[0].blocks, vec![h.id, b.id]);
    let e = s.accept_extraction(p, true, &a).unwrap().remove(0);
    assert_eq!(ids(&s, e.id, &a), vec![h.id, b.id]);
    assert!(!ids(&s, e.id, &a).contains(&intro.id));
    assert!(s.plan_extraction(n.id, &a).unwrap().candidates().is_empty());
    assert_eq!(s.blocks(n.id, 0, 512, &a).unwrap().len(), 6);
}
#[test]
fn existing_membership_survives_heading_confirmation_removal_rename_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("exercises.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let h = text(&mut s, n.id, "Exercise 1");
    s.set_heading(h.id, 0, true, &a).unwrap();
    let middle = text(&mut s, n.id, "Exercise 2");
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let e = extract(&mut s, n.id).remove(0);
    let expected = ids(&s, e.id, &a);
    s.set_heading(middle.id, 0, true, &a).unwrap();
    assert!(s.plan_extraction(n.id, &a).unwrap().candidates().is_empty());
    let h = s
        .edit_content(
            h.id,
            1,
            Content::Text("Not an extraction keyword".into()),
            &a,
        )
        .unwrap();
    s.set_heading(h.id, h.revision, false, &a).unwrap();
    let e = s
        .rename_exercise(e.id, e.revision, "New visible title", &a)
        .unwrap();
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(ids(&s, e.id, &a), expected);
    assert_eq!(s.exercise(e.id, &a).unwrap(), e);
    assert_eq!(s.source_ink(b.id, &a).unwrap().len(), 1);
}
#[test]
fn shared_edits_use_one_revision_and_stale_second_connection_cannot_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let b = text(&mut s, n.id, "before");
    let e = s.create_exercise(n.id, "exercise", &[b.id], &a).unwrap();
    let mut other = Store::open(&path).unwrap();
    let seen = other.exercise_blocks(e.id, 0, 50, &a).unwrap().remove(0);
    let edited = s
        .edit_content(
            seen.id,
            seen.revision,
            Content::Text("from linked view".into()),
            &a,
        )
        .unwrap();
    assert_eq!(other.block(b.id, &a).unwrap(), edited);
    assert_eq!(s.blocks(n.id, 0, 50, &a).unwrap().len(), 1);
    assert!(matches!(
        other.edit_content(b.id, seen.revision, Content::Text("stale".into()), &a),
        Err(Error::Conflict)
    ));
    assert_eq!(s.history(b.id, &a).unwrap().len(), 2);
    s.restore_block_revision(b.id, edited.revision, 0, &a)
        .unwrap();
    assert_eq!(
        s.exercise_blocks(e.id, 0, 50, &a).unwrap()[0]
            .content
            .literal(),
        "before"
    );
}
#[test]
fn linked_append_is_atomic_and_keeps_master_order_and_shared_save_state() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let independent = s.create(Kind::Note, None, "independent", &a).unwrap();
    let one = text(&mut s, n.id, "one");
    let two = text(&mut s, n.id, "next exercise");
    let e = s.create_exercise(n.id, "exercise", &[one.id], &a).unwrap();
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s
        .append_ink_to(WritingTarget::Exercise(e.id), vec![stroke(0)], &a)
        .is_err());
    assert_eq!(
        s.exercise_save_state(e.id, &a).unwrap(),
        SaveState::NeedsAttention
    );
    assert_eq!(
        s.document_save_state(n.id, &a).unwrap(),
        SaveState::NeedsAttention
    );
    assert_eq!(
        s.document_save_state(independent.id, &a).unwrap(),
        SaveState::SavedLocally
    );
    assert_eq!(
        s.blocks(n.id, 0, 50, &a)
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>(),
        vec![one.id, two.id]
    );
    let added = s
        .append_ink_to(WritingTarget::Exercise(e.id), vec![stroke(1)], &a)
        .unwrap();
    assert_eq!(
        s.blocks(n.id, 0, 50, &a)
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>(),
        vec![one.id, added.id, two.id]
    );
    assert_eq!(ids(&s, e.id, &a), vec![one.id, added.id]);
    assert_eq!(
        s.exercise_save_state(e.id, &a).unwrap(),
        SaveState::SavedLocally
    );
}
#[test]
fn removing_view_preserves_master_ink_membership_and_protection() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let e = s
        .create_exercise(n.id, "private exercise", &[b.id], &a)
        .unwrap();
    let auth = access([Scope::Exercise(e.id)]);
    let e = s.set_exercise_lock(e.id, 0, true, &auth).unwrap();
    let e = s
        .set_exercise_visible(e.id, e.revision, false, &auth)
        .unwrap();
    assert!(s.exercises(n.id, false, &auth).unwrap().is_empty());
    assert!(s.exercise_blocks(e.id, 0, 50, &auth).is_err());
    assert!(s.blocks(n.id, 0, 50, &a).unwrap().is_empty());
    assert!(s.source_ink(b.id, &a).is_err());
    assert_eq!(s.source_ink(b.id, &auth).unwrap().len(), 1);
    s.set_exercise_visible(e.id, e.revision, true, &auth)
        .unwrap();
    assert_eq!(ids(&s, e.id, &auth), vec![b.id]);
}
#[test]
fn exercise_locks_cover_master_search_history_inference_links_export_and_deletion() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "public master", &a).unwrap();
    let target = s.create(Kind::Note, None, "target", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    s.edit_content(b.id, 0, Content::Text("private token".into()), &a)
        .unwrap();
    let link = s.add_link(n.id, target.id, Some(b.id), &a).unwrap();
    let e = s
        .create_exercise(n.id, "private exercise", &[b.id], &a)
        .unwrap();
    let auth = access([Scope::Exercise(e.id)]);
    s.set_exercise_lock(e.id, 0, true, &auth).unwrap();
    assert!(s.block(b.id, &a).is_err());
    assert!(s.history(b.id, &a).is_err());
    assert!(s.ink_snapshot(b.id, &a).is_err());
    assert!(s.search("private token", &a).unwrap().is_empty());
    assert!(s.exercises(n.id, true, &a).unwrap().is_empty());
    assert!(s.links(target.id, true, &a).unwrap().is_empty());
    assert!(s.follow_link(link.id, &a).is_err());
    assert!(s.prepare_native_export(&[n.id], true, &a).is_err());
    assert!(s.review_deletion(n.id, &a).is_err());
    for encrypted in [false, true] {
        let p = s.prepare_exercise_export(e.id, true, &auth).unwrap();
        let protection = if encrypted {
            ExportProtection::Encrypted {
                password: "Synthetic example passphrase",
            }
        } else {
            ExportProtection::Plaintext {
                acknowledge_protected: true,
            }
        };
        assert!(matches!(
            s.finish_native_export(p, protection, &mut TestAuth(false), true),
            Err(Error::Denied)
        ));
    }
    let review = s.review_deletion(n.id, &auth).unwrap();
    let mut auth = auth;
    auth.relock();
    assert!(matches!(s.trash(review, true, &auth), Err(Error::Denied)));
    assert!(s.entity(n.id, &a).is_ok());
}
#[test]
fn reassignment_requires_preview_confirmation_and_undo_preserves_new_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("membership-history.db");
    let mut s = Store::open(&path).unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let b = text(&mut s, n.id, "work");
    let one = s.create_exercise(n.id, "same name", &[b.id], &a).unwrap();
    let two = s.create_exercise(n.id, "same name", &[], &a).unwrap();
    assert_ne!(one.id, two.id);
    let p = s
        .plan_reassignment(n.id, &[b.id], Some(two.id), &a)
        .unwrap();
    assert_eq!(p.changes()[0].from, Some(one.id));
    assert!(matches!(
        s.commit_reassignment(p, false, &a),
        Err(Error::ReviewRequired)
    ));
    let p = s
        .plan_reassignment(n.id, &[b.id], Some(two.id), &a)
        .unwrap();
    let op = s.commit_reassignment(p, true, &a).unwrap();
    assert!(ids(&s, one.id, &a).is_empty());
    assert_eq!(ids(&s, two.id, &a), vec![b.id]);
    s.edit_content(b.id, 0, Content::Text("newer content stays".into()), &a)
        .unwrap();
    drop(s);
    let mut s = Store::open(&path).unwrap();
    let undo = s.undo_reassignment(op.id, &a).unwrap();
    assert_eq!(ids(&s, one.id, &a), vec![b.id]);
    assert_eq!(
        s.block(b.id, &a).unwrap().content.literal(),
        "newer content stays"
    );
    assert!(matches!(
        s.undo_reassignment(op.id, &a),
        Err(Error::Conflict)
    ));
    s.undo_reassignment(undo.id, &a).unwrap();
    assert_eq!(ids(&s, two.id, &a), vec![b.id]);
}
#[test]
fn reassignment_rejects_stale_review_cross_master_and_aba_history() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let other = s.create(Kind::Note, None, "other", &a).unwrap();
    let b = text(&mut s, n.id, "work");
    let e = s.create_exercise(n.id, "exercise", &[b.id], &a).unwrap();
    let x = s.create_exercise(other.id, "foreign", &[], &a).unwrap();
    assert!(s.plan_reassignment(n.id, &[b.id], Some(x.id), &a).is_err());
    let plan = s.plan_reassignment(n.id, &[b.id], None, &a).unwrap();
    s.rename_exercise(e.id, 0, "changed", &a).unwrap();
    assert!(matches!(
        s.commit_reassignment(plan, true, &a),
        Err(Error::Conflict)
    ));
    let plan = s.plan_reassignment(n.id, &[b.id], None, &a).unwrap();
    let op = s.commit_reassignment(plan, true, &a).unwrap();
    let plan = s.plan_reassignment(n.id, &[b.id], Some(e.id), &a).unwrap();
    s.commit_reassignment(plan, true, &a).unwrap();
    let plan = s.plan_reassignment(n.id, &[b.id], None, &a).unwrap();
    s.commit_reassignment(plan, true, &a).unwrap();
    assert!(matches!(
        s.undo_reassignment(op.id, &a),
        Err(Error::Conflict)
    ));
}
#[test]
fn protected_reassignment_carries_lock_even_when_unassigned() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let b = text(&mut s, n.id, "protected work");
    let e = s.create_exercise(n.id, "protected", &[b.id], &a).unwrap();
    let auth = access([Scope::Exercise(e.id), Scope::Block(b.id)]);
    s.set_exercise_lock(e.id, 0, true, &auth).unwrap();
    assert!(s.plan_reassignment(n.id, &[b.id], None, &a).is_err());
    let p = s.plan_reassignment(n.id, &[b.id], None, &auth).unwrap();
    let op = s.commit_reassignment(p, true, &auth).unwrap();
    assert!(s.block(b.id, &a).is_err());
    assert!(s.block(b.id, &auth).unwrap().locked);
    s.undo_reassignment(op.id, &auth).unwrap();
    assert!(s.block(b.id, &auth).unwrap().locked);
}
#[test]
fn failed_extraction_and_reassignment_roll_back_every_membership() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let h = text(&mut s, n.id, "Exercise 1");
    s.set_heading(h.id, 0, true, &a).unwrap();
    let b = text(&mut s, n.id, "work");
    let p = s.plan_extraction(n.id, &a).unwrap();
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.accept_extraction(p, true, &a).is_err());
    assert!(s.exercises(n.id, true, &a).unwrap().is_empty());
    let e = extract(&mut s, n.id).remove(0);
    let p = s.plan_reassignment(n.id, &[h.id, b.id], None, &a).unwrap();
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.commit_reassignment(p, true, &a).is_err());
    assert_eq!(ids(&s, e.id, &a), vec![h.id, b.id]);
    s.check_integrity().unwrap();
}
#[test]
fn independent_copy_preserves_ink_but_has_separate_ids_edits_modes_and_status() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let original = stroke(0);
    let b = s.append_ink(n.id, vec![original.clone()], &a).unwrap();
    s.edit_content(
        b.id,
        0,
        Content::Code {
            text: "  001\n".into(),
            language: None,
        },
        &a,
    )
    .unwrap();
    let e = s.create_exercise(n.id, "exercise", &[b.id], &a).unwrap();
    s.set_mode(n.id, WritingMode::InkOnly, &a).unwrap();
    assert_eq!(s.exercise_mode(e.id, &a).unwrap(), WritingMode::InkOnly);
    let copy = s.copy_exercise_independent(e.id, &a).unwrap();
    assert_ne!(copy, n.id);
    let cb = s.blocks(copy, 0, 50, &a).unwrap().remove(0);
    assert_ne!(cb.id, b.id);
    assert_ne!(cb.source_ids[0], original.id);
    assert_eq!(s.source_ink(cb.id, &a).unwrap()[0].points, original.points);
    assert_eq!(s.mode(copy, &a).unwrap(), WritingMode::InkOnly);
    s.set_mode(copy, WritingMode::Assisted, &a).unwrap();
    assert_eq!(s.exercise_mode(e.id, &a).unwrap(), WritingMode::InkOnly);
    s.edit_content(cb.id, cb.revision, Content::Text("copy edit".into()), &a)
        .unwrap();
    assert_eq!(s.block(b.id, &a).unwrap().content.literal(), "  001\n");
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s
        .edit_content(b.id, 1, Content::Text("unsaved".into()), &a)
        .is_err());
    assert_eq!(
        s.exercise_save_state(e.id, &a).unwrap(),
        SaveState::NeedsAttention
    );
    assert_eq!(
        s.document_save_state(copy, &a).unwrap(),
        SaveState::SavedLocally
    );
}
#[test]
fn selected_exercise_export_excludes_unrelated_master_content_and_roundtrips() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s
        .create(Kind::Note, None, "master secret title", &a)
        .unwrap();
    let chosen = text(&mut s, n.id, "chosen working");
    text(&mut s, n.id, "UNSELECTED_PRIVATE_VALUE");
    let e = s
        .create_exercise(n.id, "selected exercise", &[chosen.id], &a)
        .unwrap();
    let p = s.prepare_exercise_export(e.id, false, &a).unwrap();
    assert_eq!(p.selected_blocks, 1);
    let file = s
        .finish_native_export(
            p,
            ExportProtection::Plaintext {
                acknowledge_protected: false,
            },
            &mut TestAuth(false),
            true,
        )
        .unwrap();
    let text = String::from_utf8(file.bytes.clone()).unwrap();
    assert!(!text.contains("UNSELECTED_PRIVATE_VALUE"));
    assert!(!text.contains("master secret title"));
    let copied = s.import_native(&file.bytes, &a).unwrap()[0];
    assert_eq!(s.blocks(copied, 0, 50, &a).unwrap().len(), 1);
}
#[test]
fn current_native_preserves_exercise_identity_relationships_locks_modes_and_heading_rules() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let h = text(&mut s, n.id, "Problem 1");
    s.set_heading(h.id, 0, true, &a).unwrap();
    s.set_exercise_keyword(n.id, "Problem", &a).unwrap();
    let b = text(&mut s, n.id, "working");
    let e = extract(&mut s, n.id).remove(0);
    let auth = access([Scope::Exercise(e.id)]);
    s.set_exercise_lock(e.id, e.revision, true, &auth).unwrap();
    s.set_mode(n.id, WritingMode::InkOnly, &a).unwrap();
    let file = export(&s, n.id, &auth);
    let doc = NativeDocument::decode(&file).unwrap();
    assert_eq!(doc.version, 10);
    assert_eq!(doc.memberships.len(), 2);
    let new = s.import_native(&file, &a).unwrap()[0];
    assert!(s.exercises(new, true, &a).unwrap().is_empty());
    // The returned imported document exposes only authorised identities; tests inspect
    // its native payload to establish a trusted auth scope, never grant via a link.
    let db_dir = tempfile::tempdir().unwrap();
    let path = db_dir.path().join("copy.db");
    let mut target = Store::open(&path).unwrap();
    let imported = target.import_native(&file, &a).unwrap()[0];
    let db = rusqlite::Connection::open(&path).unwrap();
    let id: String = db
        .query_row(
            "SELECT id FROM exercises WHERE master=?1",
            [imported.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let eid = Id::parse(&id).unwrap();
    let ia = access([Scope::Exercise(eid)]);
    let copy = target.exercise(eid, &ia).unwrap();
    assert_ne!(eid, e.id);
    assert_eq!(copy.master, imported);
    assert_ne!(copy.heading, Some(h.id));
    assert_eq!(ids(&target, eid, &ia).len(), 2);
    assert_eq!(
        target.exercise_mode(eid, &ia).unwrap(),
        WritingMode::InkOnly
    );
    assert!(target
        .plan_extraction(imported, &ia)
        .unwrap()
        .candidates()
        .is_empty());
    assert!(s.block(b.id, &auth).is_ok());
    target.check_integrity().unwrap();
}
#[test]
fn legacy_v1_native_remains_readable_and_malformed_v2_membership_is_rejected() {
    use sha2::{Digest, Sha256};
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    let b = text(&mut s, n.id, "work");
    let bytes = export(&s, n.id, &a);
    let doc = NativeDocument::decode(&bytes).unwrap();
    let mut value = serde_json::to_value(&doc).unwrap();
    value["version"] = serde_json::json!(1);
    for key in ["exercises", "memberships", "headings", "rules", "modes"] {
        value.as_object_mut().unwrap().remove(key);
    }
    let wrap = |v: serde_json::Value| {
        let payload = serde_json::to_string(&v).unwrap();
        serde_json::to_vec(&serde_json::json!({"sha256":format!("{:x}",Sha256::digest(payload.as_bytes())),"payload":payload})).unwrap()
    };
    assert!(s.import_native(&wrap(value), &a).is_ok());
    let e = s.create_exercise(n.id, "exercise", &[b.id], &a).unwrap();
    let mut value =
        serde_json::to_value(NativeDocument::decode(&export(&s, n.id, &a)).unwrap()).unwrap();
    value["memberships"][0]["exercise"] = serde_json::json!(Id::new());
    let before = s.list(None, Order::Name, None, None, &a).unwrap().len();
    assert!(s.import_native(&wrap(value), &a).is_err());
    assert_eq!(
        s.list(None, Order::Name, None, None, &a).unwrap().len(),
        before
    );
    assert_eq!(ids(&s, e.id, &a), vec![b.id]);
}
#[test]
fn master_trash_restore_and_purge_preserve_then_remove_exercise_state() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let b = text(&mut s, n.id, "work");
    let e = s.create_exercise(n.id, "exercise", &[b.id], &a).unwrap();
    let p = s.plan_reassignment(n.id, &[b.id], None, &a).unwrap();
    let op = s.commit_reassignment(p, true, &a).unwrap();
    s.undo_reassignment(op.id, &a).unwrap();
    let review = s.review_deletion(n.id, &a).unwrap();
    assert!(review.has_dependencies);
    assert!(matches!(
        s.trash(review, false, &a),
        Err(Error::ReviewRequired)
    ));
    let review = s.review_deletion(n.id, &a).unwrap();
    s.trash(review, true, &a).unwrap();
    assert!(s.exercise(e.id, &a).is_err());
    s.restore(n.id, &a).unwrap();
    assert_eq!(ids(&s, e.id, &a), vec![b.id]);
    let review = s.review_deletion(n.id, &a).unwrap();
    s.trash(review, true, &a).unwrap();
    s.purge(n.id, &a).unwrap();
    assert!(s.exercise(e.id, &a).is_err());
    s.check_integrity().unwrap();
}

#[test]
fn flat_and_search_views_follow_visibility_and_permissions_without_copying_content() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let e = s.create_exercise(n.id, "Exercise target", &[], &a).unwrap();
    let b = s
        .append_content_to(
            WritingTarget::Exercise(e.id),
            Content::Text("needle".into()),
            &a,
        )
        .unwrap();
    assert_eq!(s.flat_exercises(&a).unwrap().len(), 1);
    let hit = s.search_views("needle", &a).unwrap().remove(0);
    assert_eq!(hit.master, n.id);
    assert_eq!(hit.block, Some(b.id));
    assert_eq!(hit.exercise, Some(e.id));
    s.set_exercise_visible(e.id, e.revision, false, &a).unwrap();
    assert!(s.flat_exercises(&a).unwrap().is_empty());
    assert_eq!(s.search_views("needle", &a).unwrap()[0].exercise, None);
    let e = s.exercise(e.id, &a).unwrap();
    let auth = access([Scope::Exercise(e.id)]);
    s.set_exercise_lock(e.id, e.revision, true, &auth).unwrap();
    assert!(s.search_views("needle", &a).unwrap().is_empty());
}
