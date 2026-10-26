mod common;
use common::*;
use std::{
    collections::BTreeSet,
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};
use stem_core::{
    crypto::{self, Credential},
    host::{Command, Host, NativeExportSelection, Request},
    portable::{ExportProtection, NativeDocument},
    processing::JobControl,
    security::{Access, Authenticator, Scope},
    store::{Kind, Store},
    Error, Id,
};
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(60)).unwrap()
}
fn wire(command: Command) -> String {
    serde_json::to_string(&Request {
        version: 1,
        command,
    })
    .unwrap()
}
fn fixture() -> (tempfile::TempDir, Arc<Host>, Id, Id) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    let (note, block) = {
        let mut store = Store::open(&path).unwrap();
        let a = Access::public();
        let note = store.create(Kind::Note, None, "exported note", &a).unwrap();
        let block = store.append_ink(note.id, vec![stroke(0)], &a).unwrap();
        (note.id, block.id)
    };
    (root, Arc::new(Host::open(path, 8).unwrap()), note, block)
}
fn prepare(host: &Host, note: Id) -> stem_core::host::NativeExportReview {
    host.prepare_native_export(NativeExportSelection::Entities(vec![note]), true)
        .unwrap()
}
fn plaintext(
    host: &Host,
    review: stem_core::host::NativeExportReview,
) -> stem_core::host::CompletedNativeExport {
    host.encode_native_export(
        review,
        ExportProtection::Plaintext {
            acknowledge_protected: true,
        },
        &mut TestAuth(true),
        true,
        &control(),
    )
    .unwrap()
}
#[test]
fn native_host_export_roundtrips_observed_ink_with_single_snapshot_admission() {
    let (_root, host, note, block) = fixture();
    let review = prepare(&host, note);
    assert!(review.summary().includes_raw_ink);
    assert_eq!(review.summary().selected_blocks, 1);
    assert!(matches!(
        host.prepare_native_export(NativeExportSelection::Entities(vec![note]), true),
        Err(Error::ResourceLimit)
    ));
    let completed = plaintext(&host, review);
    assert!(matches!(
        host.prepare_native_export(NativeExportSelection::Entities(vec![note]), true),
        Err(Error::ResourceLimit)
    ));
    let output = host.finish_native_export(completed).unwrap();
    let document = NativeDocument::decode(&output.bytes).unwrap();
    assert_eq!(document.blocks[0].current.id, block);
    assert_eq!(document.blocks[0].strokes[0].points, stroke(0).points);
    drop(prepare(&host, note));
    drop(prepare(&host, note));
}
#[test]
fn native_host_release_refuses_stale_generation_and_foreign_host() {
    let (_root, host, note, _) = fixture();
    let complete = plaintext(&host, prepare(&host, note));
    host.request(&wire(Command::Create {
        kind: Kind::Note,
        parent: None,
        title: "later edit".into(),
    }))
    .unwrap();
    assert!(matches!(
        host.finish_native_export(complete),
        Err(Error::Conflict)
    ));
    let (_other_root, other, _, _) = fixture();
    assert!(matches!(
        other.encode_native_export(
            prepare(&host, note),
            ExportProtection::Plaintext {
                acknowledge_protected: true
            },
            &mut TestAuth(true),
            true,
            &control()
        ),
        Err(Error::Denied)
    ));
    assert!(matches!(
        other.finish_native_export(plaintext(&host, prepare(&host, note))),
        Err(Error::Denied)
    ));
    drop(prepare(&host, note));
}
#[test]
fn relock_cancel_and_close_retire_native_export_without_delivering_bytes() {
    let (_root, host, note, _) = fixture();
    let complete = plaintext(&host, prepare(&host, note));
    host.request(&wire(Command::Relock {})).unwrap();
    assert!(matches!(
        host.finish_native_export(complete),
        Err(Error::Cancelled)
    ));
    let review = prepare(&host, note);
    let stopped = control();
    stopped.cancel();
    assert!(matches!(
        host.encode_native_export(
            review,
            ExportProtection::Plaintext {
                acknowledge_protected: false
            },
            &mut TestAuth(true),
            true,
            &stopped
        ),
        Err(Error::Cancelled)
    ));
    let review = prepare(&host, note);
    host.close().unwrap();
    assert!(matches!(
        host.encode_native_export(
            review,
            ExportProtection::Plaintext {
                acknowledge_protected: false
            },
            &mut TestAuth(true),
            true,
            &control()
        ),
        Err(Error::Cancelled) | Err(Error::WorkerFailed)
    ));
}
#[test]
fn protected_native_export_requires_fresh_verification_and_plaintext_acknowledgment() {
    let (_root, host, note, _) = fixture();
    host.authenticate(
        [Scope::Entity(note)].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    assert!(
        host.request(&wire(Command::SetLock {
            entity: note,
            revision: 1,
            locked: true
        }))
        .unwrap()
        .ok
    );
    assert!(matches!(
        host.encode_native_export(
            prepare(&host, note),
            ExportProtection::Plaintext {
                acknowledge_protected: true
            },
            &mut TestAuth(false),
            true,
            &control()
        ),
        Err(Error::Denied)
    ));
    assert!(matches!(
        host.encode_native_export(
            prepare(&host, note),
            ExportProtection::Plaintext {
                acknowledge_protected: false
            },
            &mut TestAuth(true),
            true,
            &control()
        ),
        Err(Error::ReviewRequired)
    ));
    let complete = host
        .encode_native_export(
            prepare(&host, note),
            ExportProtection::Encrypted {
                password: "deliberate export encryption passphrase",
            },
            &mut TestAuth(true),
            true,
            &control(),
        )
        .unwrap();
    let output = host.finish_native_export(complete).unwrap();
    let plain = crypto::decrypt(
        &output.bytes,
        Credential::Password("deliberate export encryption passphrase"),
    )
    .unwrap();
    assert_eq!(NativeDocument::decode(&plain).unwrap().entities[0].id, note);
    assert!(output.recovery_key.is_some());
}
struct Reentrant {
    host: Arc<Host>,
    close: bool,
}
impl Authenticator for Reentrant {
    fn verify(&mut self, _: Id, _: &BTreeSet<Scope>) -> bool {
        assert_eq!(self.host.capture_library_identity().unwrap().len(), 36);
        if self.close {
            assert!(matches!(self.host.close(), Err(Error::WorkerFailed)));
        }
        true
    }
}
#[test]
fn native_export_verification_runs_off_writer_and_close_reentry_does_not_deadlock() {
    for close in [false, true] {
        let (_root, host, note, _) = fixture();
        host.authenticate(
            [Scope::Entity(note)].into_iter().collect(),
            Box::new(TestAuth(true)),
        )
        .unwrap();
        assert!(
            host.request(&wire(Command::SetLock {
                entity: note,
                revision: 1,
                locked: true
            }))
            .unwrap()
            .ok
        );
        let review = prepare(&host, note);
        let owner = Arc::clone(&host);
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let result = owner.encode_native_export(
                review,
                ExportProtection::Plaintext {
                    acknowledge_protected: true,
                },
                &mut Reentrant {
                    host: Arc::clone(&owner),
                    close,
                },
                true,
                &control(),
            );
            tx.send(result).unwrap();
        });
        let result = rx
            .recv_timeout(Duration::from_secs(30))
            .expect("native export verifier must not block writer or close");
        worker.join().unwrap();
        if close {
            assert!(matches!(result, Err(Error::Cancelled)));
            host.close().unwrap();
        } else {
            assert!(host.finish_native_export(result.unwrap()).is_ok());
        }
    }
}

struct Panics;
impl Authenticator for Panics {
    fn verify(&mut self, _: Id, _: &BTreeSet<Scope>) -> bool {
        panic!("synthetic verifier panic")
    }
}
#[test]
fn panicking_export_verifier_releases_admission_and_leaves_writer_usable() {
    let (_root, host, note, _) = fixture();
    host.authenticate(
        [Scope::Entity(note)].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    assert!(
        host.request(&wire(Command::SetLock {
            entity: note,
            revision: 1,
            locked: true
        }))
        .unwrap()
        .ok
    );
    assert!(matches!(
        host.encode_native_export(
            prepare(&host, note),
            ExportProtection::Plaintext {
                acknowledge_protected: true
            },
            &mut Panics,
            true,
            &control()
        ),
        Err(Error::WorkerFailed)
    ));
    assert!(host
        .finish_native_export(plaintext(&host, prepare(&host, note)))
        .is_ok());
}
struct Held {
    started: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
impl Authenticator for Held {
    fn verify(&mut self, _: Id, _: &BTreeSet<Scope>) -> bool {
        self.started.send(()).unwrap();
        self.release.recv().unwrap();
        true
    }
}
#[test]
fn external_close_waits_for_native_export_worker_then_refuses_release() {
    let (_root, host, note, _) = fixture();
    host.authenticate(
        [Scope::Entity(note)].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    assert!(
        host.request(&wire(Command::SetLock {
            entity: note,
            revision: 1,
            locked: true
        }))
        .unwrap()
        .ok
    );
    let review = prepare(&host, note);
    let (started, ready) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let exporting = Arc::clone(&host);
    let worker = thread::spawn(move || {
        exporting.encode_native_export(
            review,
            ExportProtection::Plaintext {
                acknowledge_protected: true,
            },
            &mut Held {
                started,
                release: held,
            },
            true,
            &control(),
        )
    });
    ready.recv_timeout(Duration::from_secs(30)).unwrap();
    let closing = Arc::clone(&host);
    let (closed, result) = mpsc::channel();
    let closer = thread::spawn(move || {
        closed.send(closing.close()).unwrap();
    });
    // Observe actual closed admission, not an elapsed-time assumption.
    let mut status = host.subscribe_status();
    loop {
        if status
            .next_timeout(Duration::from_secs(30))
            .unwrap()
            .expect("close must publish admission state")
            .closed
        {
            break;
        }
    }
    assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));
    release.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    result
        .recv_timeout(Duration::from_secs(30))
        .unwrap()
        .unwrap();
    closer.join().unwrap();
}

#[test]
fn exercise_native_host_export_keeps_only_selected_members_and_originals() {
    let (_root, host, note, block) = fixture();
    let exercise = host
        .request(&wire(Command::CreateExercise {
            master: note,
            title: "chosen exercise".into(),
            blocks: vec![block],
        }))
        .unwrap();
    assert!(exercise.ok);
    let exercise = Id::parse(exercise.data.unwrap()["id"].as_str().unwrap()).unwrap();
    let unrelated = host
        .request(&wire(Command::Append {
            entity: note,
            content: stem_core::content::Content::Text("unselected adjacent content".into()),
        }))
        .unwrap();
    assert!(unrelated.ok);
    let review = host
        .prepare_native_export(NativeExportSelection::Exercise(exercise), true)
        .unwrap();
    assert_eq!(review.summary().selected_blocks, 1);
    let output = host.finish_native_export(plaintext(&host, review)).unwrap();
    let document = NativeDocument::decode(&output.bytes).unwrap();
    assert_eq!(document.entities[0].title, "chosen exercise");
    assert_eq!(document.blocks.len(), 1);
    assert_eq!(document.blocks[0].current.id, block);
    assert_eq!(document.blocks[0].strokes[0].points, stroke(0).points);
    assert!(!String::from_utf8(output.bytes)
        .unwrap()
        .contains("unselected adjacent content"));
}

#[test]
fn cancellation_after_encoding_refuses_byte_release_and_retires_snapshot() {
    let (_root, host, note, _) = fixture();
    let control = control();
    let complete = host
        .encode_native_export(
            prepare(&host, note),
            ExportProtection::Plaintext {
                acknowledge_protected: true,
            },
            &mut TestAuth(true),
            true,
            &control,
        )
        .unwrap();
    control.cancel();
    assert!(matches!(
        host.finish_native_export(complete),
        Err(Error::Cancelled)
    ));
    assert!(host
        .finish_native_export(plaintext(&host, prepare(&host, note)))
        .is_ok());
}
