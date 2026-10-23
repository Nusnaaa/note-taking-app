mod common;
use common::*;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use stem_core::{
    content::Content,
    converters::TextConverter,
    processing::*,
    provisioning::{resource_digest, ResourceLease, ResourceLeases},
    security::Access,
    store::{InkSnapshot, Kind, Store},
    Error, Result,
};

fn descriptor(id: &str, family: Family) -> Descriptor {
    let recogniser = family == Family::Recogniser;
    Descriptor {
        id: id.into(),
        version: "synthetic-1".into(),
        adapter_version: "1".into(),
        family,
        input: if recogniser {
            InputContract::OrderedStrokesV1
        } else {
            InputContract::EditableV1
        },
        tasks: if recogniser {
            vec![Task::Prose]
        } else {
            vec![]
        },
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some(
            "app-owned synthetic work fixture; no recognition quality claim".into(),
        ),
        redistributable_resources: false,
        resource_sha256: None,
        runtime_contract: "stem-processing-v1".into(),
    }
}
fn snapshot() -> (Store, InkSnapshot) {
    let mut store = Store::memory().unwrap();
    let public = Access::public();
    let note = store
        .create(Kind::Note, None, "work fixture", &public)
        .unwrap();
    let block = store.append_ink(note.id, vec![stroke(0)], &public).unwrap();
    let snapshot = store.ink_snapshot(block.id, &public).unwrap();
    (store, snapshot)
}
struct Immediate;
impl Recogniser for Immediate {
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
        Ok(RecognitionOutput {
            candidates: vec![Content::Text("1 + 1 = 3".into())],
            raw_output: "1 + 1 = 3".into(),
            confidence: None,
            diagnostics: vec![],
        })
    }
}
fn manager() -> Manager {
    let mut manager = Manager::new();
    manager
        .register_recogniser(descriptor("first", Family::Recogniser), Box::new(Immediate))
        .unwrap();
    manager.activate_local("first", None, true).unwrap();
    manager.select(Task::Prose, "first").unwrap();
    manager
}
#[test]
fn prepared_work_freezes_engine_choice_and_keeps_explicit_proposal_acceptance() {
    let (mut store, snapshot) = snapshot();
    let public = Access::public();
    let block = snapshot.block();
    let original = snapshot.strokes().to_vec();
    let mut manager = manager();
    let work = manager
        .prepare_recognition(snapshot, Task::Prose, None)
        .unwrap();
    manager
        .register_recogniser(
            descriptor("second", Family::Recogniser),
            Box::new(Immediate),
        )
        .unwrap();
    manager.activate_local("second", None, true).unwrap();
    manager.select(Task::Prose, "second").unwrap();
    assert_eq!(work.descriptor().id, "first");
    let completed =
        std::thread::spawn(move || work.run(&JobControl::new(Duration::from_secs(5)).unwrap()))
            .join()
            .unwrap()
            .unwrap();
    let proposal = store.propose(completed, &public).unwrap();
    assert_eq!(proposal.engine.id, "first");
    assert_eq!(store.block(block, &public).unwrap().content, Content::Ink);
    store.accept_proposal(proposal.id, 0, &public).unwrap();
    assert_eq!(
        store.block(block, &public).unwrap().content,
        Content::Text("1 + 1 = 3".into())
    );
    assert_eq!(store.source_ink(block, &public).unwrap(), original);
}
#[test]
fn removal_failed_reverification_and_manager_drop_invalidate_prepared_work() {
    for operation in 0..3 {
        let (_store, snapshot) = snapshot();
        let mut manager = manager();
        let work = manager
            .prepare_recognition(snapshot, Task::Prose, None)
            .unwrap();
        match operation {
            0 => manager.remove("first").unwrap(),
            1 => assert!(manager
                .activate_local("first", Some(b"unexpected"), true)
                .is_err()),
            _ => drop(manager),
        }
        assert!(matches!(
            work.run(&JobControl::new(Duration::from_secs(5)).unwrap()),
            Err(Error::Cancelled)
        ));
    }
}
struct Blocking {
    started: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    observed: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
    _resource: ResourceLease,
}
impl Drop for Blocking {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
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
        self.observed.store(
            matches!(control.check(), Err(Error::Cancelled)),
            Ordering::Release,
        );
        // Deliberately ignore cooperative cancellation: the framework must still reject this late result.
        Ok(RecognitionOutput {
            candidates: vec![Content::Text("late".into())],
            raw_output: "late".into(),
            confidence: None,
            diagnostics: vec![],
        })
    }
}
#[test]
fn running_work_observes_removal_and_pins_resources_until_actual_worker_retirement() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("synthetic.resource");
    std::fs::write(&path, b"resource").unwrap();
    let pool = ResourceLeases::default();
    let resource = pool.acquire(&path, &resource_digest(b"resource")).unwrap();
    let (started, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let observed = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let mut manager = Manager::new();
    let mut descriptor = descriptor("blocking", Family::Recogniser);
    descriptor.resource_sha256 = Some(resource_digest(b"resource"));
    manager
        .register_recogniser(
            descriptor,
            Box::new(Blocking {
                started,
                release: Mutex::new(blocked),
                observed: observed.clone(),
                dropped: dropped.clone(),
                _resource: resource,
            }),
        )
        .unwrap();
    manager
        .activate_local("blocking", Some(b"resource"), true)
        .unwrap();
    let (_store, snapshot) = snapshot();
    let work = manager
        .prepare_recognition(snapshot, Task::Prose, Some("blocking"))
        .unwrap();
    let thread =
        std::thread::spawn(move || work.run(&JobControl::new(Duration::from_secs(5)).unwrap()));
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    manager.remove("blocking").unwrap();
    assert!(!dropped.load(Ordering::Acquire));
    assert!(matches!(pool.remove(&path), Err(Error::Conflict)));
    release.send(()).unwrap();
    assert!(matches!(thread.join().unwrap(), Err(Error::Cancelled)));
    assert!(observed.load(Ordering::Acquire));
    assert!(dropped.load(Ordering::Acquire));
    pool.remove(&path).unwrap();
}
#[test]
fn converter_work_is_owned_cancelled_by_retirement_and_contains_format_panics() {
    let mut manager = Manager::new();
    manager
        .register_converter(
            descriptor("text", Family::Converter),
            Box::new(TextConverter),
        )
        .unwrap();
    manager.activate_local("text", None, true).unwrap();
    let content = Content::Code {
        text: "\t001\r\n".into(),
        language: None,
    };
    let work = manager
        .prepare_conversion("text", content.clone(), Format::PlainText)
        .unwrap();
    assert_eq!(work.descriptor().id, "text");
    assert_eq!(
        std::thread::spawn(move || work.run(&JobControl::new(Duration::from_secs(5)).unwrap()))
            .join()
            .unwrap()
            .unwrap()
            .text,
        "\t001\r\n"
    );
    let cancelled = manager
        .prepare_conversion("text", content, Format::PlainText)
        .unwrap();
    manager.remove("text").unwrap();
    assert!(matches!(
        cancelled.run(&JobControl::new(Duration::from_secs(5)).unwrap()),
        Err(Error::Cancelled)
    ));
    struct Broken;
    impl Converter for Broken {
        fn formats(&self) -> Vec<Format> {
            panic!("synthetic formats panic")
        }
        fn convert(&self, _: &Content, _: Format, _: &JobControl) -> Result<Converted> {
            unreachable!()
        }
    }
    manager
        .register_converter(descriptor("broken", Family::Converter), Box::new(Broken))
        .unwrap();
    manager.activate_local("broken", None, true).unwrap();
    assert!(matches!(
        manager
            .prepare_conversion("broken", Content::Text("literal".into()), Format::PlainText)
            .unwrap()
            .run(&JobControl::new(Duration::from_secs(5)).unwrap()),
        Err(Error::WorkerFailed)
    ));
}
