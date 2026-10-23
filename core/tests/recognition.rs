mod common;
use common::*;
use std::time::Duration;
use stem_core::{
    content::Content,
    processing::*,
    security::{Access, Scope},
    store::{Display, FailurePoint, InkSnapshot, Kind, ProposalState, Store},
    Error, Result,
};

struct Fake {
    text: String,
    cancel: bool,
    panic: bool,
}
impl Recogniser for Fake {
    fn probe(&self) -> Result<()> {
        Ok(())
    }
    fn recognise(&self, s: &InkSnapshot, _: Task, c: &JobControl) -> Result<RecognitionOutput> {
        assert!(!s.strokes().is_empty());
        if self.cancel {
            c.cancel();
        }
        if self.panic {
            panic!("synthetic worker failure");
        }
        Ok(RecognitionOutput {
            candidates: vec![Content::Text(self.text.clone())],
            raw_output: self.text.clone(),
            confidence: None,
            diagnostics: vec![],
        })
    }
}
fn descriptor(id: &str) -> Descriptor {
    Descriptor {
        id: id.into(),
        version: "synthetic-1".into(),
        adapter_version: "1".into(),
        family: Family::Recogniser,
        input: InputContract::OrderedStrokesV1,
        tasks: vec![Task::Prose],
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some(
            "app-owned synthetic test double; not a real recogniser".into(),
        ),
        redistributable_resources: false,
        resource_sha256: None,
        runtime_contract: "stem-processing-v1".into(),
    }
}
fn manager() -> Manager {
    let mut m = Manager::new();
    m.register_recogniser(
        descriptor("fake"),
        Box::new(Fake {
            text: "1 + 1 = 3".into(),
            cancel: false,
            panic: false,
        }),
    )
    .unwrap();
    m.activate_local("fake", None, true).unwrap();
    m.select(Task::Prose, "fake").unwrap();
    m
}

#[test]
fn failed_resource_reverification_clears_previous_readiness() {
    use sha2::{Digest, Sha256};
    let mut m = Manager::new();
    let mut d = descriptor("resource");
    d.resource_sha256 = Some(format!("{:x}", Sha256::digest(b"synthetic model bytes")));
    m.register_recogniser(
        d,
        Box::new(Fake {
            text: "test".into(),
            cancel: false,
            panic: false,
        }),
    )
    .unwrap();
    m.activate_local("resource", Some(b"synthetic model bytes"), true)
        .unwrap();
    assert_eq!(m.readiness("resource"), Readiness::Ready);
    assert!(matches!(
        m.activate_local("resource", Some(b"changed resource"), true),
        Err(Error::Integrity)
    ));
    assert_eq!(m.readiness("resource"), Readiness::Failed);
    assert!(m.select(Task::Prose, "resource").is_err());
}

#[test]
fn panicking_model_probe_becomes_failed_readiness_without_disabling_notes() {
    struct ProbePanic;
    impl Recogniser for ProbePanic {
        fn probe(&self) -> Result<()> {
            panic!("synthetic probe failure")
        }
        fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
            unreachable!()
        }
    }
    let mut m = Manager::new();
    m.register_recogniser(descriptor("probe"), Box::new(ProbePanic))
        .unwrap();
    assert!(matches!(
        m.activate_local("probe", None, true),
        Err(Error::WorkerFailed)
    ));
    assert_eq!(m.readiness("probe"), Readiness::Failed);
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "still writable", &a).unwrap();
    assert!(s.append_ink(n.id, vec![stroke(0)], &a).is_ok());
}
fn request(m: &Manager, s: &Store, id: stem_core::Id, a: &Access) -> CompletedRecognition {
    m.recognise(
        s.ink_snapshot(id, a).unwrap(),
        Task::Prose,
        None,
        &JobControl::new(Duration::from_secs(5)).unwrap(),
    )
    .unwrap()
}

#[test]
fn proposal_review_acceptance_and_source_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.db");
    let a = Access::public();
    let mut s = Store::open(&path).unwrap();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let original = s.source_ink(b.id, &a).unwrap();
    let p = s.propose(request(&manager(), &s, b.id, &a), &a).unwrap();
    assert_eq!(s.block(b.id, &a).unwrap(), b);
    drop(s);
    let mut s = Store::open(path).unwrap();
    s.correct_proposal(
        p.id,
        Content::Text("author's deliberate mistake".into()),
        &a,
    )
    .unwrap();
    let accepted = s.accept_proposal(p.id, 0, &a).unwrap();
    assert_eq!(accepted.display, Display::Editable);
    assert!(accepted.manually_corrected);
    assert_eq!(s.source_ink(b.id, &a).unwrap(), original);
    assert_eq!(s.proposal(p.id, &a).unwrap().output.raw_output, "1 + 1 = 3");
    assert!(s.accept_proposal(p.id, 0, &a).is_err());
}
#[test]
fn rejection_and_failed_atomic_accept_leave_ink_untouched() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let p = s.propose(request(&manager(), &s, b.id, &a), &a).unwrap();
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.accept_proposal(p.id, 0, &a).is_err());
    assert_eq!(s.block(b.id, &a).unwrap(), b);
    assert_eq!(s.proposal(p.id, &a).unwrap().state, ProposalState::Pending);
    s.reject_proposal(p.id, &a).unwrap();
    assert_eq!(s.block(b.id, &a).unwrap(), b);
    assert!(s.accept_proposal(p.id, 0, &a).is_err());
}
#[test]
fn stale_inference_and_stale_proposal_cannot_overwrite_corrections() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let m = manager();
    let pending = request(&m, &s, b.id, &a);
    let p = s.propose(request(&m, &s, b.id, &a), &a).unwrap();
    s.edit_content(b.id, 0, Content::Text("new correction".into()), &a)
        .unwrap();
    assert!(matches!(s.propose(pending, &a), Err(Error::Conflict)));
    assert!(matches!(
        s.accept_proposal(p.id, 0, &a),
        Err(Error::Conflict)
    ));
    assert_eq!(
        s.block(b.id, &a).unwrap().content.literal(),
        "new correction"
    );
}
#[test]
fn cancel_timeout_and_worker_panic_discard_results() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    for (cancel, panics) in [(true, false), (false, true)] {
        let mut m = Manager::new();
        m.register_recogniser(
            descriptor("bad"),
            Box::new(Fake {
                text: "late".into(),
                cancel,
                panic: panics,
            }),
        )
        .unwrap();
        m.activate_local("bad", None, true).unwrap();
        let result = m.recognise(
            s.ink_snapshot(b.id, &a).unwrap(),
            Task::Prose,
            Some("bad"),
            &JobControl::new(Duration::from_secs(5)).unwrap(),
        );
        assert!(matches!(
            result,
            Err(Error::Cancelled | Error::WorkerFailed)
        ));
        assert_eq!(s.block(b.id, &a).unwrap(), b);
    }
    let result = manager().recognise(
        s.ink_snapshot(b.id, &a).unwrap(),
        Task::Prose,
        None,
        &JobControl::new(Duration::ZERO).unwrap(),
    );
    assert!(matches!(result, Err(Error::Timeout)));
}
#[test]
fn model_switch_and_remove_leave_accepted_content_and_raw_strokes() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let mut m = manager();
    let p = s.propose(request(&m, &s, b.id, &a), &a).unwrap();
    let accepted = s.accept_proposal(p.id, 0, &a).unwrap();
    m.register_recogniser(
        descriptor("second"),
        Box::new(Fake {
            text: "different".into(),
            cancel: false,
            panic: false,
        }),
    )
    .unwrap();
    m.activate_local("second", None, true).unwrap();
    m.select(Task::Prose, "second").unwrap();
    m.remove("fake").unwrap();
    assert_eq!(s.block(b.id, &a).unwrap(), accepted);
    assert_eq!(s.source_ink(b.id, &a).unwrap().len(), 1);
    let proposal = s.propose(request(&m, &s, b.id, &a), &a).unwrap();
    assert_eq!(proposal.engine.id, "second");
    assert_eq!(s.block(b.id, &a).unwrap(), accepted);
}
#[test]
fn incompatible_raster_egress_and_unreviewed_licence_are_rejected() {
    for change in 0..5 {
        let mut d = descriptor("bad");
        match change {
            0 => d.input = InputContract::MandatoryRaster,
            1 => d.content_egress = true,
            2 => d.personal_use_evidence = None,
            3 => d.offline = false,
            _ => d.runtime_contract = "unknown".into(),
        };
        let mut m = Manager::new();
        assert!(m
            .register_recogniser(
                d,
                Box::new(Fake {
                    text: "".into(),
                    cancel: false,
                    panic: false
                })
            )
            .is_err());
    }
    let mut m = manager();
    assert!(m.select(Task::Maths, "fake").is_err());
}
#[test]
fn readiness_requires_consent_integrity_and_actual_probe() {
    let mut m = Manager::new();
    m.register_recogniser(
        descriptor("new"),
        Box::new(Fake {
            text: "".into(),
            cancel: false,
            panic: false,
        }),
    )
    .unwrap();
    assert_eq!(m.readiness("new"), Readiness::Missing);
    assert!(m.activate_local("new", None, false).is_err());
    assert!(m.select(Task::Prose, "new").is_err());
    m.activate_local("new", None, true).unwrap();
    assert_eq!(m.readiness("new"), Readiness::Ready);
    let mut d = descriptor("hash");
    d.resource_sha256 = Some("not-the-resource-hash".into());
    m.register_recogniser(
        d,
        Box::new(Fake {
            text: "".into(),
            cancel: false,
            panic: false,
        }),
    )
    .unwrap();
    assert!(m.activate_local("hash", Some(b"data"), true).is_err());
    assert_eq!(m.readiness("hash"), Readiness::Failed);
}
#[test]
fn locked_ink_and_proposals_cannot_be_sent_or_reviewed_without_access() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let b = s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let p = s.propose(request(&manager(), &s, b.id, &a), &a).unwrap();
    s.set_block_lock(b.id, 0, true, &access([Scope::Block(b.id)]))
        .unwrap();
    assert!(s.ink_snapshot(b.id, &a).is_err());
    assert!(s.proposal(p.id, &a).is_err());
    assert!(s.accept_proposal(p.id, 0, &a).is_err());
}

#[test]
fn stored_proposal_cannot_rebind_protected_output_to_a_public_block() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("proposal-binding.db");
    let mut store = Store::open(&path).unwrap();
    let public = Access::public();
    let note = store
        .create(Kind::Note, None, "proposal fixture", &public)
        .unwrap();
    let private = store.append_ink(note.id, vec![stroke(0)], &public).unwrap();
    let other = store.append_ink(note.id, vec![stroke(1)], &public).unwrap();
    let original = store.source_ink(private.id, &public).unwrap();
    let proposal = store
        .propose(request(&manager(), &store, private.id, &public), &public)
        .unwrap();
    let proof = access([Scope::Block(private.id)]);
    store
        .set_block_lock(private.id, private.revision, true, &proof)
        .unwrap();
    let mut payload = serde_json::to_value(&proposal).unwrap();
    payload["block"] = serde_json::json!(other.id);
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute(
        "UPDATE proposals SET payload=?1 WHERE id=?2",
        rusqlite::params![payload.to_string(), proposal.id.to_string()],
    )
    .unwrap();
    assert!(matches!(
        store.proposal(proposal.id, &public),
        Err(Error::Denied)
    ));
    assert!(matches!(
        store.proposal(proposal.id, &proof),
        Err(Error::Integrity)
    ));
    assert_eq!(store.source_ink(private.id, &proof).unwrap(), original);
}

#[test]
fn stored_proposal_protection_precedes_malformed_payload_parsing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("proposal-malformed.db");
    let mut store = Store::open(&path).unwrap();
    let public = Access::public();
    let note = store
        .create(Kind::Note, None, "proposal fixture", &public)
        .unwrap();
    let block = store.append_ink(note.id, vec![stroke(0)], &public).unwrap();
    let proposal = store
        .propose(request(&manager(), &store, block.id, &public), &public)
        .unwrap();
    let proof = access([Scope::Block(block.id)]);
    store
        .set_block_lock(block.id, block.revision, true, &proof)
        .unwrap();
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute(
        "UPDATE proposals SET payload='not-json' WHERE id=?1",
        [proposal.id.to_string()],
    )
    .unwrap();
    assert!(matches!(
        store.proposal(proposal.id, &public),
        Err(Error::Denied)
    ));
    assert!(matches!(
        store.proposal(proposal.id, &proof),
        Err(Error::Json(_))
    ));
}

#[test]
fn stored_proposal_identity_mismatch_cannot_acknowledge_a_correction() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("proposal-id.db");
    let mut store = Store::open(&path).unwrap();
    let public = Access::public();
    let note = store
        .create(Kind::Note, None, "proposal fixture", &public)
        .unwrap();
    let block = store.append_ink(note.id, vec![stroke(0)], &public).unwrap();
    let proposal = store
        .propose(request(&manager(), &store, block.id, &public), &public)
        .unwrap();
    let original = store.source_ink(block.id, &public).unwrap();
    let mut payload = serde_json::to_value(&proposal).unwrap();
    payload["id"] = serde_json::json!(stem_core::Id::new());
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute(
        "UPDATE proposals SET payload=?1 WHERE id=?2",
        rusqlite::params![payload.to_string(), proposal.id.to_string()],
    )
    .unwrap();
    assert!(matches!(
        store.correct_proposal(
            proposal.id,
            Content::Text("explicit correction".into()),
            &public
        ),
        Err(Error::Integrity)
    ));
    assert_eq!(store.source_ink(block.id, &public).unwrap(), original);
    let app = access([Scope::App]);
    store.set_app_lock(true, &app).unwrap();
    for id in [proposal.id, stem_core::Id::new()] {
        assert!(matches!(store.proposal(id, &public), Err(Error::Denied)));
    }
    assert!(matches!(
        store.proposal(proposal.id, &app),
        Err(Error::Integrity)
    ));
    assert_eq!(store.source_ink(block.id, &app).unwrap(), original);
}
