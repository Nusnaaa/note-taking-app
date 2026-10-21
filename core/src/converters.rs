//! Deterministic presentation converters. They do not evaluate maths or repair code.
use crate::{
    content::{Content, MathForm, MathNode},
    processing::{Converted, Converter, Fidelity, Format, JobControl},
    Error, Result,
};

pub struct TextConverter;
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn latex(n: &MathNode, warnings: &mut Vec<String>) -> String {
    match &n.form {
        MathForm::Token(t) => t.clone(),
        MathForm::Text(t) => {
            warnings.push("Text-in-maths requires dialect review; retained verbatim.".into());
            format!("\\text{{{t}}}")
        }
        MathForm::Row(v) => v.iter().map(|n| latex(n, warnings)).collect::<String>(),
        MathForm::Fraction {
            numerator,
            denominator,
        } => format!(
            "\\frac{{{}}}{{{}}}",
            latex(numerator, warnings),
            latex(denominator, warnings)
        ),
        MathForm::Root { radicand, index } => {
            let i = index
                .as_deref()
                .map(|n| format!("[{}]", latex(n, warnings)))
                .unwrap_or_default();
            format!("\\sqrt{i}{{{}}}", latex(radicand, warnings))
        }
        MathForm::Scripts { base, sub, sup } => {
            let mut s = latex(base, warnings);
            if let Some(n) = sub {
                s += &format!("_{{{}}}", latex(n, warnings));
            }
            if let Some(n) = sup {
                s += &format!("^{{{}}}", latex(n, warnings));
            }
            s
        }
        MathForm::Fence { open, body, close } => {
            if close.is_none() {
                warnings.push("Incomplete fence retained; no closing delimiter invented.".into());
            }
            format!(
                "{}{}{}",
                open,
                latex(body, warnings),
                close.as_deref().unwrap_or("")
            )
        }
        MathForm::Matrix(rows) => format!(
            "\\begin{{matrix}}{}\\end{{matrix}}",
            rows.iter()
                .map(|r| r
                    .iter()
                    .map(|n| latex(n, warnings))
                    .collect::<Vec<_>>()
                    .join(" & "))
                .collect::<Vec<_>>()
                .join(" \\\\ ")
        ),
        MathForm::AlignedLines(lines) => format!(
            "\\begin{{aligned}}{}\\end{{aligned}}",
            lines
                .iter()
                .map(|n| latex(n, warnings))
                .collect::<Vec<_>>()
                .join(" \\\\ ")
        ),
        MathForm::Hole => {
            warnings.push("An editing hole has no ordinary LaTeX equivalent.".into());
            "\\square".into()
        }
        MathForm::Unknown(t) | MathForm::Unparsed(t) => {
            warnings.push("Unparsed notation retained verbatim; rendering and structural roundtrip are unverified.".into());
            t.clone()
        }
        MathForm::Alternatives(v) => {
            warnings
                .push("Unresolved alternatives require review; no candidate was selected.".into());
            v.iter()
                .map(|n| latex(n, warnings))
                .collect::<Vec<_>>()
                .join(" | ")
        }
    }
}
fn mathml(n: &MathNode, warnings: &mut Vec<String>) -> String {
    match &n.form {
        MathForm::Token(t) => format!("<mi>{}</mi>", xml(t)),
        MathForm::Text(t) => format!("<mtext>{}</mtext>", xml(t)),
        MathForm::Row(v) => format!(
            "<mrow>{}</mrow>",
            v.iter().map(|n| mathml(n, warnings)).collect::<String>()
        ),
        MathForm::Fraction {
            numerator,
            denominator,
        } => format!(
            "<mfrac>{}{}</mfrac>",
            mathml(numerator, warnings),
            mathml(denominator, warnings)
        ),
        MathForm::Root { radicand, index } => {
            if let Some(i) = index {
                format!(
                    "<mroot>{}{}</mroot>",
                    mathml(radicand, warnings),
                    mathml(i, warnings)
                )
            } else {
                format!("<msqrt>{}</msqrt>", mathml(radicand, warnings))
            }
        }
        MathForm::Scripts { base, sub, sup } => match (sub, sup) {
            (Some(a), Some(b)) => format!(
                "<msubsup>{}{}{}</msubsup>",
                mathml(base, warnings),
                mathml(a, warnings),
                mathml(b, warnings)
            ),
            (Some(a), None) => format!(
                "<msub>{}{}</msub>",
                mathml(base, warnings),
                mathml(a, warnings)
            ),
            (None, Some(a)) => format!(
                "<msup>{}{}</msup>",
                mathml(base, warnings),
                mathml(a, warnings)
            ),
            _ => mathml(base, warnings),
        },
        MathForm::Fence { open, body, close } => format!(
            "<mrow><mo>{}</mo>{}<mo>{}</mo></mrow>",
            xml(open),
            mathml(body, warnings),
            xml(close.as_deref().unwrap_or(""))
        ),
        MathForm::Matrix(rows) => format!(
            "<mtable>{}</mtable>",
            rows.iter()
                .map(|r| format!(
                    "<mtr>{}</mtr>",
                    r.iter()
                        .map(|n| format!("<mtd>{}</mtd>", mathml(n, warnings)))
                        .collect::<String>()
                ))
                .collect::<String>()
        ),
        MathForm::AlignedLines(lines) => format!(
            "<mtable>{}</mtable>",
            lines
                .iter()
                .map(|n| format!("<mtr><mtd>{}</mtd></mtr>", mathml(n, warnings)))
                .collect::<String>()
        ),
        _ => {
            warnings.push("Unsupported presentation node preserved as labelled text; native structure is not retained in this derivative.".into());
            format!("<mtext>{}</mtext>", xml(&n.literal()))
        }
    }
}
impl Converter for TextConverter {
    fn formats(&self) -> Vec<Format> {
        vec![
            Format::Markdown,
            Format::Latex,
            Format::MathMl,
            Format::PlainText,
        ]
    }
    fn convert(
        &self,
        content: &Content,
        target: Format,
        control: &JobControl,
    ) -> Result<Converted> {
        control.check()?;
        content.validate()?;
        let mut warnings = Vec::new();
        let mut lexical_change = false;
        let text = match (content, &target) {
            (Content::Ink, _) => return Err(Error::ReviewRequired),
            (
                Content::Text(t) | Content::Numeric(t) | Content::Code { text: t, .. },
                Format::PlainText,
            ) => t.clone(),
            (Content::Text(t) | Content::Numeric(t), Format::Markdown) => t.clone(),
            (Content::Code { text, .. }, Format::Markdown) => {
                let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
                let fence = "`".repeat((longest + 1).max(3));
                lexical_change = !text.ends_with('\n');
                format!(
                    "{fence}\n{text}{}{fence}\n",
                    if lexical_change { "\n" } else { "" }
                )
            }
            (Content::Maths(n), Format::Latex) => latex(n, &mut warnings),
            (Content::Maths(n), Format::Markdown) => format!("$${}$$", latex(n, &mut warnings)),
            (Content::Maths(n), Format::MathMl) => format!(
                "<math xmlns=\"http://www.w3.org/1998/Math/MathML\">{}</math>",
                mathml(n, &mut warnings)
            ),
            (Content::Maths(n), Format::PlainText) => {
                warnings.push("Plain text does not preserve mathematical layout; use native structure or a reviewed maths format.".into());
                n.literal()
            }
            _ => return Err(Error::Unsupported),
        };
        let complete = warnings.is_empty();
        Ok(Converted {
            text,
            fidelity: Fidelity {
                complete,
                lexical_change,
                layout_change: target != Format::PlainText,
                unsupported: warnings,
                semantic_equivalence_asserted: false,
            },
        })
    }
}
