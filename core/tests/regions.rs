mod common;
use common::*;
use stem_core::{
    content::Content,
    security::Access,
    store::{Kind, Store},
    Error,
};
#[test]
fn regions_split_merge_without_replacing_observations_and_undo_restores_partition() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let strokes = vec![stroke(0), stroke(1), stroke(2)];
    let b = s.append_ink(n.id, strokes.clone(), &a).unwrap();
    let b = s
        .partition_regions(b.id, 0, vec![b.source_ids.clone()], true, &a)
        .unwrap();
    let b = s
        .split_region(
            b.id,
            1,
            b.regions[0].id,
            vec![vec![strokes[0].id], vec![strokes[1].id, strokes[2].id]],
            &a,
        )
        .unwrap();
    let b = s
        .merge_regions(
            b.id,
            2,
            &b.regions.iter().map(|r| r.id).collect::<Vec<_>>(),
            &a,
        )
        .unwrap();
    assert_eq!(b.regions.len(), 1);
    assert_eq!(s.source_ink(b.id, &a).unwrap(), strokes);
    s.undo(n.id, &a).unwrap();
    assert_eq!(s.block(b.id, &a).unwrap().regions.len(), 2);
}
#[test]
fn invalid_partition_and_overlapping_split_are_atomic() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0), stroke(1)], &a).unwrap();
    assert!(s
        .partition_regions(
            b.id,
            0,
            vec![vec![b.source_ids[0]], vec![b.source_ids[0]]],
            true,
            &a
        )
        .is_err());
    assert_eq!(s.block(b.id, &a).unwrap(), b);
}
#[test]
fn selected_region_receives_only_selected_vector_strokes_and_commits_only_it() {
    use std::time::Duration;
    use stem_core::processing::*;
    use stem_core::{store::InkSnapshot, Result};
    struct Echo;
    impl Recogniser for Echo {
        fn probe(&self) -> Result<()> {
            Ok(())
        }
        fn recognise(&self, s: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
            assert_eq!(s.strokes().len(), 1);
            Ok(RecognitionOutput {
                candidates: vec![Content::Text("selected".into())],
                raw_output: "selected".into(),
                confidence: None,
                diagnostics: vec![],
            })
        }
    }
    let mut m = Manager::new();
    m.register_recogniser(
        Descriptor {
            id: "test".into(),
            version: "test".into(),
            adapter_version: "test".into(),
            family: Family::Recogniser,
            input: InputContract::OrderedStrokesV1,
            tasks: vec![Task::Prose],
            internals: Internals::StrokeOnlyVerified,
            offline: true,
            content_egress: false,
            personal_use_evidence: Some("own synthetic fixture".into()),
            redistributable_resources: false,
            resource_sha256: None,
            runtime_contract: "stem-processing-v1".into(),
        },
        Box::new(Echo),
    )
    .unwrap();
    m.activate_local("test", None, true).unwrap();
    m.select(Task::Prose, "test").unwrap();
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0), stroke(1)], &a).unwrap();
    let b = s
        .partition_regions(
            b.id,
            0,
            b.source_ids.iter().map(|id| vec![*id]).collect(),
            true,
            &a,
        )
        .unwrap();
    let snapshot = s.region_snapshot(b.id, b.regions[0].id, &a).unwrap();
    let result = m
        .recognise(
            snapshot,
            Task::Prose,
            None,
            &JobControl::new(Duration::from_secs(1)).unwrap(),
        )
        .unwrap();
    let p = s.propose(result, &a).unwrap();
    assert!(s.block(b.id, &a).unwrap().regions[0].content.is_none());
    let b = s.accept_proposal(p.id, 0, &a).unwrap();
    assert_eq!(b.regions[0].content.as_ref().unwrap().literal(), "selected");
    assert!(b.regions[1].content.is_none());
    assert_eq!(s.search("selected", &a).unwrap().len(), 1);
    assert!(matches!(
        s.merge_regions(
            b.id,
            b.revision,
            &b.regions.iter().map(|r| r.id).collect::<Vec<_>>(),
            &a
        ),
        Err(Error::ReviewRequired)
    ));
}
#[test]
fn region_exports_remap_ids_and_keep_accepted_text() {
    use stem_core::portable::ExportProtection;
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let b = s
        .partition_regions(b.id, 0, vec![b.source_ids.clone()], true, &a)
        .unwrap();
    let b = s
        .edit_region(
            b.id,
            b.revision,
            b.regions[0].id,
            Content::Code {
                text: "  x = 3\n".into(),
                language: None,
            },
            &a,
        )
        .unwrap();
    let plan = s.prepare_native_export(&[n.id], true, &a).unwrap();
    let bytes = s
        .finish_native_export(
            plan,
            ExportProtection::Plaintext {
                acknowledge_protected: true,
            },
            &mut TestAuth(true),
            true,
        )
        .unwrap()
        .bytes;
    let mut copy = Store::memory().unwrap();
    let ids = copy.import_native(&bytes, &a).unwrap();
    let copied = copy.blocks(ids[0], 0, 10, &a).unwrap().remove(0);
    assert_ne!(copied.regions[0].id, b.regions[0].id);
    assert_ne!(copied.regions[0].strokes, b.regions[0].strokes);
    assert_eq!(copied.regions[0].content, b.regions[0].content);
}
