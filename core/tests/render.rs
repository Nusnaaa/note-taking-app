mod common;
use common::*;
use stem_core::{
    content::{Content, MathForm, MathNode},
    export::{Page, Selection},
    render::render_pdf,
    security::{Access, Scope},
    store::{Kind, Store},
    Error, Id,
};
fn font() -> Vec<u8> {
    std::fs::read(
        std::env::var("STEM_TEST_FONT")
            .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
    )
    .expect("Install DejaVu Sans or set STEM_TEST_FONT; font validation is not silently skipped")
}
fn fraction() -> Content {
    Content::Maths(MathNode {
        id: Id::new(),
        form: MathForm::Fraction {
            numerator: Box::new(MathNode::token("∂u")),
            denominator: Box::new(MathNode::token("∂t")),
        },
    })
}
#[test]
fn unicode_maths_and_paginated_text_produce_real_pdf_with_embedded_fonts() {
    let mut content = vec![
        Content::Text("Unicode λ, Ω, ∂. This expression is not solved.".into()),
        fraction(),
    ];
    content.extend((0..100).map(|n| Content::Text(format!("Line {n}: original authored content"))));
    let pdf = render_pdf(&content, &font(), Page::default()).unwrap();
    assert!(pdf.bytes.starts_with(b"%PDF-"));
    assert!(pdf.pages > 1);
    assert!(String::from_utf8_lossy(&pdf.bytes).contains("/FontFile"));
    assert!(pdf.bytes.ends_with(b"%%EOF"));
}
#[test]
fn invalid_font_missing_glyph_and_impossible_page_fail_without_partial_output() {
    assert!(render_pdf(&[fraction()], b"not a font", Page::default()).is_err());
    assert!(matches!(
        render_pdf(
            &[Content::Text("\u{10ffff}".into())],
            &font(),
            Page::default()
        ),
        Err(Error::Unsupported)
    ));
    assert!(render_pdf(
        &[],
        &font(),
        Page {
            width_mm: f64::NAN,
            ..Default::default()
        }
    )
    .is_err());
}
#[test]
fn protected_pdf_requires_fresh_auth_and_stale_plan_cannot_disclose_new_state() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "protected", &a).unwrap();
    s.append_content(n.id, fraction(), &a).unwrap();
    let auth = access([Scope::Entity(n.id)]);
    let revision = s.entity(n.id, &a).unwrap().revision;
    s.set_lock(n.id, revision, true, &auth).unwrap();
    let plan = s
        .prepare_derivative(Selection::Document(n.id), false, &auth)
        .unwrap();
    assert!(matches!(
        s.finish_pdf_export(plan, &font(), Page::default(), &mut TestAuth(false), true),
        Err(Error::Denied)
    ));
    let plan = s
        .prepare_derivative(Selection::Document(n.id), false, &auth)
        .unwrap();
    s.append_content(n.id, Content::Text("later".into()), &auth)
        .unwrap();
    assert!(matches!(
        s.finish_pdf_export(plan, &font(), Page::default(), &mut TestAuth(true), true),
        Err(Error::Conflict)
    ));
}
