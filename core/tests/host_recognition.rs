mod common;
use common::*;
use serde_json::Value;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use stem_core::{
    content::Content,
    host::{Command, Host, Request},
    processing::*,
    security::Scope,
    store::{InkSnapshot, Kind, Store},
    Error, Id, Result,
};
fn call(host: &Host, command: Command) -> Value {
    let response = host
        .request(
            &serde_json::to_string(&Request {
                version: 1,
                command,
            })
            .unwrap(),
        )
        .unwrap();
    assert!(response.ok, "{:?}", response.error);
    response.data.unwrap()
}
fn fixture() -> (tempfile::TempDir, Arc<Host>, Id, Id) {
    let root = tempfile::tempdir().unwrap();
    let host = Arc::new(Host::open(root.path().join("library.db"), 8).unwrap());
    let note = Id::parse(
        call(
            &host,
            Command::Create {
                kind: Kind::Note,
                parent: None,
                title: "recognition fixture".into(),
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let block = Id::parse(
        call(
            &host,
            Command::Capture {
                ticket: Id::new(),
                entity: note,
                strokes: vec![stroke(0)],
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    (root, host, note, block)
}
fn descriptor() -> Descriptor {
    Descriptor {
        id: "synthetic".into(),
        version: "1".into(),
        adapter_version: "1".into(),
        family: Family::Recogniser,
        input: InputContract::OrderedStrokesV1,
        tasks: vec![Task::Prose],
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some("app-owned host fixture; not a real recogniser".into()),
        redistributable_resources: false,
        resource_sha256: None,
        runtime_contract: "stem-processing-v1".into(),
    }
}
struct Model;
fn output() -> RecognitionOutput {
    RecognitionOutput {
        candidates: vec![Content::Text("1 + 1 = 3".into())],
        raw_output: "1 + 1 = 3".into(),
        confidence: None,
        diagnostics: vec![],
    }
}
impl Recogniser for Model {
    fn probe(&self) -> Result<()> {
        Ok(())
    }
    fn recognise(
        &self,
        _: &InkSnapshot,
        _: Task,
        control: &JobControl,
    ) -> Result<RecognitionOutput> {
        control.check()?;
        Ok(output())
    }
}
fn manager_with(model: Box<dyn Recogniser>) -> Manager {
    let mut manager = Manager::new();
    manager.register_recogniser(descriptor(), model).unwrap();
    manager.activate_local("synthetic", None, true).unwrap();
    manager.select(Task::Prose, "synthetic").unwrap();
    manager
}
fn complete(host: &Host, manager: &Manager, block: Id) -> CompletedRecognition {
    host.recognition_source(block, None)
        .unwrap()
        .prepare(manager, Task::Prose, None)
        .unwrap()
        .run(&JobControl::new(Duration::from_secs(30)).unwrap())
        .unwrap()
}
#[test]
fn host_manual_recognition_review_correction_and_acceptance_preserve_original_ink() {
    let (root, host, _, block) = fixture();
    let manager = manager_with(Box::new(Model));
    let original = call(&host, Command::SourceInk { block });
    let source = host.recognition_source(block, None).unwrap();
    assert_eq!(source.block(), block);
    assert_eq!(source.revision(), 0);
    let ticket = source.ticket();
    let work = source.prepare(&manager, Task::Prose, None).unwrap();
    let completed =
        std::thread::spawn(move || work.run(&JobControl::new(Duration::from_secs(30)).unwrap()))
            .join()
            .unwrap()
            .unwrap();
    let proposal = host.publish_recognition(completed.clone()).unwrap();
    assert_eq!(proposal.id, ticket);
    assert_eq!(
        call(&host, Command::Block { block })["content"]["type"],
        "Ink"
    );
    assert_eq!(
        call(
            &host,
            Command::Proposal {
                proposal: proposal.id
            }
        )["state"],
        "Pending"
    );
    call(
        &host,
        Command::CorrectProposal {
            proposal: proposal.id,
            correction: Content::Text("author's deliberate mistake".into()),
        },
    );
    assert_eq!(
        host.publish_recognition(completed.clone())
            .unwrap()
            .correction,
        Some(Content::Text("author's deliberate mistake".into()))
    );
    call(
        &host,
        Command::AcceptProposal {
            proposal: proposal.id,
            candidate: 0,
        },
    );
    assert_eq!(
        call(&host, Command::Block { block })["content"]["value"],
        "author's deliberate mistake"
    );
    assert_eq!(call(&host, Command::SourceInk { block }), original);
    assert_eq!(
        host.publish_recognition(completed).unwrap().state,
        stem_core::store::ProposalState::Accepted
    );
    host.close().unwrap();
    let reopened = Host::open(root.path().join("library.db"), 8).unwrap();
    assert_eq!(
        call(
            &reopened,
            Command::Proposal {
                proposal: proposal.id
            }
        )["state"],
        "Accepted"
    );
    assert_eq!(call(&reopened, Command::SourceInk { block }), original);
}
struct Blocking {
    started: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    cancelled: Arc<AtomicBool>,
}
impl Recogniser for Blocking {
    fn probe(&self) -> Result<()> {
        Ok(())
    }
    fn recognise(
        &self,
        _: &InkSnapshot,
        _: Task,
        control: &JobControl,
    ) -> Result<RecognitionOutput> {
        self.started.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        self.cancelled.store(
            matches!(control.check(), Err(Error::Cancelled)),
            Ordering::Release,
        );
        Ok(output()) // deliberately return success after cancellation
    }
}
#[test]
fn blocked_inference_never_holds_writer_and_relock_rejects_late_output() {
    let (_root, host, note, block) = fixture();
    let (started, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let manager = manager_with(Box::new(Blocking {
        started,
        release: Mutex::new(blocked),
        cancelled: cancelled.clone(),
    }));
    let work = host
        .recognition_source(block, None)
        .unwrap()
        .prepare(&manager, Task::Prose, None)
        .unwrap();
    let thread =
        std::thread::spawn(move || work.run(&JobControl::new(Duration::from_secs(30)).unwrap()));
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    call(
        &host,
        Command::Append {
            entity: note,
            content: Content::Text("writer remains usable".into()),
        },
    );
    call(&host, Command::Relock {});
    release.send(()).unwrap();
    assert!(matches!(thread.join().unwrap(), Err(Error::Cancelled)));
    assert!(cancelled.load(Ordering::Acquire));
    assert_eq!(manager.readiness("synthetic"), Readiness::Ready);
}
#[test]
fn completed_work_cannot_resurrect_after_relock_reauthentication_or_reopen() {
    let (root, host, _, block) = fixture();
    let manager = manager_with(Box::new(Model));
    let completed = complete(&host, &manager, block);
    call(&host, Command::Relock {});
    host.authenticate([Scope::App].into_iter().collect(), Box::new(TestAuth(true)))
        .unwrap();
    assert!(matches!(
        host.publish_recognition(completed.clone()),
        Err(Error::Cancelled)
    ));
    host.close().unwrap();
    let reopened = Host::open(root.path().join("library.db"), 8).unwrap();
    assert!(matches!(
        reopened.publish_recognition(completed),
        Err(Error::Denied)
    ));
    assert_eq!(
        call(&reopened, Command::Block { block })["content"]["type"],
        "Ink"
    );
}
#[test]
fn source_permissions_are_current_and_review_cannot_bypass_a_block_lock() {
    let (_root, host, _, block) = fixture();
    let manager = manager_with(Box::new(Model));
    host.authenticate(
        [Scope::Block(block)].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    let before_lock = host.recognition_source(block, None).unwrap();
    call(
        &host,
        Command::SetBlockLock {
            block,
            revision: 0,
            locked: true,
        },
    );
    assert!(matches!(
        before_lock.prepare(&manager, Task::Prose, None),
        Err(Error::Cancelled)
    ));
    let proposal = host
        .publish_recognition(complete(&host, &manager, block))
        .unwrap();
    call(&host, Command::Relock {});
    assert!(matches!(
        host.recognition_source(block, None),
        Err(Error::Denied)
    ));
    let response = host
        .request(
            &serde_json::to_string(&Request {
                version: 1,
                command: Command::Proposal {
                    proposal: proposal.id,
                },
            })
            .unwrap(),
        )
        .unwrap();
    assert!(!response.ok);
    assert!(response.data.is_none());
    assert!(response.generation.is_none());
    host.authenticate(
        [Scope::Block(block)].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    call(
        &host,
        Command::RejectProposal {
            proposal: proposal.id,
        },
    );
    assert_eq!(
        call(
            &host,
            Command::Proposal {
                proposal: proposal.id
            }
        )["state"],
        "Rejected"
    );
}
#[test]
fn cancelled_stale_unscoped_and_forged_results_cannot_publish() {
    let (_root, host, _, block) = fixture();
    let manager = manager_with(Box::new(Model));
    let control = JobControl::new(Duration::from_secs(30)).unwrap();
    let completed = host
        .recognition_source(block, None)
        .unwrap()
        .prepare(&manager, Task::Prose, None)
        .unwrap()
        .run(&control)
        .unwrap();
    control.cancel();
    assert!(matches!(
        host.publish_recognition(completed),
        Err(Error::Cancelled)
    ));
    let stale = complete(&host, &manager, block);
    let original = call(&host, Command::SourceInk { block });
    call(
        &host,
        Command::Edit {
            block,
            revision: 0,
            content: Content::Text("owner's newer edit".into()),
        },
    );
    assert!(matches!(
        host.publish_recognition(stale),
        Err(Error::Conflict)
    ));
    assert_eq!(call(&host, Command::SourceInk { block }), original);
    assert_eq!(
        call(&host, Command::Block { block })["content"]["value"],
        "owner's newer edit"
    );
    let mut raw = Store::memory().unwrap();
    let access = stem_core::security::Access::public();
    let note = raw.create(Kind::Note, None, "raw", &access).unwrap();
    let other = raw.append_ink(note.id, vec![stroke(0)], &access).unwrap();
    let unscoped = manager
        .recognise(
            raw.ink_snapshot(other.id, &access).unwrap(),
            Task::Prose,
            None,
            &JobControl::new(Duration::from_secs(30)).unwrap(),
        )
        .unwrap();
    assert!(matches!(
        host.publish_recognition(unscoped),
        Err(Error::Denied)
    ));
    assert!(host
        .submit(r#"{"version":1,"command":{"op":"PublishRecognition","output":"forged"}}"#)
        .is_err());
}
#[test]
fn host_close_invalidates_prepared_work_while_pending_reviews_remain_explicit() {
    let (_root, host, _, block) = fixture();
    let manager = manager_with(Box::new(Model));
    let work = host
        .recognition_source(block, None)
        .unwrap()
        .prepare(&manager, Task::Prose, None)
        .unwrap();
    host.close().unwrap();
    assert!(matches!(
        work.run(&JobControl::new(Duration::from_secs(30)).unwrap()),
        Err(Error::Cancelled)
    ));
}

#[test]
fn processing_admission_counts_unrun_sources_and_close_retires_them_without_waiting_for_callers() {
    let (_root, host, note, block) = fixture();
    let mut sources = (0..8)
        .map(|_| host.recognition_source(block, None).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(host.recognition_status().reserved, 8);
    assert!(matches!(
        host.recognition_source(block, None),
        Err(Error::ResourceLimit)
    ));
    call(
        &host,
        Command::Append {
            entity: note,
            content: Content::Text("capture admission remains available".into()),
        },
    );
    drop(sources.pop());
    assert_eq!(host.recognition_status().reserved, 7);
    sources.push(host.recognition_source(block, None).unwrap());
    host.close().unwrap();
    assert_eq!(host.recognition_status().reserved, 0);
    assert!(host.recognition_status().closed);
    let manager = manager_with(Box::new(Model));
    assert!(matches!(
        sources.pop().unwrap().prepare(&manager, Task::Prose, None),
        Err(Error::Cancelled)
    ));
}
#[test]
fn encoded_source_byte_budget_is_independent_of_job_count_and_does_not_evict_input() {
    let root = tempfile::tempdir().unwrap();
    let host = Host::open(root.path().join("library.db"), 8).unwrap();
    let note = Id::parse(
        call(
            &host,
            Command::Create {
                kind: Kind::Note,
                parent: None,
                title: "byte quota".into(),
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let mut original = stroke(0);
    original.points = vec![original.points[0].clone(); 50_000];
    let block = Id::parse(
        call(
            &host,
            Command::Capture {
                ticket: Id::new(),
                entity: note,
                strokes: vec![original],
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let mut sources = Vec::new();
    loop {
        match host.recognition_source(block, None) {
            Ok(source) => sources.push(source),
            Err(Error::ResourceLimit) => break,
            Err(error) => panic!("Unexpected source error: {error}"),
        }
    }
    assert!(sources.len() < 8);
    assert!(!sources.is_empty());
    let status = host.recognition_status();
    assert_eq!(status.reserved, sources.len());
    assert_eq!(
        call(&host, Command::SourceInk { block })[0]["points"]
            .as_array()
            .unwrap()
            .len(),
        50_000
    );
    drop(sources);
    assert_eq!(host.recognition_status().reserved, 0);
}
#[test]
fn shutdown_keeps_library_ownership_until_blocked_worker_really_retires() {
    let (root, host, _, block) = fixture();
    let (started, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let manager = manager_with(Box::new(Blocking {
        started,
        release: Mutex::new(blocked),
        cancelled,
    }));
    let work = host
        .recognition_source(block, None)
        .unwrap()
        .prepare(&manager, Task::Prose, None)
        .unwrap();
    let worker =
        std::thread::spawn(move || work.run(&JobControl::new(Duration::from_secs(30)).unwrap()));
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let closing = host.clone();
    let (done, closed) = mpsc::channel();
    let closer = std::thread::spawn(move || {
        let result = closing.close();
        done.send(result).unwrap();
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !host.recognition_status().closed {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(matches!(
        closed.recv_timeout(Duration::from_millis(50)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(Host::open(root.path().join("library.db"), 8).is_err());
    release.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    closed
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    closer.join().unwrap();
    assert!(Host::open(root.path().join("library.db"), 8).is_ok());
}
#[test]
fn reentrant_close_from_model_callback_cancels_without_self_join() {
    struct Closing {
        host: Arc<Host>,
        observed: Arc<AtomicBool>,
    }
    impl Recogniser for Closing {
        fn probe(&self) -> Result<()> {
            Ok(())
        }
        fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
            self.observed.store(
                matches!(self.host.close(), Err(Error::WorkerFailed)),
                Ordering::Release,
            );
            Ok(output())
        }
    }
    let (_root, host, _, block) = fixture();
    let observed = Arc::new(AtomicBool::new(false));
    let manager = manager_with(Box::new(Closing {
        host: host.clone(),
        observed: observed.clone(),
    }));
    let work = host
        .recognition_source(block, None)
        .unwrap()
        .prepare(&manager, Task::Prose, None)
        .unwrap();
    assert!(matches!(
        std::thread::spawn(move || work.run(&JobControl::new(Duration::from_secs(30)).unwrap()))
            .join()
            .unwrap(),
        Err(Error::Cancelled)
    ));
    assert!(observed.load(Ordering::Acquire));
    host.close().unwrap();
}
#[test]
fn idle_runtime_cleanup_is_off_writer_and_reentrant_close_never_releases_ownership_early() {
    struct ClosingDrop {
        host: Arc<Host>,
        observed: Arc<AtomicBool>,
        cleanup_thread: mpsc::Sender<std::thread::ThreadId>,
    }
    impl Drop for ClosingDrop {
        fn drop(&mut self) {
            self.cleanup_thread
                .send(std::thread::current().id())
                .unwrap();
            self.observed.store(
                matches!(self.host.close(), Err(Error::WorkerFailed)),
                Ordering::Release,
            );
        }
    }
    impl Recogniser for ClosingDrop {
        fn probe(&self) -> Result<()> {
            Ok(())
        }
        fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
            unreachable!()
        }
    }
    let (root, host, _, block) = fixture();
    let observed = Arc::new(AtomicBool::new(false));
    struct ThreadAuth(mpsc::Sender<std::thread::ThreadId>);
    impl stem_core::security::Authenticator for ThreadAuth {
        fn verify(&mut self, _: Id, _: &std::collections::BTreeSet<Scope>) -> bool {
            self.0.send(std::thread::current().id()).unwrap();
            true
        }
    }
    let (writer_sender, writer_thread) = mpsc::channel();
    host.authenticate(
        [Scope::App].into_iter().collect(),
        Box::new(ThreadAuth(writer_sender)),
    )
    .unwrap();
    let writer_thread = writer_thread.recv_timeout(Duration::from_secs(5)).unwrap();
    let (cleanup_sender, cleanup_thread) = mpsc::channel();
    let mut manager = manager_with(Box::new(ClosingDrop {
        host: host.clone(),
        observed: observed.clone(),
        cleanup_thread: cleanup_sender,
    }));
    let work = host
        .recognition_source(block, None)
        .unwrap()
        .prepare(&manager, Task::Prose, None)
        .unwrap();
    manager.remove("synthetic").unwrap();
    host.close().unwrap();
    assert!(observed.load(Ordering::Acquire));
    assert_ne!(
        cleanup_thread.recv_timeout(Duration::from_secs(5)).unwrap(),
        writer_thread
    );
    assert_eq!(host.recognition_status().reserved, 0);
    assert!(matches!(
        work.run(&JobControl::new(Duration::from_secs(30)).unwrap()),
        Err(Error::Cancelled)
    ));
    assert!(Host::open(root.path().join("library.db"), 8).is_ok());
}

#[test]
fn selected_regions_publish_independent_reviews_and_explicit_batch_acceptance_is_atomic() {
    struct RegionModel;
    impl Recogniser for RegionModel {
        fn probe(&self) -> Result<()> {
            Ok(())
        }
        fn recognise(
            &self,
            snapshot: &InkSnapshot,
            _: Task,
            _: &JobControl,
        ) -> Result<RecognitionOutput> {
            assert_eq!(snapshot.strokes().len(), 1);
            Ok(output())
        }
    }
    let (_root, host, note, _) = fixture();
    let first = stroke(0);
    let second = stroke(1);
    let block = Id::parse(
        call(
            &host,
            Command::Capture {
                ticket: Id::new(),
                entity: note,
                strokes: vec![first.clone(), second.clone()],
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let partition = call(
        &host,
        Command::Partition {
            block,
            revision: 0,
            groups: vec![vec![first.id], vec![second.id]],
            confirmed: true,
        },
    );
    let regions = partition["regions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|region| Id::parse(region["id"].as_str().unwrap()).unwrap())
        .collect::<Vec<_>>();
    let original = call(&host, Command::SourceInk { block });
    let manager = manager_with(Box::new(RegionModel));
    let mut selected = Vec::new();
    for region in regions {
        let source = host.recognition_source(block, Some(region)).unwrap();
        let ticket = source.ticket();
        let completed = source
            .prepare(&manager, Task::Prose, None)
            .unwrap()
            .run(&JobControl::new(Duration::from_secs(30)).unwrap())
            .unwrap();
        let proposal = host.publish_recognition(completed).unwrap();
        assert_eq!(proposal.id, ticket);
        assert_eq!(proposal.region, Some(region));
        selected.push((proposal.id, 0));
    }
    let denied = host
        .request(
            &serde_json::to_string(&Request {
                version: 1,
                command: Command::AcceptProposals {
                    selections: selected.clone(),
                    confirmed: false,
                },
            })
            .unwrap(),
        )
        .unwrap();
    assert!(!denied.ok);
    assert!(call(&host, Command::Block { block })["regions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|region| region["content"].is_null()));
    call(
        &host,
        Command::AcceptProposals {
            selections: selected.clone(),
            confirmed: true,
        },
    );
    for (proposal, _) in selected {
        assert_eq!(
            call(&host, Command::Proposal { proposal })["state"],
            "Accepted"
        );
    }
    assert_eq!(call(&host, Command::Block { block })["revision"], 2);
    call(&host, Command::Undo { entity: note });
    assert!(call(&host, Command::Block { block })["regions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|region| region["content"].is_null()));
    call(&host, Command::Redo { entity: note });
    assert!(call(&host, Command::Block { block })["regions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|region| !region["content"].is_null()));
    assert_eq!(call(&host, Command::SourceInk { block }), original);
}

#[test]
fn batch_acceptance_spanning_documents_records_independent_document_undo() {
    let (_root, host, first_note, first_block) = fixture();
    let manager = manager_with(Box::new(Model));
    let second_note = Id::parse(
        call(
            &host,
            Command::Create {
                kind: Kind::Note,
                parent: None,
                title: "second owner".into(),
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let second_block = Id::parse(
        call(
            &host,
            Command::Capture {
                ticket: Id::new(),
                entity: second_note,
                strokes: vec![stroke(1)],
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let first = host
        .publish_recognition(complete(&host, &manager, first_block))
        .unwrap();
    let second = host
        .publish_recognition(complete(&host, &manager, second_block))
        .unwrap();
    call(
        &host,
        Command::AcceptProposals {
            selections: vec![(first.id, 0), (second.id, 0)],
            confirmed: true,
        },
    );
    call(&host, Command::Undo { entity: first_note });
    assert_eq!(
        call(&host, Command::Block { block: first_block })["content"]["type"],
        "Ink"
    );
    assert_eq!(
        call(
            &host,
            Command::Block {
                block: second_block
            }
        )["content"]["value"],
        "1 + 1 = 3"
    );
    call(&host, Command::Redo { entity: first_note });
    call(
        &host,
        Command::Undo {
            entity: second_note,
        },
    );
    assert_eq!(
        call(&host, Command::Block { block: first_block })["content"]["value"],
        "1 + 1 = 3"
    );
    assert_eq!(
        call(
            &host,
            Command::Block {
                block: second_block
            }
        )["content"]["type"],
        "Ink"
    );
}
#[test]
fn overlapping_region_or_whole_block_selections_leave_every_review_pending() {
    let (_root, host, _, block) = fixture();
    let manager = manager_with(Box::new(Model));
    let stroke = call(&host, Command::SourceInk { block })[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let partition = call(
        &host,
        Command::Partition {
            block,
            revision: 0,
            groups: vec![vec![Id::parse(&stroke).unwrap()]],
            confirmed: true,
        },
    );
    let region = Id::parse(partition["regions"][0]["id"].as_str().unwrap()).unwrap();
    let mut proposals = Vec::new();
    for _ in 0..2 {
        let completed = host
            .recognition_source(block, Some(region))
            .unwrap()
            .prepare(&manager, Task::Prose, None)
            .unwrap()
            .run(&JobControl::new(Duration::from_secs(30)).unwrap())
            .unwrap();
        proposals.push(host.publish_recognition(completed).unwrap().id);
    }
    let whole = host
        .publish_recognition(complete(&host, &manager, block))
        .unwrap()
        .id;
    for selections in [
        vec![(proposals[0], 0), (proposals[1], 0)],
        vec![(whole, 0), (proposals[0], 0)],
        vec![(proposals[0], 0), (whole, 0)],
    ] {
        let response = host
            .request(
                &serde_json::to_string(&Request {
                    version: 1,
                    command: Command::AcceptProposals {
                        selections,
                        confirmed: true,
                    },
                })
                .unwrap(),
            )
            .unwrap();
        assert!(!response.ok);
    }
    for proposal in proposals.into_iter().chain([whole]) {
        assert_eq!(
            call(&host, Command::Proposal { proposal })["state"],
            "Pending"
        );
    }
    assert!(call(&host, Command::Block { block })["regions"][0]["content"].is_null());
}

#[test]
fn pending_reviews_are_discoverable_after_restart_without_a_loaded_model_and_hide_locked_source() {
    let (root, host, _, block) = fixture();
    let manager = manager_with(Box::new(Model));
    let first = host
        .publish_recognition(complete(&host, &manager, block))
        .unwrap();
    let second = host
        .publish_recognition(complete(&host, &manager, block))
        .unwrap();
    host.close().unwrap();
    drop(manager);
    let reopened = Host::open(root.path().join("library.db"), 8).unwrap();
    let page = call(
        &reopened,
        Command::Proposals {
            block,
            offset: 0,
            limit: 1,
        },
    );
    let next = call(
        &reopened,
        Command::Proposals {
            block,
            offset: 1,
            limit: 1,
        },
    );
    let ids = [
        Id::parse(page[0]["id"].as_str().unwrap()).unwrap(),
        Id::parse(next[0]["id"].as_str().unwrap()).unwrap(),
    ];
    assert!(ids.contains(&first.id) && ids.contains(&second.id));
    assert_ne!(ids[0], ids[1]);
    assert!(page[0].get("output").is_none());
    assert_eq!(page[0]["candidates"], 1);
    assert!(call(
        &reopened,
        Command::Proposals {
            block,
            offset: 2,
            limit: 1
        }
    )
    .as_array()
    .unwrap()
    .is_empty());
    reopened
        .authenticate(
            [Scope::Block(block)].into_iter().collect(),
            Box::new(TestAuth(true)),
        )
        .unwrap();
    call(
        &reopened,
        Command::SetBlockLock {
            block,
            revision: 0,
            locked: true,
        },
    );
    call(&reopened, Command::Relock {});
    let hidden = reopened
        .request(
            &serde_json::to_string(&Request {
                version: 1,
                command: Command::Proposals {
                    block,
                    offset: 0,
                    limit: 10,
                },
            })
            .unwrap(),
        )
        .unwrap();
    assert!(!hidden.ok);
    assert!(hidden.data.is_none());
}
