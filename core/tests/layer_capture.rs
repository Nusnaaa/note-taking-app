mod common;
use common::*;
use stem_core::{
    security::{Access, Scope},
    store::*,
    Error, Id,
};
fn setup(s: &mut Store) -> (Id, Id) {
    let a = Access::public();
    let canvas = s.create(Kind::Canvas, None, "destination", &a).unwrap().id;
    let layer = Id::new();
    s.set_canvas_layers(
        CanvasLayers {
            canvas,
            revision: 0,
            base_visible: false,
            layers: vec![CanvasLayer {
                id: layer,
                name: "selected".into(),
                visible: true,
                blocks: vec![],
            }],
        },
        0,
        &a,
    )
    .unwrap();
    (canvas, layer)
}
#[test]
fn capture_and_layer_membership_commit_undo_redo_and_reopen_together_without_rewriting_ink() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("layers.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let (canvas, layer) = setup(&mut s);
    let ticket = Id::new();
    let original = stroke(0);
    let target = WritingTarget::CanvasLayer {
        canvas,
        layer: LayerRef::Custom(layer),
    };
    let b = s
        .append_capture(ticket, target, vec![original.clone()], &a)
        .unwrap();
    assert_eq!(
        s.canvas_layers(canvas, &a).unwrap().layers[0].blocks,
        vec![b.id]
    );
    assert_eq!(
        s.canvas_scene(canvas, None, 0, 10, &a).unwrap().items.len(),
        1
    );
    s.undo(canvas, &a).unwrap();
    assert!(s.canvas_layers(canvas, &a).unwrap().layers[0]
        .blocks
        .is_empty());
    assert!(s
        .canvas_scene(canvas, None, 0, 10, &a)
        .unwrap()
        .items
        .is_empty());
    assert_eq!(s.source_ink(b.id, &a).unwrap(), vec![original.clone()]);
    s.redo(canvas, &a).unwrap();
    drop(s);
    let mut s = Store::open(&path).unwrap();
    assert_eq!(
        s.canvas_layers(canvas, &a).unwrap().layers[0].blocks,
        vec![b.id]
    );
    assert_eq!(
        s.append_capture(ticket, target, vec![original.clone()], &a)
            .unwrap()
            .id,
        b.id
    );
    assert_eq!(s.source_ink(b.id, &a).unwrap(), vec![original]);
}
#[test]
fn missing_layer_never_falls_back_and_identical_committed_receipt_does_not_rebind_membership() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let (canvas, layer) = setup(&mut s);
    let target = WritingTarget::CanvasLayer {
        canvas,
        layer: LayerRef::Custom(layer),
    };
    let ticket = Id::new();
    let original = stroke(0);
    let b = s
        .append_capture(ticket, target, vec![original.clone()], &a)
        .unwrap();
    let mut state = s.canvas_layers(canvas, &a).unwrap();
    let revision = state.revision;
    state.layers.clear();
    s.set_canvas_layers(state, revision, &a).unwrap();
    assert_eq!(
        s.append_capture(ticket, target, vec![original], &a)
            .unwrap()
            .id,
        b.id
    );
    assert!(matches!(
        s.append_capture(Id::new(), target, vec![stroke(1)], &a),
        Err(Error::ReviewRequired)
    ));
    assert_eq!(s.blocks(canvas, 0, 10, &a).unwrap().len(), 1);
    assert!(s.canvas_layers(canvas, &a).unwrap().layers.is_empty());
    assert!(matches!(
        s.append_capture(
            ticket,
            WritingTarget::CanvasLayer {
                canvas,
                layer: LayerRef::Base
            },
            vec![stroke(0)],
            &a
        ),
        Err(Error::Conflict)
    ));
}
#[test]
fn layer_capture_checks_owner_protection_and_rolls_back_source_and_destination_on_failure() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let (canvas, layer) = setup(&mut s);
    let target = WritingTarget::CanvasLayer {
        canvas,
        layer: LayerRef::Custom(layer),
    };
    let ticket = Id::new();
    let original = stroke(0);
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s
        .append_capture(ticket, target, vec![original.clone()], &a)
        .is_err());
    assert!(!s.capture_committed(ticket, canvas, &a).unwrap());
    assert!(s.blocks(canvas, 0, 10, &a).unwrap().is_empty());
    assert!(s.canvas_layers(canvas, &a).unwrap().layers[0]
        .blocks
        .is_empty());
    let b = s
        .append_capture(ticket, target, vec![original], &a)
        .unwrap();
    let proof = access([Scope::Block(b.id)]);
    s.set_block_lock(b.id, 0, true, &proof).unwrap();
    assert!(matches!(
        s.append_capture(Id::new(), target, vec![stroke(1)], &a),
        Err(Error::Denied)
    ));
    let note = s.create(Kind::Note, None, "wrong kind", &a).unwrap();
    assert!(s
        .append_ink_to(
            WritingTarget::CanvasLayer {
                canvas: note.id,
                layer: LayerRef::Base
            },
            vec![stroke(0)],
            &a
        )
        .is_err());
    s.check_integrity().unwrap();
}
