use sha2::Digest;
mod common;
use common::*;
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use stem_core::{
    crypto::{self, Credential},
    host::{Command, Host, NativeImportReview, Request},
    portable::{ExportProtection, NativeDocument},
    processing::JobControl,
    security::{Access, Authenticator, Scope},
    store::{Kind, Store},
    Error, Id,
};
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(180)).unwrap()
}
fn wire(command: Command) -> String {
    serde_json::to_string(&Request {
        version: 1,
        command,
    })
    .unwrap()
}
fn artifact(protected: bool) -> (Vec<u8>, Id) {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "source", &a).unwrap();
    s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    let key = access([Scope::Entity(n.id)]);
    if protected {
        let rev = s.entity(n.id, &a).unwrap().revision;
        s.set_lock(n.id, rev, true, &key).unwrap();
    }
    let p = s.prepare_native_export(&[n.id], true, &key).unwrap();
    (
        s.finish_native_export(
            p,
            ExportProtection::Plaintext {
                acknowledge_protected: true,
            },
            &mut TestAuth(true),
            true,
        )
        .unwrap()
        .bytes,
        n.id,
    )
}
fn fixture() -> (tempfile::TempDir, Arc<Host>) {
    let root = tempfile::tempdir().unwrap();
    let host = Arc::new(Host::open(root.path().join("library.db"), 8).unwrap());
    (root, host)
}
fn prepare(host: &Host, bytes: &[u8], c: &JobControl) -> NativeImportReview {
    host.prepare_native_import(host.native_import_source().unwrap(), bytes, None, c)
        .unwrap()
}
#[test]
fn native_import_remaps_ids_and_receipt_reconciles_discarded_response_after_reopen() {
    let (bytes, original) = artifact(false);
    let (root, host) = fixture();
    let source = host.native_import_source().unwrap();
    let operation = source.operation();
    assert!(host.native_import_receipt(operation).unwrap().is_none());
    let r = host
        .prepare_native_import(source, &bytes, None, &control())
        .unwrap();
    assert_eq!(r.summary().unwrap().strokes, 1);
    let receipt = host
        .publish_native_import(r, &mut TestAuth(true), true, true)
        .unwrap();
    assert_eq!(receipt.operation, operation);
    assert_eq!(receipt.entities.len(), 1);
    assert_ne!(receipt.entities[0], original);
    let rows = host
        .request(&wire(Command::Blocks {
            entity: receipt.entities[0],
            offset: 0,
            limit: 10,
        }))
        .unwrap()
        .data
        .unwrap();
    let block = Id::parse(rows[0]["id"].as_str().unwrap()).unwrap();
    let ink: Vec<stem_core::ink::Stroke> = serde_json::from_value(
        host.request(&wire(Command::SourceInk { block }))
            .unwrap()
            .data
            .unwrap(),
    )
    .unwrap();
    assert_eq!(ink[0].points, stroke(0).points);
    drop(receipt);
    host.close().unwrap();
    let reopened = Host::open_existing(root.path().join("library.db"), 8).unwrap();
    let receipt = reopened.native_import_receipt(operation).unwrap().unwrap();
    assert_ne!(receipt.entities[0], original);
}
#[test]
fn native_import_confirmation_cancellation_stale_generation_and_foreign_host_do_not_publish() {
    let (bytes, _) = artifact(false);
    let (_root, host) = fixture();
    let (_other, other) = fixture();
    let source = host.native_import_source().unwrap();
    assert!(matches!(
        other.prepare_native_import(source, &bytes, None, &control()),
        Err(Error::Denied)
    ));
    let r = prepare(&host, &bytes, &control());
    let op = r.operation();
    assert!(matches!(
        host.publish_native_import(r, &mut TestAuth(true), false, true),
        Err(Error::ReviewRequired)
    ));
    assert!(host.native_import_receipt(op).unwrap().is_none());
    let r = prepare(&host, &bytes, &control());
    let op = r.operation();
    assert!(matches!(
        other.publish_native_import(r, &mut TestAuth(true), true, true),
        Err(Error::Denied)
    ));
    assert!(host.native_import_receipt(op).unwrap().is_none());
    let c = control();
    let r = prepare(&host, &bytes, &c);
    let op = r.operation();
    c.cancel();
    assert!(matches!(
        host.publish_native_import(r, &mut TestAuth(true), true, true),
        Err(Error::Cancelled)
    ));
    assert!(host.native_import_receipt(op).unwrap().is_none());
    let r = prepare(&host, &bytes, &control());
    let op = r.operation();
    assert!(
        host.request(&wire(Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "later".into()
        }))
        .unwrap()
        .ok
    );
    assert!(matches!(
        host.publish_native_import(r, &mut TestAuth(true), true, true),
        Err(Error::Conflict)
    ));
    assert!(host.native_import_receipt(op).unwrap().is_none());
}
#[test]
fn encrypted_import_requires_correct_credential_and_preserves_locked_flags_without_grants() {
    let (plain, _) = artifact(true);
    let encrypted = crypto::encrypt(&plain, "Native import password test passphrase").unwrap();
    let (_root, host) = fixture();
    let source = host.native_import_source().unwrap();
    let op = source.operation();
    assert!(host
        .prepare_native_import(
            source,
            &encrypted.bytes,
            Some(Credential::Password("wrong password")),
            &control()
        )
        .is_err());
    assert!(host.native_import_receipt(op).unwrap().is_none());
    let r = host
        .prepare_native_import(
            host.native_import_source().unwrap(),
            &encrypted.bytes,
            Some(Credential::Recovery(&encrypted.recovery_key)),
            &control(),
        )
        .unwrap();
    assert!(r.summary().unwrap().protected_objects > 0);
    let receipt = host
        .publish_native_import(r, &mut TestAuth(true), true, true)
        .unwrap();
    assert!(
        !host
            .request(&wire(Command::Read {
                id: receipt.entities[0]
            }))
            .unwrap()
            .ok
    );
    host.authenticate(
        [Scope::Entity(receipt.entities[0])].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    assert!(
        host.request(&wire(Command::Read {
            id: receipt.entities[0]
        }))
        .unwrap()
        .ok
    );
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
fn app_lock(host: &Host) {
    host.authenticate([Scope::App].into_iter().collect(), Box::new(TestAuth(true)))
        .unwrap();
    assert!(
        host.request(&wire(Command::SetAppLock { enabled: true }))
            .unwrap()
            .ok
    );
}
#[test]
fn native_import_fresh_app_verification_is_off_writer_and_reentrant_close_cancels() {
    let (bytes, _) = artifact(false);
    for close in [false, true] {
        let (_root, host) = fixture();
        app_lock(&host);
        let r = prepare(&host, &bytes, &control());
        let op = r.operation();
        assert!(matches!(
            host.publish_native_import(r, &mut TestAuth(false), true, true),
            Err(Error::Denied)
        ));
        assert!(host.native_import_receipt(op).unwrap().is_none());
        let r = prepare(&host, &bytes, &control());
        let result = host.publish_native_import(
            r,
            &mut Reentrant {
                host: Arc::clone(&host),
                close,
            },
            true,
            true,
        );
        if close {
            assert!(matches!(result, Err(Error::Cancelled)));
            host.close().unwrap();
        } else {
            assert!(result.is_ok());
        }
    }
}
#[test]
fn malformed_import_and_capture_priority_release_shared_transfer_admission() {
    let (bytes, _) = artifact(false);
    let (_root, host) = fixture();
    let source = host.native_import_source().unwrap();
    assert!(matches!(
        host.native_import_source(),
        Err(Error::ResourceLimit)
    ));
    assert!(matches!(
        host.derivative_source(stem_core::export::Selection::Blocks(vec![]), false),
        Err(Error::ResourceLimit)
    ));
    assert!(host
        .prepare_native_import(source, b"private malformed source", None, &control())
        .is_err());
    let source = host.native_import_source().unwrap();
    let pen = Id::new();
    host.set_capture_priority(pen, true).unwrap();
    assert!(matches!(
        host.prepare_native_import(source, &bytes, None, &control()),
        Err(Error::Cancelled)
    ));
    host.set_capture_priority(pen, false).unwrap();
    drop(host.native_import_source().unwrap());
}
#[test]
fn untrusted_source_omission_metadata_requires_explicit_acknowledgment() {
    let (bytes, _) = artifact(false);
    let checked: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let mut doc: NativeDocument =
        serde_json::from_str(checked["payload"].as_str().unwrap()).unwrap();
    doc.omissions
        .push("Source-declared omission; untrusted metadata".into());
    let payload = serde_json::to_string(&doc).unwrap();
    let bytes = serde_json::to_vec(
        &serde_json::json!({"sha256":format!("{:x}",sha2::Sha256::digest(payload.as_bytes())),"payload":payload}),
    )
    .unwrap();
    let (_root, host) = fixture();
    let r = prepare(&host, &bytes, &control());
    let op = r.operation();
    assert_eq!(r.summary().unwrap().source_omissions.len(), 1);
    assert!(matches!(
        host.publish_native_import(r, &mut TestAuth(true), true, false),
        Err(Error::ReviewRequired)
    ));
    assert!(host.native_import_receipt(op).unwrap().is_none());
}
