mod common;
use std::{
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    sync::{mpsc, Mutex},
    thread,
    time::Duration,
};
use stem_core::{
    processing::*,
    provisioning::{resource_digest, ResourceLeases},
    security::Access,
    store::*,
    Error, Result,
};
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(30)).unwrap()
}
fn descriptor() -> Descriptor {
    Descriptor {
        id: "synthetic-pin".into(),
        version: "1".into(),
        adapter_version: "1".into(),
        family: Family::Recogniser,
        input: InputContract::OrderedStrokesV1,
        tasks: vec![Task::Prose],
        internals: Internals::StrokeOnlyVerified,
        offline: true,
        content_egress: false,
        personal_use_evidence: Some("app-owned lifetime fixture; no real SDK".into()),
        redistributable_resources: false,
        resource_sha256: Some(resource_digest(b"model")),
        runtime_contract: "stem-processing-v1".into(),
    }
}
fn setup() -> (tempfile::TempDir, Store, EngineVersion, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let v = s
        .stage_engine(
            descriptor(),
            Some(&mut std::io::Cursor::new(b"model")),
            root.path(),
            5,
            true,
            &Access::public(),
        )
        .unwrap();
    let path = PathBuf::from(v.local_resource.as_ref().unwrap());
    (root, s, v, path)
}
fn blocked(path: &Path) {
    assert!(matches!(
        ResourceLeases::default().remove(path),
        Err(Error::Conflict)
    ));
    assert!(path.exists());
}
struct Model;
impl Recogniser for Model {
    fn probe(&self) -> Result<()> {
        Ok(())
    }
    fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
        Err(Error::EngineUnavailable)
    }
}
#[test]
fn factory_candidate_ready_and_detached_owners_pin_the_actual_resource_across_registries() {
    let (_root, mut s, v, path) = setup();
    let mut manager = Manager::new();
    let mut prepared = s
        .engine_activation_plan(v.id, &Access::public())
        .unwrap()
        .prepare_recogniser_with(5, &control(), |_, bytes, _| {
            assert_eq!(bytes.unwrap(), b"model");
            blocked(&path);
            Ok(Box::new(Model))
        })
        .unwrap();
    blocked(&path);
    assert!(s
        .commit_engine_activation(&mut prepared, &mut manager, &Access::public())
        .unwrap()
        .is_none());
    drop(prepared);
    blocked(&path);
    let detached = manager.detach("synthetic-pin").unwrap();
    blocked(&path);
    drop(detached);
    ResourceLeases::default().remove(&path).unwrap();
    assert!(!path.exists());
}
#[test]
fn independent_resource_registries_and_nested_readers_share_one_deletion_reservation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("resource");
    std::fs::write(&path, b"model").unwrap();
    let pool = ResourceLeases::default();
    let one = pool.acquire(&path, &resource_digest(b"model")).unwrap();
    let two = one.file().unwrap().file().unwrap();
    drop(one);
    blocked(&path);
    let another = ResourceLeases::default()
        .acquire(&path, &resource_digest(b"model"))
        .unwrap();
    drop(two);
    blocked(&path);
    drop(another);
    pool.remove(&path).unwrap();
}
struct BlockingDrop {
    entered: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    panic: bool,
}
impl Recogniser for BlockingDrop {
    fn probe(&self) -> Result<()> {
        Ok(())
    }
    fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
        Err(Error::EngineUnavailable)
    }
}
impl Drop for BlockingDrop {
    fn drop(&mut self) {
        self.entered.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        assert!(!self.panic, "synthetic retirement failure");
    }
}
#[test]
fn reservation_survives_actual_sdk_destructor_and_retirement_failure_is_fail_closed() {
    for fail in [false, true] {
        let (_root, s, v, path) = setup();
        let (tx, entered) = mpsc::channel();
        let (release, rx) = mpsc::channel();
        let prepared = s
            .engine_activation_plan(v.id, &Access::public())
            .unwrap()
            .prepare_recogniser(
                Box::new(BlockingDrop {
                    entered: tx,
                    release: Mutex::new(rx),
                    panic: fail,
                }),
                5,
                &control(),
            )
            .unwrap();
        let worker = thread::spawn(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(prepared))).is_err()
        });
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        blocked(&path);
        release.send(()).unwrap();
        assert_eq!(worker.join().unwrap(), fail);
        if fail {
            blocked(&path);
            assert!(std::fs::read_dir(path.parent().unwrap())
                .unwrap()
                .any(|entry| {
                    let entry = entry.unwrap();
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".stem-resource-")
                        && entry.metadata().unwrap().len() > 0
                }));
        } else {
            ResourceLeases::default().remove(&path).unwrap();
        }
    }
}
#[test]
fn in_flight_inference_owns_the_resource_after_logical_removal_until_real_worker_return() {
    struct Running {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl Recogniser for Running {
        fn probe(&self) -> Result<()> {
            Ok(())
        }
        fn recognise(
            &self,
            _: &InkSnapshot,
            _: Task,
            control: &JobControl,
        ) -> Result<RecognitionOutput> {
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            control.check()?;
            Err(Error::EngineUnavailable)
        }
    }
    let (_root, mut s, v, path) = setup();
    let (tx, entered) = mpsc::channel();
    let (release, rx) = mpsc::channel();
    let mut manager = Manager::new();
    s.activate_engine(
        v.id,
        &mut manager,
        Box::new(Running {
            entered: tx,
            release: Mutex::new(rx),
        }),
        5,
        &Access::public(),
    )
    .unwrap();
    let note = s
        .create(Kind::Note, None, "independent ink", &Access::public())
        .unwrap();
    let block = s
        .append_ink(note.id, vec![common::stroke(0)], &Access::public())
        .unwrap();
    let work = manager
        .prepare_recognition(
            s.ink_snapshot(block.id, &Access::public()).unwrap(),
            Task::Prose,
            Some("synthetic-pin"),
        )
        .unwrap();
    let worker = thread::spawn(move || work.run(&control()));
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let detached = manager.detach("synthetic-pin").unwrap();
    drop(detached);
    blocked(&path);
    release.send(()).unwrap();
    drop(worker.join().unwrap());
    ResourceLeases::default().remove(&path).unwrap();
}
#[test]
fn failed_resource_loading_drops_a_captured_provider_before_its_reservation() {
    let (_root, s, v, path) = setup();
    let (tx, entered) = mpsc::channel();
    let (release, rx) = mpsc::channel();
    let plan = s.engine_activation_plan(v.id, &Access::public()).unwrap();
    let worker = thread::spawn(move || {
        plan.prepare_recogniser(
            Box::new(BlockingDrop {
                entered: tx,
                release: Mutex::new(rx),
                panic: false,
            }),
            1,
            &control(),
        )
        .err()
        .unwrap()
    });
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    blocked(&path);
    release.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Error::ResourceLimit));
    ResourceLeases::default().remove(&path).unwrap();
}
#[test]
fn factory_error_payload_retires_while_the_resource_is_still_reserved() {
    struct ErrorOwner {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl std::fmt::Debug for ErrorOwner {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("synthetic error owner")
        }
    }
    impl std::fmt::Display for ErrorOwner {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("synthetic error")
        }
    }
    impl std::error::Error for ErrorOwner {}
    impl Drop for ErrorOwner {
        fn drop(&mut self) {
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }
    }
    let (_root, s, v, path) = setup();
    let (tx, entered) = mpsc::channel();
    let (release, rx) = mpsc::channel();
    let plan = s.engine_activation_plan(v.id, &Access::public()).unwrap();
    let worker = thread::spawn(move || {
        plan.prepare_recogniser_with(5, &control(), |_, _, _| {
            Err(Error::Io(std::io::Error::other(ErrorOwner {
                entered: tx,
                release: Mutex::new(rx),
            })))
        })
        .err()
        .unwrap()
    });
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    blocked(&path);
    release.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Error::Io(_)));
    ResourceLeases::default().remove(&path).unwrap();
}
#[test]
fn shared_reservations_prevent_deletion_in_a_separate_process() {
    const CHILD: &str = "STEM_RESOURCE_PIN_CHILD_CASE";
    if let Some(path) = std::env::var_os(CHILD) {
        let _lease = ResourceLeases::default()
            .acquire(Path::new(&path), &resource_digest(b"model"))
            .unwrap();
        println!("resource-child-ready");
        std::io::stdout().flush().unwrap();
        let mut byte = [0];
        std::io::stdin().read_exact(&mut byte).unwrap();
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("resource");
    std::fs::write(&path, b"model").unwrap();
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Child(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "shared_reservations_prevent_deletion_in_a_separate_process",
                "--nocapture",
            ])
            .env(CHILD, &path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let output = child.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut ready = false;
        for line in std::io::BufReader::new(output).lines() {
            if line.unwrap().contains("resource-child-ready") && !ready {
                tx.send(()).unwrap();
                ready = true;
            }
        }
    });
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    blocked(&path);
    child.0.stdin.take().unwrap().write_all(b"x").unwrap();
    assert!(child.0.wait().unwrap().success());
    reader.join().unwrap();
    ResourceLeases::default().remove(&path).unwrap();
}

#[test]
fn converter_jobs_keep_the_resource_after_the_ready_owner_is_detached() {
    struct Convert;
    impl Converter for Convert {
        fn formats(&self) -> Vec<Format> {
            vec![Format::PlainText]
        }
        fn convert(
            &self,
            _: &stem_core::content::Content,
            _: Format,
            _: &JobControl,
        ) -> Result<Converted> {
            Err(Error::EngineUnavailable)
        }
    }
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::memory().unwrap();
    let mut d = descriptor();
    d.family = Family::Converter;
    d.input = InputContract::EditableV1;
    d.tasks.clear();
    let v = s
        .stage_engine(
            d,
            Some(&mut std::io::Cursor::new(b"model")),
            root.path(),
            5,
            true,
            &Access::public(),
        )
        .unwrap();
    let path = PathBuf::from(v.local_resource.as_ref().unwrap());
    let mut prepared = s
        .engine_activation_plan(v.id, &Access::public())
        .unwrap()
        .prepare_converter_with(5, &control(), |_, _, _| {
            blocked(&path);
            Ok(Box::new(Convert))
        })
        .unwrap();
    let mut manager = Manager::new();
    s.commit_engine_activation(&mut prepared, &mut manager, &Access::public())
        .unwrap();
    drop(prepared);
    let work = manager
        .prepare_conversion(
            "synthetic-pin",
            stem_core::content::Content::Text("001".into()),
            Format::PlainText,
        )
        .unwrap();
    drop(manager.detach("synthetic-pin").unwrap());
    blocked(&path);
    drop(work);
    ResourceLeases::default().remove(&path).unwrap();
}
#[test]
fn resource_backed_factory_panic_retains_a_failed_reservation_and_cannot_silently_retry() {
    let (_root, s, v, path) = setup();
    assert!(matches!(
        s.engine_activation_plan(v.id, &Access::public())
            .unwrap()
            .prepare_recogniser_with(5, &control(), |_, _, _| panic!(
                "synthetic provider construction panic"
            )),
        Err(Error::WorkerFailed)
    ));
    blocked(&path);
    assert!(matches!(
        s.engine_activation_plan(v.id, &Access::public())
            .unwrap()
            .prepare_recogniser_with(5, &control(), |_, _, _| Ok(Box::new(Model))),
        Err(Error::ReviewRequired)
    ));
    assert_eq!(std::fs::read(path).unwrap(), b"model");
}

#[test]
fn inference_error_payload_finishes_before_the_last_sdk_reservation_retires() {
    struct Payload {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl std::fmt::Debug for Payload {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("synthetic callback payload")
        }
    }
    impl std::fmt::Display for Payload {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("synthetic callback error")
        }
    }
    impl std::error::Error for Payload {}
    impl Drop for Payload {
        fn drop(&mut self) {
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }
    }
    struct ErrorModel(Mutex<Option<Payload>>);
    impl Recogniser for ErrorModel {
        fn probe(&self) -> Result<()> {
            Ok(())
        }
        fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
            Err(Error::Io(std::io::Error::other(
                self.0.lock().unwrap().take().unwrap(),
            )))
        }
    }
    let (_root, mut s, v, path) = setup();
    let (tx, entered) = mpsc::channel();
    let (release, rx) = mpsc::channel();
    let mut manager = Manager::new();
    s.activate_engine(
        v.id,
        &mut manager,
        Box::new(ErrorModel(Mutex::new(Some(Payload {
            entered: tx,
            release: Mutex::new(rx),
        })))),
        5,
        &Access::public(),
    )
    .unwrap();
    let note = s
        .create(Kind::Note, None, "callback error source", &Access::public())
        .unwrap();
    let block = s
        .append_ink(note.id, vec![common::stroke(0)], &Access::public())
        .unwrap();
    let work = manager
        .prepare_recognition(
            s.ink_snapshot(block.id, &Access::public()).unwrap(),
            Task::Prose,
            Some("synthetic-pin"),
        )
        .unwrap();
    let worker = thread::spawn(move || work.run(&control()));
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    drop(manager.detach("synthetic-pin").unwrap());
    blocked(&path);
    release.send(()).unwrap();
    let safe_error = worker.join().unwrap().err().unwrap();
    assert!(matches!(safe_error, Error::Io(_)));
    ResourceLeases::default().remove(&path).unwrap();
    drop(safe_error);
}
