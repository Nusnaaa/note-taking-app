use std::time::Duration;
use stem_core::{
    content::{Content, MathForm, MathNode},
    converters::TextConverter,
    help::{self, Topic},
    processing::{Converter, Format, JobControl},
    Id,
};
fn job() -> JobControl {
    JobControl::new(Duration::from_secs(5)).unwrap()
}
#[test]
fn converter_preserves_deliberately_wrong_math_without_solving() {
    let c = TextConverter;
    let n = Content::Maths(MathNode::token("1+1=3"));
    let r = c.convert(&n, Format::Latex, &job()).unwrap();
    assert_eq!(r.text, "1+1=3");
    assert!(!r.fidelity.semantic_equivalence_asserted);
}
#[test]
fn code_plaintext_is_exact_and_markdown_fence_cannot_be_closed_by_content() {
    let code = "\t001\r\n```\n<script>alert(1)</script>";
    let c = TextConverter;
    let n = Content::Code {
        text: code.into(),
        language: None,
    };
    assert_eq!(c.convert(&n, Format::PlainText, &job()).unwrap().text, code);
    let r = c.convert(&n, Format::Markdown, &job()).unwrap();
    assert!(r.text.starts_with("````\n"));
    assert!(r.text.contains(code));
    assert!(r.fidelity.lexical_change);
}
#[test]
fn unsupported_maths_is_reported_and_xml_is_escaped() {
    let c = TextConverter;
    let n = Content::Maths(MathNode {
        id: Id::new(),
        form: MathForm::Unparsed("<script>&unknown".into()),
    });
    let r = c.convert(&n, Format::MathMl, &job()).unwrap();
    assert!(!r.fidelity.complete);
    assert!(!r.fidelity.unsupported.is_empty());
    assert!(!r.text.contains("<script>"));
    assert!(r.text.contains("&lt;script&gt;&amp;unknown"));
}
#[test]
fn offline_help_covers_recovery_and_unsearchable_ink_without_remote_dependency() {
    for topic in [
        Topic::Trash,
        Topic::History,
        Topic::Retention,
        Topic::SaveFailure,
        Topic::MissingPdf,
        Topic::Recognition,
        Topic::BackupCredentials,
        Topic::RestoreCleanup,
    ] {
        let h = help::topic(topic);
        assert!(!h.next_action.is_empty());
        assert!(!h.explanation.contains("https://"));
    }
    let h = help::topic(Topic::Retention);
    assert!(h.explanation.contains("active notebook use"));
    assert!(h.explanation.contains("original handwriting remain"));
}
