mod common;
use common::*;
use stem_core::{
    ink::{EraserGesture, Stroke, Tool, Transform},
    security::{Access, Scope},
    store::{FailurePoint, Kind, Store},
    Error,
};
fn line(sequence: u64, a: [f64; 2], b: [f64; 2]) -> Stroke {
    let mut s = stroke(sequence);
    s.points[0].x = a[0];
    s.points[0].y = a[1];
    s.points[1].x = b[0];
    s.points[1].y = b[1];
    s
}
fn gesture(path: Vec<[f64; 2]>, radius: f64) -> EraserGesture {
    EraserGesture {
        coordinate_space: "synthetic-local-units".into(),
        path,
        radius,
    }
}
#[test]
fn crossing_segments_and_single_points_erase_one_operation_and_preserve_source_after_reopen() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("ink.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let note = s.create(Kind::Note, None, "erase", &a).unwrap();
    let strokes = vec![
        line(0, [-10., 0.], [10., 0.]),
        line(1, [0., 2.], [0., 2.]),
        line(2, [100., 0.], [101., 0.]),
    ];
    let block = s.append_ink(note.id, strokes.clone(), &a).unwrap();
    let erased = s
        .erase_gesture(
            block.id,
            block.revision,
            &gesture(vec![[0., -3.], [0., 3.]], 0.1),
            &a,
        )
        .unwrap();
    assert_eq!(erased.visible_strokes, vec![strokes[2].id]);
    assert_eq!(erased.revision, block.revision + 1);
    assert_eq!(s.source_ink(block.id, &a).unwrap(), strokes);
    s.undo(note.id, &a).unwrap();
    assert_eq!(
        s.block(block.id, &a).unwrap().visible_strokes,
        block.visible_strokes
    );
    s.redo(note.id, &a).unwrap();
    drop(s);
    let s = Store::open(path).unwrap();
    assert_eq!(
        s.block(block.id, &a).unwrap().visible_strokes,
        erased.visible_strokes
    );
    assert_eq!(
        serde_json::to_vec(&s.source_ink(block.id, &a).unwrap()).unwrap(),
        serde_json::to_vec(&strokes).unwrap()
    );
}
#[test]
fn radius_boundary_collinear_disjoint_and_miss_do_not_create_false_hits_or_undo() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "geometry", &a).unwrap();
    let source = line(0, [0., 0.], [10., 0.]);
    let b = s.append_ink(n.id, vec![source.clone()], &a).unwrap();
    for path in [
        vec![[20., 0.], [30., 0.]],
        vec![[0., 2.], [10., 2.]],
        vec![[5., 1.001]],
    ] {
        assert_eq!(
            s.erase_gesture(b.id, b.revision, &gesture(path, 1.), &a)
                .unwrap(),
            b
        );
    }
    // The only Undo remains the original capture, not the three missed gestures.
    s.undo(n.id, &a).unwrap();
    assert!(!s.block(b.id, &a).unwrap().visible);
    s.redo(n.id, &a).unwrap();
    let b = s.block(b.id, &a).unwrap();
    let hit = s
        .erase_gesture(b.id, b.revision, &gesture(vec![[5., 1.]], 1.), &a)
        .unwrap();
    assert!(hit.visible_strokes.is_empty());
}
#[test]
fn source_space_and_revision_protection_overflow_and_commit_failure_preserve_visibility() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "guard", &a).unwrap();
    let b = s
        .append_ink(n.id, vec![line(0, [0., 0.], [10., 0.])], &a)
        .unwrap();
    let g = gesture(vec![[5., 0.]], 0.5);
    let mut wrong = g.clone();
    wrong.coordinate_space = "view-pixels".into();
    assert!(s.erase_gesture(b.id, b.revision, &wrong, &a).is_err());
    assert!(matches!(
        s.erase_gesture(b.id, b.revision + 1, &g, &a),
        Err(Error::Conflict)
    ));
    let mut bad = g.clone();
    bad.radius = f64::NAN;
    assert!(s.erase_gesture(b.id, b.revision, &bad, &a).is_err());
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.erase_gesture(b.id, b.revision, &g, &a).is_err());
    assert_eq!(s.block(b.id, &a).unwrap(), b);
    let b = s
        .set_block_lock(b.id, b.revision, true, &access([Scope::Block(b.id)]))
        .unwrap();
    assert!(matches!(
        s.erase_gesture(b.id, b.revision, &g, &a),
        Err(Error::Denied)
    ));
    let private = access([Scope::Block(b.id)]);
    let b = s
        .transform_block(
            b.id,
            b.revision,
            Transform {
                tx: 1000.,
                ..Transform::default()
            },
            &private,
        )
        .unwrap();
    // Explicit source coordinates are unaffected by the view transform.
    assert!(s
        .erase_gesture(b.id, b.revision, &g, &private)
        .unwrap()
        .visible_strokes
        .is_empty());
    let other = s
        .append_ink(n.id, vec![line(1, [-f64::MAX, 0.], [f64::MAX, 0.])], &a)
        .unwrap();
    assert!(s.erase_gesture(other.id, other.revision, &g, &a).is_err());
    assert_eq!(s.block(other.id, &a).unwrap(), other);
}
#[test]
fn resource_limit_or_later_mixed_space_never_publishes_an_earlier_hit_and_eraser_traces_are_not_targets(
) {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "bounded", &a).unwrap();
    let first = line(0, [0., 0.], [1., 0.]);
    let mut second = line(1, [0., 0.], [1., 0.]);
    second.coordinate_space = "other-source".into();
    let b = s.append_ink(n.id, vec![first, second], &a).unwrap();
    let g = gesture(vec![[0.5, 0.]], 0.1);
    assert!(s.erase_gesture(b.id, b.revision, &g, &a).is_err());
    assert_eq!(s.block(b.id, &a).unwrap(), b);
    let mut long = line(2, [0., 0.], [1., 0.]);
    long.points = vec![long.points[0].clone(); 1000];
    let b = s.append_ink(n.id, vec![long], &a).unwrap();
    let path = vec![[0., 0.]; 2000];
    assert!(matches!(
        s.erase_gesture(b.id, b.revision, &gesture(path, 1.), &a),
        Err(Error::ResourceLimit)
    ));
    assert_eq!(s.block(b.id, &a).unwrap(), b);
    let mut eraser = line(3, [0., 0.], [1., 0.]);
    eraser.tool = Tool::Eraser;
    let b = s.append_ink(n.id, vec![eraser], &a).unwrap();
    assert_eq!(s.erase_gesture(b.id, b.revision, &g, &a).unwrap(), b);
}
#[test]
fn host_command_is_authoring_and_cas_bound() {
    use stem_core::host::{Command, Host, Request};
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("lib.db"), 8).unwrap();
    let request = |command| {
        h.request(
            &serde_json::to_string(&Request {
                version: 1,
                command,
            })
            .unwrap(),
        )
        .unwrap()
    };
    let n = request(Command::Create {
        kind: Kind::Note,
        parent: None,
        title: "host eraser".into(),
    })
    .data
    .unwrap();
    let id = stem_core::Id::parse(n["id"].as_str().unwrap()).unwrap();
    let b = request(Command::Capture {
        ticket: stem_core::Id::new(),
        entity: id,
        strokes: vec![line(0, [0., 0.], [1., 0.])],
    });
    let b = b.data.unwrap();
    let id = stem_core::Id::parse(b["id"].as_str().unwrap()).unwrap();
    let rev = b["revision"].as_u64().unwrap();
    let cmd = || Command::EraseGesture {
        block: id,
        revision: rev,
        gesture: gesture(vec![[0.5, 0.]], 0.1),
    };
    let erased = request(cmd());
    assert!(erased.ok, "{:?}", erased.error);
    assert!(erased.data.unwrap()["visible_strokes"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!request(cmd()).ok);
    h.close().unwrap();
}

#[test]
fn degenerate_extreme_distance_rejects_instead_of_silently_reporting_a_miss() {
    let a = Access::public();
    let mut store = Store::memory().unwrap();
    let note = store.create(Kind::Note, None, "extreme", &a).unwrap();
    let b = store
        .append_ink(note.id, vec![line(0, [0., 0.], [0., 0.])], &a)
        .unwrap();
    let g = gesture(vec![[f64::MAX, f64::MAX]], 1.);
    assert!(store.erase_gesture(b.id, b.revision, &g, &a).is_err());
    assert_eq!(store.block(b.id, &a).unwrap(), b);
}
