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
    let note = s
        .create(Kind::Note, None, "pinned replacement fixture", &a)
        .unwrap();
    let block = s.append_ink(note.id, vec![stroke(0)], &a).unwrap();
    let control = JobControl::new(Duration::from_secs(5)).unwrap();
    let before_probe = m
        .prepare_recognition(s.ink_snapshot(block.id, &a).unwrap(), Task::Prose, None)
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
    assert!(before_probe.run(&control).is_ok());
    let before_commit = m
        .prepare_recognition(s.ink_snapshot(block.id, &a).unwrap(), Task::Prose, None)
        .unwrap();
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s
        .activate_engine(next.id, &mut m, Box::new(Fake(true)), 100, &a)
        .is_err());
    assert_eq!(m.descriptors()[0].version, "1");
    assert!(before_commit.run(&control).is_ok());
    let before_swap = m
        .prepare_recognition(s.ink_snapshot(block.id, &a).unwrap(), Task::Prose, None)
        .unwrap();
    s.activate_engine(next.id, &mut m, Box::new(Fake(true)), 100, &a)
        .unwrap();
    assert_eq!(m.descriptors()[0].version, "2");
    assert!(matches!(before_swap.run(&control), Err(Error::Cancelled)));
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
    assert_eq!(fresh.default_for(Task::Prose), Some("synthetic"));
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
    for owned in [false, true] {
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
        if owned {
            let plan = s.engine_removal_plan(v.id, &a).unwrap();
            let retired = s
                .commit_engine_removal(
                    &plan,
                    &mut m,
                    &JobControl::new(Duration::from_secs(30)).unwrap(),
                    &a,
                )
                .unwrap();
            drop(retired);
        } else {
            s.remove_engine(v.id, &mut m, &a).unwrap();
        }
        assert_eq!(m.readiness("synthetic"), Readiness::Missing);
        assert!(s.engine_defaults(&a).unwrap().is_empty());
        assert_eq!(
            s.block(b.id, &a).unwrap().content,
            Content::Text("synthetic".into())
        );
        assert_eq!(s.source_ink(b.id, &a).unwrap(), raw);
    }
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

#[test]
fn converters_have_durable_staged_lifecycle_and_failed_probe_preserves_previous() {
    let dir = tempfile::tempdir().unwrap();
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let mut m = Manager::new();
    let mut d = descriptor("converter-1");
    d.family = Family::Converter;
    d.input = InputContract::EditableV1;
    d.tasks.clear();
    let first = s
        .stage_engine(d.clone(), None, dir.path(), 0, true, &a)
        .unwrap();
    s.activate_converter(
        first.id,
        &mut m,
        Box::new(stem_core::converters::TextConverter),
        0,
        &a,
    )
    .unwrap();
    struct Broken;
    impl Converter for Broken {
        fn probe(&self) -> Result<()> {
            Err(Error::WorkerFailed)
        }
        fn formats(&self) -> Vec<Format> {
            vec![Format::PlainText]
        }
        fn convert(&self, _: &Content, _: Format, _: &JobControl) -> Result<Converted> {
            Err(Error::WorkerFailed)
        }
    }
    d.version = "converter-2".into();
    let second = s.stage_engine(d, None, dir.path(), 0, true, &a).unwrap();
    assert!(s
        .activate_converter(second.id, &mut m, Box::new(Broken), 0, &a)
        .is_err());
    assert_eq!(
        s.active_engine("synthetic", &a).unwrap().unwrap().id,
        first.id
    );
    assert_eq!(
        m.convert(
            "synthetic",
            &Content::Text("λ".into()),
            Format::PlainText,
            &JobControl::new(Duration::from_secs(1)).unwrap()
        )
        .unwrap()
        .text,
        "λ"
    );
}

#[test]
fn catalogue_pages_all_versions_without_exposing_resource_paths() {
    let dir = tempfile::tempdir().unwrap();
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let mut ids = Vec::new();
    for version in ["1", "2", "3"] {
        ids.push(
            s.stage_engine(descriptor(version), None, dir.path(), 100, true, &a)
                .unwrap()
                .id,
        );
    }
    s.activate_engine(ids[1], &mut Manager::new(), Box::new(Fake(true)), 100, &a)
        .unwrap();
    ids.sort();
    let mut cursor = None;
    let mut observed = Vec::new();
    let mut active = 0;
    loop {
        let page = s.engine_catalogue(cursor, 1, &a).unwrap();
        assert_eq!(page.versions.len(), 1);
        let json = serde_json::to_string(&page).unwrap();
        assert!(!json.contains("local_resource"));
        assert!(!json.contains(&dir.path().display().to_string()));
        active += usize::from(page.versions[0].active);
        observed.push(page.versions[0].id);
        match page.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(observed, ids);
    assert_eq!(active, 1);
    assert!(matches!(
        s.engine_catalogue(None, 0, &a),
        Err(Error::ResourceLimit)
    ));
    assert!(matches!(
        s.engine_catalogue(None, 129, &a),
        Err(Error::ResourceLimit)
    ));
    assert!(s
        .engine_catalogue(Some(*ids.last().unwrap()), 1, &a)
        .unwrap()
        .versions
        .is_empty());
}

#[test]
fn converter_cannot_persist_a_recognition_default() {
    let dir = tempfile::tempdir().unwrap();
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let mut m = Manager::new();
    let mut d = descriptor("1");
    d.family = Family::Converter;
    d.input = InputContract::EditableV1;
    let version = s.stage_engine(d, None, dir.path(), 0, true, &a).unwrap();
    s.activate_converter(
        version.id,
        &mut m,
        Box::new(stem_core::converters::TextConverter),
        0,
        &a,
    )
    .unwrap();
    assert!(s
        .select_engine(Task::Prose, "synthetic", &mut m, &a)
        .is_err());
    assert!(s.engine_defaults(&a).unwrap().is_empty());
    assert_eq!(m.default_for(Task::Prose), None);
}

#[test]
fn replacement_preserves_unsupported_preference_without_runtime_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let mut m = Manager::new();
    let first = s
        .stage_engine(descriptor("1"), None, dir.path(), 0, true, &a)
        .unwrap();
    s.activate_engine(first.id, &mut m, Box::new(Fake(true)), 0, &a)
        .unwrap();
    s.select_engine(Task::Prose, "synthetic", &mut m, &a)
        .unwrap();
    let mut d = descriptor("2");
    d.tasks = vec![Task::Maths];
    let next = s.stage_engine(d, None, dir.path(), 0, true, &a).unwrap();
    s.activate_engine(next.id, &mut m, Box::new(Fake(true)), 0, &a)
        .unwrap();
    assert_eq!(
        s.engine_defaults(&a).unwrap(),
        vec![(Task::Prose, "synthetic".into())]
    );
    assert_eq!(m.default_for(Task::Prose), None);
    assert_eq!(m.default_for(Task::Maths), None);
    s.activate_engine(first.id, &mut m, Box::new(Fake(true)), 0, &a)
        .unwrap();
    assert_eq!(m.default_for(Task::Prose), Some("synthetic"));
}

#[test]
fn catalogue_and_active_lookup_reject_mismatched_engine_binding() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let version = s
        .stage_engine(descriptor("1"), None, dir.path(), 0, true, &a)
        .unwrap();
    s.activate_engine(version.id, &mut Manager::new(), Box::new(Fake(true)), 0, &a)
        .unwrap();
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute(
        "UPDATE engine_versions SET engine='different' WHERE id=?1",
        [version.id.to_string()],
    )
    .unwrap();
    assert!(matches!(
        s.engine_catalogue(None, 8, &a),
        Err(Error::Integrity)
    ));
    assert!(matches!(
        s.engine_version(version.id, &a),
        Err(Error::Integrity)
    ));
    assert!(matches!(
        s.active_engine("synthetic", &a),
        Err(Error::Integrity)
    ));
}

#[test]
fn owned_removal_rolls_back_then_detaches_without_disposing_pinned_sdk() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Owned(Arc<AtomicUsize>);
    impl Drop for Owned {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl Recogniser for Owned {
        fn probe(&self) -> Result<()> {
            Ok(())
        }
        fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
            Err(Error::EngineUnavailable)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let mut m = Manager::new();
    let mut descriptor = descriptor("1");
    descriptor.resource_sha256 = Some(format!("{:x}", Sha256::digest(b"model")));
    let version = s
        .stage_engine(
            descriptor,
            Some(&mut Cursor::new(b"model")),
            dir.path(),
            5,
            true,
            &a,
        )
        .unwrap();
    let resource = version.local_resource.clone().unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    s.activate_engine(
        version.id,
        &mut m,
        Box::new(Owned(Arc::clone(&dropped))),
        5,
        &a,
    )
    .unwrap();
    s.select_engine(Task::Prose, "synthetic", &mut m, &a)
        .unwrap();
    let note = s.create(Kind::Note, None, "removal fixture", &a).unwrap();
    let block = s.append_ink(note.id, vec![stroke(0)], &a).unwrap();
    let before = s.source_ink(block.id, &a).unwrap();
    let work = m
        .prepare_recognition(s.ink_snapshot(block.id, &a).unwrap(), Task::Prose, None)
        .unwrap();
    let plan = s.engine_removal_plan(version.id, &a).unwrap();
    let control = JobControl::new(Duration::from_secs(30)).unwrap();
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s
        .commit_engine_removal(&plan, &mut m, &control, &a)
        .is_err());
    assert_eq!(m.readiness("synthetic"), Readiness::Ready);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert_eq!(
        s.active_engine("synthetic", &a).unwrap().unwrap().id,
        version.id
    );
    assert_eq!(
        s.engine_defaults(&a).unwrap(),
        vec![(Task::Prose, "synthetic".into())]
    );
    let owner = s
        .commit_engine_removal(&plan, &mut m, &control, &a)
        .unwrap()
        .unwrap();
    assert_eq!(m.readiness("synthetic"), Readiness::Missing);
    assert_eq!(m.default_for(Task::Prose), None);
    assert!(s.engine_defaults(&a).unwrap().is_empty());
    assert!(s.active_engine("synthetic", &a).unwrap().is_none());
    assert_eq!(
        s.engine_version(version.id, &a).unwrap().state,
        Installation::Removed
    );
    drop(owner);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert!(matches!(work.run(&control), Err(Error::Cancelled)));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(s.source_ink(block.id, &a).unwrap(), before);
    assert_eq!(std::fs::read(resource).unwrap(), b"model");
}
#[test]
fn owned_removal_rejects_cancelled_foreign_stale_and_runtime_mismatched_plans() {
    let dir = tempfile::tempdir().unwrap();
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let mut m = Manager::new();
    let v = s
        .stage_engine(descriptor("1"), None, dir.path(), 0, true, &a)
        .unwrap();
    s.activate_engine(v.id, &mut m, Box::new(Fake(true)), 0, &a)
        .unwrap();
    let plan = s.engine_removal_plan(v.id, &a).unwrap();
    let cancelled = JobControl::new(Duration::from_secs(30)).unwrap();
    cancelled.cancel();
    assert!(matches!(
        s.commit_engine_removal(&plan, &mut m, &cancelled, &a),
        Err(Error::Cancelled)
    ));
    let control = JobControl::new(Duration::from_secs(30)).unwrap();
    assert!(matches!(
        Store::memory()
            .unwrap()
            .commit_engine_removal(&plan, &mut m, &control, &a),
        Err(Error::Conflict)
    ));
    let mut other = Manager::new();
    other
        .register_recogniser(descriptor("2"), Box::new(Fake(true)))
        .unwrap();
    other.activate_local("synthetic", None, true).unwrap();
    assert!(matches!(
        s.commit_engine_removal(&plan, &mut other, &control, &a),
        Err(Error::Conflict)
    ));
    assert_eq!(
        s.engine_version(v.id, &a).unwrap().state,
        Installation::Installed
    );
    s.create(Kind::Note, None, "generation changed", &a)
        .unwrap();
    assert!(matches!(
        s.commit_engine_removal(&plan, &mut m, &control, &a),
        Err(Error::Conflict)
    ));
    assert_eq!(m.readiness("synthetic"), Readiness::Ready);
    let app = Access::authenticate(
        &mut common::TestAuth(true),
        [stem_core::security::Scope::App].into_iter().collect(),
    )
    .unwrap();
    s.set_app_lock(true, &app).unwrap();
    let protected = s.engine_removal_plan(v.id, &app).unwrap();
    assert!(matches!(
        s.commit_engine_removal(&protected, &mut m, &control, &a),
        Err(Error::Denied)
    ));
    assert_eq!(m.readiness("synthetic"), Readiness::Ready);
}
#[test]
fn inactive_and_repeated_removal_preserve_the_active_default_and_do_not_write_twice() {
    let dir = tempfile::tempdir().unwrap();
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let mut m = Manager::new();
    let first = s
        .stage_engine(descriptor("1"), None, dir.path(), 0, true, &a)
        .unwrap();
    s.activate_engine(first.id, &mut m, Box::new(Fake(true)), 0, &a)
        .unwrap();
    s.select_engine(Task::Prose, "synthetic", &mut m, &a)
        .unwrap();
    let next = s
        .stage_engine(descriptor("2"), None, dir.path(), 0, true, &a)
        .unwrap();
    let control = JobControl::new(Duration::from_secs(30)).unwrap();
    let plan = s.engine_removal_plan(next.id, &a).unwrap();
    assert!(s
        .commit_engine_removal(&plan, &mut m, &control, &a)
        .unwrap()
        .is_none());
    assert_eq!(m.default_for(Task::Prose), Some("synthetic"));
    assert_eq!(m.descriptor("synthetic").unwrap().version, "1");
    let active_plan = s.engine_removal_plan(first.id, &a).unwrap();
    let repeated = s.engine_removal_plan(next.id, &a).unwrap();
    assert!(s
        .commit_engine_removal(&repeated, &mut m, &control, &a)
        .unwrap()
        .is_none());
    // A repeated acknowledgement must not advance generation and stale this plan.
    assert!(s
        .commit_engine_removal(&active_plan, &mut m, &control, &a)
        .unwrap()
        .is_some());
}
