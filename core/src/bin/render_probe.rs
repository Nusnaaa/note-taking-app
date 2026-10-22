//! Synthetic rendering fixture, never an owner's handwriting or private notebook.
use stem_core::{
    content::{Content, MathForm, MathNode},
    export::Page,
    render::render_pdf,
    Id,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: render_probe FONT OUTPUT.pdf".into());
    }
    let mut contents = vec![
        Content::Text("STEM notebook — synthetic PDF rendering check".into()),
        Content::Text("Unicode: α β γ λ Ω ∂. Editable source remains unchanged.".into()),
    ];
    contents.push(Content::Maths(MathNode {
        id: Id::new(),
        form: MathForm::Row(vec![
            MathNode {
                id: Id::new(),
                form: MathForm::Fraction {
                    numerator: Box::new(MathNode::token("∂u")),
                    denominator: Box::new(MathNode::token("∂t")),
                },
            },
            MathNode::token(" + "),
            MathNode {
                id: Id::new(),
                form: MathForm::Scripts {
                    base: Box::new(MathNode::token("x")),
                    sub: None,
                    sup: Some(Box::new(MathNode::token("2"))),
                },
            },
            MathNode::token(" = 0"),
        ]),
    }));
    contents.extend((1..=75).map(|n| {
        Content::Text(format!(
            "{n:02}. Synthetic line for deterministic page-break verification."
        ))
    }));
    let result = render_pdf(&contents, &std::fs::read(&args[1])?, Page::default())?;
    std::fs::write(&args[2], result.bytes)?;
    println!("pages={}; warnings={}", result.pages, result.warnings.len());
    Ok(())
}
