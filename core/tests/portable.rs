mod common;
use common::*;
use stem_core::{
    content::Content,
    crypto::{self, Credential},
    portable::{ExportProtection, NativeDocument},
    security::{Access, Scope},
    store::{FailurePoint, Kind, Order, Store},
    Error,
};
const PASSWORD: &str = "Synthetic test passphrase 2026!";

#[test]
fn encrypted_package_opens_with_password_or_independent_recovery_key() {
    let plain = b"private document title and original source ink";
    let encrypted = crypto::encrypt(plain, PASSWORD).unwrap();
    assert!(!encrypted.bytes.windows(plain.len()).any(|w| w == plain));
    assert_eq!(
        &**crypto::decrypt(&encrypted.bytes, Credential::Password(PASSWORD)).unwrap(),
        plain
    );
    assert_eq!(
        &**crypto::decrypt(
            &encrypted.bytes,
            Credential::Recovery(&encrypted.recovery_key)
        )
        .unwrap(),
        plain
    );
    assert!(crypto::decrypt(&encrypted.bytes, Credential::Password("wrong password")).is_err());
    assert!(crypto::decrypt(&encrypted.bytes, Credential::Recovery(&"00".repeat(32))).is_err());
}
#[test]
fn encrypted_header_payload_truncation_and_weak_credentials_fail_closed() {
    assert!(crypto::encrypt(b"x", "1234").is_err());
    assert!(crypto::encrypt(b"x", "1234567890123456").is_err());
    let encrypted = crypto::encrypt(b"private data", PASSWORD).unwrap();
    for index in [0, 9, 30, 50, 105, 130, 180, 200] {
        let mut damaged = encrypted.bytes.clone();
        damaged[index] ^= 1;
        assert!(
            crypto::decrypt(&damaged, Credential::Recovery(&encrypted.recovery_key)).is_err(),
            "byte {index}"
        );
    }
    for length in [0, 7, 24, 191, encrypted.bytes.len() - 1] {
        assert!(crypto::decrypt(
            &encrypted.bytes[..length],
            Credential::Recovery(&encrypted.recovery_key)
        )
        .is_err());
    }
}
#[test]
fn raw_ink_requires_deliberate_inclusion_and_subset_export_never_follows_links() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "chosen", &a).unwrap();
    let hidden = s.create(Kind::Note, None, "unselected secret", &a).unwrap();
    s.append_ink(n.id, vec![stroke(0)], &a).unwrap();
    s.add_link(n.id, hidden.id, None, &a).unwrap();
    assert!(matches!(
        s.prepare_native_export(&[n.id], false, &a),
        Err(Error::ReviewRequired)
    ));
    let p = s.prepare_native_export(&[n.id], true, &a).unwrap();
    assert!(!p.omissions.is_empty());
    assert!(s
        .finish_native_export(
            p,
            ExportProtection::Plaintext {
                acknowledge_protected: false
            },
            &mut TestAuth(false),
            false
        )
        .is_err());
    let p = s.prepare_native_export(&[n.id], true, &a).unwrap();
    let exported = s
        .finish_native_export(
            p,
            ExportProtection::Plaintext {
                acknowledge_protected: false,
            },
            &mut TestAuth(false),
            true,
        )
        .unwrap();
    assert!(!String::from_utf8(exported.bytes.clone())
        .unwrap()
        .contains("unselected secret"));
    let doc = NativeDocument::decode(&exported.bytes).unwrap();
    assert_eq!(doc.entities.len(), 1);
    assert!(doc.links.is_empty());
}
#[test]
fn both_protected_export_paths_require_fresh_authentication() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "secret", &a).unwrap();
    let auth = access([Scope::Entity(n.id)]);
    s.set_lock(n.id, 0, true, &auth).unwrap();
    for protection in [
        ExportProtection::Encrypted { password: PASSWORD },
        ExportProtection::Plaintext {
            acknowledge_protected: true,
        },
    ] {
        let plan = s.prepare_native_export(&[n.id], true, &auth).unwrap();
        assert!(matches!(
            s.finish_native_export(plan, protection, &mut TestAuth(false), true),
            Err(Error::Denied)
        ));
    }
    let plan = s.prepare_native_export(&[n.id], true, &auth).unwrap();
    assert!(matches!(
        s.finish_native_export(
            plan,
            ExportProtection::Plaintext {
                acknowledge_protected: false
            },
            &mut TestAuth(true),
            true
        ),
        Err(Error::ReviewRequired)
    ));
    let plan = s.prepare_native_export(&[n.id], true, &auth).unwrap();
    let bytes = s
        .finish_native_export(
            plan,
            ExportProtection::Plaintext {
                acknowledge_protected: true,
            },
            &mut TestAuth(true),
            true,
        )
        .unwrap();
    assert_eq!(
        NativeDocument::decode(&bytes.bytes).unwrap().entities[0].title,
        "secret"
    );
    assert!(s.entity(n.id, &a).is_err());
}
#[test]
fn export_plan_cannot_survive_later_lock_or_source_change() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let p = s.prepare_native_export(&[n.id], false, &a).unwrap();
    s.set_lock(n.id, 0, true, &access([Scope::Entity(n.id)]))
        .unwrap();
    assert!(matches!(
        s.finish_native_export(
            p,
            ExportProtection::Plaintext {
                acknowledge_protected: true
            },
            &mut TestAuth(true),
            true
        ),
        Err(Error::Conflict)
    ));
}
#[test]
fn encrypted_native_roundtrip_creates_independent_copy_and_keeps_current_work() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "original", &a).unwrap();
    let source = stroke(0);
    let b = s.append_ink(n.id, vec![source.clone()], &a).unwrap();
    s.edit_content(b.id, 0, Content::Numeric("001".into()), &a)
        .unwrap();
    let plan = s.prepare_native_export(&[n.id], true, &a).unwrap();
    let exported = s
        .finish_native_export(
            plan,
            ExportProtection::Encrypted { password: PASSWORD },
            &mut TestAuth(false),
            true,
        )
        .unwrap();
    let plain = crypto::decrypt(&exported.bytes, Credential::Password(PASSWORD)).unwrap();
    let new = s.import_native(&plain, &a).unwrap();
    assert_ne!(new[0], n.id);
    let copy = s.blocks(new[0], 0, 100, &a).unwrap().remove(0);
    assert_ne!(copy.id, b.id);
    assert_ne!(copy.source_ids[0], source.id);
    assert_eq!(s.source_ink(copy.id, &a).unwrap()[0].points, source.points);
    assert_eq!(copy.content, Content::Numeric("001".into()));
    assert_eq!(s.entity(n.id, &a).unwrap().title, "original");
    assert_eq!(s.history(copy.id, &a).unwrap().len(), 2);
}
#[test]
fn corrupt_native_import_and_interruption_do_not_touch_existing_documents() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "healthy", &a).unwrap();
    let plan = s.prepare_native_export(&[n.id], false, &a).unwrap();
    let exported = s
        .finish_native_export(
            plan,
            ExportProtection::Plaintext {
                acknowledge_protected: false,
            },
            &mut TestAuth(false),
            true,
        )
        .unwrap();
    let mut corrupt = exported.bytes.clone();
    let last = corrupt.len() - 5;
    corrupt[last] ^= 1;
    assert!(s.import_native(&corrupt, &a).is_err());
    assert!(s.import_native(b"{\"version\":999}", &a).is_err());
    s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.import_native(&exported.bytes, &a).is_err());
    assert_eq!(s.list(None, Order::Name, None, None, &a).unwrap().len(), 1);
    assert_eq!(s.entity(n.id, &a).unwrap(), n);
}
#[test]
fn hostile_native_relationships_rejected_even_with_recomputed_checksum() {
    use sha2::{Digest, Sha256};
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "n", &a).unwrap();
    let plan = s.prepare_native_export(&[n.id], false, &a).unwrap();
    let exported = s
        .finish_native_export(
            plan,
            ExportProtection::Plaintext {
                acknowledge_protected: false,
            },
            &mut TestAuth(false),
            true,
        )
        .unwrap();
    let mut doc = NativeDocument::decode(&exported.bytes).unwrap();
    doc.entities[0].parent = Some(n.id);
    let payload = serde_json::to_string(&doc).unwrap();
    let attack=serde_json::to_vec(&serde_json::json!({"sha256":format!("{:x}",Sha256::digest(payload.as_bytes())),"payload":payload})).unwrap();
    assert!(s.import_native(&attack, &a).is_err());
    assert_eq!(s.entity(n.id, &a).unwrap(), n);
}

#[test]
fn concurrent_change_during_export_authentication_invalidates_output() {
    use stem_core::security::Authenticator;
    struct ConcurrentAuth {
        path: std::path::PathBuf,
    }
    impl Authenticator for ConcurrentAuth {
        fn verify(&mut self, _: stem_core::Id, _: &std::collections::BTreeSet<Scope>) -> bool {
            let mut other = Store::open(&self.path).unwrap();
            other
                .create(Kind::Note, None, "concurrent edit", &Access::public())
                .unwrap();
            true
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("race.db");
    let mut s = Store::open(&path).unwrap();
    let a = Access::public();
    let n = s.create(Kind::Note, None, "locked", &a).unwrap();
    let auth = access([Scope::Entity(n.id)]);
    s.set_lock(n.id, 0, true, &auth).unwrap();
    let plan = s.prepare_native_export(&[n.id], false, &auth).unwrap();
    assert!(matches!(
        s.finish_native_export(
            plan,
            ExportProtection::Plaintext {
                acknowledge_protected: true
            },
            &mut ConcurrentAuth { path },
            true
        ),
        Err(Error::Conflict)
    ));
}
