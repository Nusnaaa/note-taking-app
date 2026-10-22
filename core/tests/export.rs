mod common;
use common::*;
use stem_core::{
    content::{Content, MathNode},
    export::*,
    objects::*,
    security::{Access, Scope},
    store::{Kind, Store},
    Error, Id,
};
#[test]
fn converted_only_export_never_reads_or_discloses_original_ink() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    s.edit_content(b.id, 0, Content::Maths(MathNode::token("1+1=3")), &a)
        .unwrap();
    s.append_ink(n.id, vec![stroke(1)], &a).unwrap();
    let p = s
        .prepare_derivative(Selection::Document(n.id), false, &a)
        .unwrap();
    assert!(!p.includes_original_ink);
    assert!(!p.warnings.is_empty());
    let bundle = s.finish_derivative(p, &mut TestAuth(true), true).unwrap();
    assert_eq!(bundle.assets.len(), 0);
    assert!(bundle.markdown.contains("1+1=3"));
    assert!(!bundle.markdown.contains("1.2345678901234567"));
    let p = s
        .prepare_derivative(Selection::Blocks(vec![b.id]), true, &a)
        .unwrap();
    assert!(p.includes_original_ink);
    let bundle = s.finish_derivative(p, &mut TestAuth(true), true).unwrap();
    assert_eq!(bundle.assets.len(), 1);
    assert!(
        String::from_utf8(bundle.assets.values().next().unwrap().clone())
            .unwrap()
            .contains("1.2345678901234567")
    );
}
#[test]
fn derivative_scope_and_fresh_auth_cannot_be_bypassed() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    let b = s
        .append_content(n.id, Content::Text("selected".into()), &a)
        .unwrap();
    s.append_content(n.id, Content::Text("adjacent secret".into()), &a)
        .unwrap();
    let e = s.create_exercise(n.id, "exercise", &[b.id], &a).unwrap();
    let p = s
        .prepare_derivative(Selection::Exercise(e.id), false, &a)
        .unwrap();
    let bundle = s.finish_derivative(p, &mut TestAuth(true), true).unwrap();
    assert!(!bundle.markdown.contains("adjacent"));
    let auth = access([Scope::Exercise(e.id)]);
    s.set_exercise_lock(e.id, 0, true, &auth).unwrap();
    assert!(s
        .prepare_derivative(Selection::Exercise(e.id), false, &a)
        .is_err());
    let p = s
        .prepare_derivative(Selection::Exercise(e.id), false, &auth)
        .unwrap();
    assert!(matches!(
        s.finish_derivative(p, &mut TestAuth(false), true),
        Err(Error::Denied)
    ));
    let p = s
        .prepare_derivative(Selection::Exercise(e.id), false, &auth)
        .unwrap();
    s.rename_exercise(e.id, 1, "changed", &auth).unwrap();
    assert!(matches!(
        s.finish_derivative(p, &mut TestAuth(true), true),
        Err(Error::Conflict)
    ));
}
#[test]
fn diagram_svg_escapes_labels_and_canvas_scope_rejects_hidden_payload() {
    let d = Content::Diagram(Diagram {
        parts: vec![DiagramPart {
            id: Id::new(),
            shape: Shape::Rectangle(Rect {
                x: 1.,
                y: 2.,
                width: 5.,
                height: 6.,
            }),
            label: "<script>remote()</script>".into(),
        }],
    });
    let bounds = Rect {
        x: 0.,
        y: 0.,
        width: 20.,
        height: 20.,
    };
    let svg = technical_svg(&d, bounds).unwrap();
    assert!(!svg.contains("<script>"));
    assert!(svg.contains("&lt;script&gt;"));
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Canvas, None, "canvas", &a).unwrap();
    let b = s.append_content(n.id, d, &a).unwrap();
    s.set_layout(b.id, None, bounds, 0, &a).unwrap();
    assert!(s
        .prepare_derivative(
            Selection::CanvasRegion {
                canvas: n.id,
                bounds: Rect {
                    width: 5.,
                    ..bounds
                }
            },
            false,
            &a
        )
        .is_err());
    assert!(s
        .prepare_derivative(
            Selection::CanvasRegion {
                canvas: n.id,
                bounds
            },
            true,
            &a
        )
        .is_err());
    let p = s
        .prepare_derivative(
            Selection::CanvasRegion {
                canvas: n.id,
                bounds,
            },
            false,
            &a,
        )
        .unwrap();
    assert_eq!(
        s.finish_derivative(p, &mut TestAuth(true), true)
            .unwrap()
            .assets
            .len(),
        1
    );
}
#[test]
fn pagination_never_bisects_indivisible_work_or_silently_scales() {
    let p = paginate(
        &[(150., 150.), (100., 150.), (180., 30.)],
        Page::default(),
        false,
    )
    .unwrap();
    assert_eq!(p.iter().map(|p| p.page).collect::<Vec<_>>(), vec![0, 1, 1]);
    assert_eq!(p[0].scale, 1.);
    assert_eq!(p[1].y_mm, 12.);
    assert!(matches!(
        paginate(&[(400., 100.)], Page::default(), false),
        Err(Error::ReviewRequired)
    ));
    let p = paginate(&[(400., 100.)], Page::default(), true).unwrap();
    assert!(p[0].scale < 1.);
    let landscape = Page {
        width_mm: 297.,
        height_mm: 210.,
        margin_mm: 12.,
    };
    assert!(paginate(&[(250., 100.)], landscape, false).is_ok());
}

#[test]
fn graph_derivative_preserves_authored_order_and_refuses_concealed_points() {
    let mut graph = Graph {
        x: Axis {
            label: "time".into(),
            unit: Some("s".into()),
            min: 0.,
            max: 10.,
        },
        y: Axis {
            label: "force".into(),
            unit: Some("N".into()),
            min: 0.,
            max: 10.,
        },
        series: vec![Series {
            id: Id::new(),
            label: "literal <series>".into(),
            points: vec![Position { x: 8., y: 2. }, Position { x: 2., y: 8. }],
        }],
    };
    let bounds = Rect {
        x: 0.,
        y: 0.,
        width: 100.,
        height: 100.,
    };
    let svg = technical_svg(&Content::Graph(graph.clone()), bounds).unwrap();
    assert!(svg.contains("80,80 20,20"));
    assert!(svg.contains("literal &lt;series&gt;"));
    graph.series[0].points.push(Position { x: f64::MAX, y: 2. });
    assert!(matches!(
        technical_svg(&Content::Graph(graph), bounds),
        Err(Error::ReviewRequired)
    ));
}
