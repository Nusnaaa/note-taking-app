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
    assert_eq!(m.readiness("hash"), Readiness::Missing);
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
