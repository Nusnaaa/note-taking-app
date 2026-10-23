mod common;
use common::*;
use stem_core::{
    security::{Access, Scope},
    store::*,
    Error, Id,
};

#[test]
fn protected_receipt_denies_matching_changed_payload_and_changed_target_before_comparison() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let canvas = s
        .create(Kind::Canvas, None, "private capture", &a)
        .unwrap()
        .id;
    let target = WritingTarget::Entity(canvas);
    let original = stroke(0);
    let ticket = Id::new();
    let b = s
        .append_capture(ticket, target, vec![original.clone()], &a)
        .unwrap();
    let proof = access([Scope::Block(b.id)]);
    s.set_block_lock(b.id, b.revision, true, &proof).unwrap();
    let mut changed = original.clone();
    changed.points[0].x += 1.;
    for (destination, source) in [
        (target, original.clone()),
        (target, changed.clone()),
        (
            WritingTarget::CanvasLayer {
                canvas,
                layer: LayerRef::Base,
            },
            original.clone(),
        ),
    ] {
        assert!(matches!(
            s.append_capture(ticket, destination, vec![source], &a),
            Err(Error::Denied)
        ));
    }
    assert!(matches!(
        s.capture_committed(ticket, canvas, &a),
        Err(Error::Denied)
    ));
    assert_eq!(
        s.append_capture(ticket, target, vec![original.clone()], &proof)
            .unwrap()
            .id,
        b.id
    );
    assert!(matches!(
        s.append_capture(ticket, target, vec![changed], &proof),
        Err(Error::Conflict)
    ));
    assert_eq!(s.source_ink(b.id, &proof).unwrap(), vec![original]);
}

#[test]
fn receipt_uses_current_exercise_protection_even_when_original_target_was_master() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "master", &a).unwrap();
    let original = stroke(0);
    let ticket = Id::new();
    let target = WritingTarget::Entity(n.id);
    let b = s
        .append_capture(ticket, target, vec![original.clone()], &a)
        .unwrap();
    let exercise = s
        .create_exercise(n.id, "private exercise", &[b.id], &a)
        .unwrap();
    let proof = access([Scope::Exercise(exercise.id)]);
    s.set_exercise_lock(exercise.id, exercise.revision, true, &proof)
        .unwrap();
    let mut changed = original.clone();
    changed.points[1].pressure = None;
    for source in [original.clone(), changed.clone()] {
        assert!(matches!(
            s.append_capture(ticket, target, vec![source], &a),
            Err(Error::Denied)
        ));
    }
    assert!(matches!(
        s.append_capture(
            ticket,
            WritingTarget::Exercise(exercise.id),
            vec![original.clone()],
            &a
        ),
        Err(Error::Denied)
    ));
    assert!(s.capture_committed(ticket, n.id, &proof).unwrap());
    assert_eq!(
        s.append_capture(ticket, target, vec![original.clone()], &proof)
            .unwrap()
            .id,
        b.id
    );
    assert!(matches!(
        s.append_capture(ticket, target, vec![changed], &proof),
        Err(Error::Conflict)
    ));
    assert_eq!(s.source_ink(b.id, &proof).unwrap(), vec![original]);
}

#[test]
fn protected_corrupt_payload_is_not_deserialised_to_answer_a_receipt_query() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipt.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "source", &a).unwrap();
    let original = stroke(0);
    let ticket = Id::new();
    let b = s
        .append_capture(
            ticket,
            WritingTarget::Entity(n.id),
            vec![original.clone()],
            &a,
        )
        .unwrap();
    let proof = access([Scope::Block(b.id)]);
    s.set_block_lock(b.id, b.revision, true, &proof).unwrap();
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE blocks SET payload=json_set(payload,'$.content',json(?1)) WHERE id=?2",
        [
            r#"{"type":"UnknownPrivateContent","value":"private fixture"}"#.to_string(),
            b.id.to_string(),
        ],
    )
    .unwrap();
    drop(db);
    let mut s = Store::open(&path).unwrap();
    assert!(matches!(
        s.capture_committed(ticket, n.id, &a),
        Err(Error::Denied)
    ));
    let mut changed = original.clone();
    changed.points[0].x += 1.;
    for source in [original, changed] {
        assert!(matches!(
            s.append_capture(ticket, WritingTarget::Entity(n.id), vec![source], &a),
            Err(Error::Denied)
        ));
    }
    assert!(matches!(
        s.capture_committed(ticket, n.id, &proof),
        Err(Error::Json(_))
    ));
}

#[test]
fn naming_a_public_destination_does_not_bypass_the_receipts_actual_owner_lock() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let private = s.create(Kind::Note, None, "private owner", &a).unwrap();
    let public = s.create(Kind::Note, None, "unrelated", &a).unwrap();
    let original = stroke(0);
    let ticket = Id::new();
    let b = s
        .append_capture(
            ticket,
            WritingTarget::Entity(private.id),
            vec![original.clone()],
            &a,
        )
        .unwrap();
    let proof = access([Scope::Entity(private.id)]);
    s.set_lock(
        private.id,
        s.entity(private.id, &a).unwrap().revision,
        true,
        &proof,
    )
    .unwrap();
    let mut changed = original.clone();
    changed.points[0].x += 1.;
    for source in [original.clone(), changed] {
        assert!(matches!(
            s.append_capture(ticket, WritingTarget::Entity(public.id), vec![source], &a),
            Err(Error::Denied)
        ));
    }
    assert!(matches!(
        s.capture_committed(ticket, public.id, &a),
        Err(Error::Denied)
    ));
    assert!(matches!(
        s.append_capture(
            ticket,
            WritingTarget::Entity(public.id),
            vec![original.clone()],
            &proof
        ),
        Err(Error::Conflict)
    ));
    assert!(s.blocks(public.id, 0, 10, &a).unwrap().is_empty());
    assert_eq!(s.source_ink(b.id, &proof).unwrap(), vec![original]);
}

#[test]
fn ordinary_block_reads_reject_a_payload_spliced_into_a_different_public_owner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("binding.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let private = s.create(Kind::Note, None, "private owner", &a).unwrap();
    let public = s.create(Kind::Note, None, "public owner", &a).unwrap();
    let b = s.append_ink(private.id, vec![stroke(0)], &a).unwrap();
    let proof = access([Scope::Entity(private.id)]);
    s.set_lock(
        private.id,
        s.entity(private.id, &a).unwrap().revision,
        true,
        &proof,
    )
    .unwrap();
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE blocks SET payload=json_set(payload,'$.entity',?1) WHERE id=?2",
        [public.id.to_string(), b.id.to_string()],
    )
    .unwrap();
    drop(db);
    let s = Store::open(&path).unwrap();
    assert!(matches!(s.block(b.id, &a), Err(Error::Integrity)));
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE blocks SET payload=json_set(payload,'$.entity',?1,'$.id',?2) WHERE id=?3",
        [
            private.id.to_string(),
            Id::new().to_string(),
            b.id.to_string(),
        ],
    )
    .unwrap();
    drop(db);
    let s = Store::open(&path).unwrap();
    assert!(matches!(s.block(b.id, &proof), Err(Error::Integrity)));
}
