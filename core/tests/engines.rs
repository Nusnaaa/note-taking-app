mod common;
use common::*;
use sha2::{Digest, Sha256};
use std::{io::Cursor, time::Duration};
use stem_core::{content::Content, processing::*, security::Access, store::*, Error, Result};
struct Fake(bool);
impl Recogniser for Fake {
    fn probe(&self) -> Result<()> {
        if self.0 {
            Ok(())
        } else {
            Err(Error::WorkerFailed)
        }
    }
    fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
        Ok(RecognitionOutput {
            candidates: vec![Content::Text("synthetic".into())],
            raw_output: "synthetic".into(),
            confidence: None,
            diagnostics: vec![],
        })
    }
}
fn descriptor(version: &str) -> Descriptor {
    Descriptor {
        id: "synthetic".into(),
        version: version.into(),
        adapter_version: "1".into(),
        family: Family::Recogniser,
        input: InputContract::OrderedStrokesV1,
        tasks: vec![Task::Prose],
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some("app-owned test double, no recognition quality claim".into()),
        redistributable_resources: false,
        resource_sha256: None,
        runtime_contract: "stem-processing-v1".into(),
    }
}
#[test]
fn staged_probe_or_commit_failure_keeps_previous_verified_engine() {
    let dir = tempfile::tempdir().unwrap();
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let mut m = Manager::new();
    let old = s
        .stage_engine(descriptor("1"), None, dir.path(), 100, true, &a)
        .unwrap();
    s.activate_engine(old.id, &mut m, Box::new(Fake(true)), 100, &a)
        .unwrap();
    s.select_engine(Task::Prose, "synthetic", &mut m, &a)
        .unwrap();
    let next = s
        .stage_engine(descriptor("2"), None, dir.path(), 100, true, &a)
        .unwrap();
    assert!(s
        .activate_engine(next.id, &mut m, Box::new(Fake(false)), 100, &a)
        .is_err());
    assert_eq!(
        s.active_engine("synthetic", &a).unwrap().unwrap().id,
        old.id
    );
    assert_eq!(m.descriptors()[0].version, "1");
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s
        .activate_engine(next.id, &mut m, Box::new(Fake(true)), 100, &a)
        .is_err());
    assert_eq!(m.descriptors()[0].version, "1");
    s.activate_engine(next.id, &mut m, Box::new(Fake(true)), 100, &a)
        .unwrap();
    assert_eq!(m.descriptors()[0].version, "2");
    s.activate_engine(old.id, &mut m, Box::new(Fake(true)), 100, &a)
        .unwrap();
    assert_eq!(m.descriptors()[0].version, "1");
}
#[test]
fn installed_provenance_and_profiles_reopen_without_fabricated_runtime_readiness() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engines.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let v = s
        .stage_engine(descriptor("1"), None, dir.path(), 100, true, &a)
        .unwrap();
    let mut m = Manager::new();
    s.activate_engine(v.id, &mut m, Box::new(Fake(true)), 100, &a)
        .unwrap();
    s.select_engine(Task::Prose, "synthetic", &mut m, &a)
        .unwrap();
    drop(s);
    let mut s = Store::open(path).unwrap();
    let mut fresh = Manager::new();
    assert_eq!(
        s.active_engine("synthetic", &a).unwrap().unwrap().state,
        Installation::Installed
    );
    assert_eq!(fresh.readiness("synthetic"), Readiness::Missing);
    assert!(s
        .select_engine(Task::Prose, "synthetic", &mut fresh, &a)
        .is_err());
    assert_eq!(
        s.engine_defaults(&a).unwrap(),
        vec![(Task::Prose, "synthetic".into())]
    );
    s.activate_engine(v.id, &mut fresh, Box::new(Fake(true)), 100, &a)
        .unwrap();
    assert_eq!(fresh.readiness("synthetic"), Readiness::Ready);
}

#[test]
fn another_connection_changing_state_during_probe_prevents_stale_activation() {
    struct ConcurrentProbe(std::path::PathBuf);
    impl Recogniser for ConcurrentProbe {
        fn probe(&self) -> Result<()> {
            Store::open(&self.0)?.create(Kind::Note, None, "concurrent work", &Access::public())?;
            Ok(())
        }
        fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
            Err(Error::EngineUnavailable)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("concurrent.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let v = s
        .stage_engine(descriptor("1"), None, dir.path(), 100, true, &a)
        .unwrap();
    let mut m = Manager::new();
    assert!(matches!(
        s.activate_engine(v.id, &mut m, Box::new(ConcurrentProbe(path)), 100, &a),
        Err(Error::Conflict)
    ));
    assert_eq!(
        s.engine_version(v.id, &a).unwrap().state,
        Installation::Staged
    );
    assert!(s.active_engine("synthetic", &a).unwrap().is_none());
    assert_eq!(m.readiness("synthetic"), Readiness::Missing);
}
#[test]
fn resource_installation_requires_consent_digest_and_bounded_streaming() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("resources");
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let mut d = descriptor("1");
    d.resource_sha256 = Some(format!("{:x}", Sha256::digest(b"model")));
    assert!(s
        .stage_engine(
            d.clone(),
            Some(&mut Cursor::new(b"model")),
            &root,
            100,
            false,
            &a
        )
        .is_err());
    assert!(!root.exists());
    assert!(s
        .stage_engine(
            d.clone(),
            Some(&mut Cursor::new(b"model")),
            &root,
            2,
            true,
            &a
        )
        .is_err());
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    assert!(s
        .stage_engine(
            d.clone(),
            Some(&mut Cursor::new(b"bad")),
            &root,
            100,
            true,
            &a
        )
        .is_err());
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    let v = s
        .stage_engine(d, Some(&mut Cursor::new(b"model")), &root, 100, true, &a)
        .unwrap();
    std::fs::write(v.local_resource.unwrap(), b"tampered").unwrap();
    assert!(s
        .activate_engine(v.id, &mut Manager::new(), Box::new(Fake(true)), 100, &a)
        .is_err());
    assert!(s.active_engine("synthetic", &a).unwrap().is_none());
}
#[test]
fn removing_engine_preserves_notebooks_strokes_and_accepted_work() {
    let a = Access::public();
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let v = s
        .stage_engine(descriptor("1"), None, dir.path(), 100, true, &a)
        .unwrap();
    let mut m = Manager::new();
    s.activate_engine(v.id, &mut m, Box::new(Fake(true)), 100, &a)
        .unwrap();
    s.select_engine(Task::Prose, "synthetic", &mut m, &a)
        .unwrap();
    let output = m
        .recognise(
            s.ink_snapshot(b.id, &a).unwrap(),
            Task::Prose,
            None,
            &JobControl::new(Duration::from_secs(1)).unwrap(),
        )
        .unwrap();
    let p = s.propose(output, &a).unwrap();
    s.accept_proposal(p.id, 0, &a).unwrap();
    let raw = s.source_ink(b.id, &a).unwrap();
    s.remove_engine(v.id, &mut m, &a).unwrap();
    assert_eq!(m.readiness("synthetic"), Readiness::Missing);
    assert!(s.engine_defaults(&a).unwrap().is_empty());
    assert_eq!(
        s.block(b.id, &a).unwrap().content,
        Content::Text("synthetic".into())
    );
    assert_eq!(s.source_ink(b.id, &a).unwrap(), raw);
}
#[test]
fn batch_acceptance_is_selected_and_atomic_on_stale_members() {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "note", &a).unwrap();
    let mut m = Manager::new();
    m.register_recogniser(descriptor("1"), Box::new(Fake(true)))
        .unwrap();
    m.activate_local("synthetic", None, true).unwrap();
    m.select(Task::Prose, "synthetic").unwrap();
    let mut proposals = Vec::new();
    for i in 0..3 {
        let b = s.append_ink(n.id, vec![stroke(i)], &a).unwrap();
        let o = m
            .recognise(
                s.ink_snapshot(b.id, &a).unwrap(),
                Task::Prose,
                None,
                &JobControl::new(Duration::from_secs(1)).unwrap(),
            )
            .unwrap();
        proposals.push(s.propose(o, &a).unwrap());
    }
    s.edit_content(
        proposals[1].block,
        0,
        Content::Text("owner correction".into()),
        &a,
    )
    .unwrap();
    assert!(s
        .accept_proposals(&[(proposals[0].id, 0), (proposals[1].id, 0)], true, &a)
        .is_err());
    assert_eq!(
        s.proposal(proposals[0].id, &a).unwrap().state,
        ProposalState::Pending
    );
    assert_eq!(
        s.block(proposals[0].block, &a).unwrap().content,
        Content::Ink
    );
    s.accept_proposals(&[(proposals[0].id, 0), (proposals[2].id, 0)], true, &a)
        .unwrap();
    assert_eq!(
        s.proposal(proposals[1].id, &a).unwrap().state,
        ProposalState::Pending
    );
    assert_eq!(
        s.block(proposals[1].block, &a).unwrap().content,
        Content::Text("owner correction".into())
    );
}
