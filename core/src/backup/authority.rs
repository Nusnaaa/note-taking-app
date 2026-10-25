//! Authority for encrypted backup only. Descriptors are metadata, not grants;
//! neither a descriptor nor ordinary notebook JSON can construct a read permit.
use super::{BackupOptions, StreamBackupPlan};
use crate::{
    security::{Access, Authenticator, Scope},
    Error, Id, Result,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::{self, Write},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupAuthority {
    pub version: u32,
    pub library: String,
    pub scope_digest: String,
    pub protected_content: bool,
}
/// Trusted application adapter. It must validate a purpose-bound, previously
/// owner-approved delegation and current live policy; a model never supplies it.
pub trait BackupAuthorizer: Send {
    fn verify(&mut self, challenge: Id, authority: &BackupAuthority) -> bool;
}
pub(crate) struct BackupReadPermit {
    pub(super) library: String,
    pub(super) generation: String,
    pub(super) proof: Access,
    pub(super) delegated: bool,
    pub(super) options: (bool, bool),
}
struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn descriptor(
    library: &str,
    options: BackupOptions,
    scopes: &BTreeSet<Scope>,
) -> Result<BackupAuthority> {
    let library = Id::parse(library)?.to_string();
    let mut hash = HashWriter(Sha256::new());
    hash.write_all(b"STEM-BACKUP-AUTHORITY-1\0")?;
    // serde writes incrementally into the digest, not an aggregate scope buffer.
    serde_json::to_writer(
        &mut hash,
        &(
            &library,
            options.include_original_ink,
            options.include_authorised_source_assets,
            scopes,
        ),
    )?;
    Ok(BackupAuthority {
        version: 1,
        library,
        scope_digest: format!("{:x}", hash.0.finalize()),
        protected_content: !scopes.is_empty(),
    })
}
impl StreamBackupPlan {
    pub fn authority(&self) -> Result<BackupAuthority> {
        descriptor(&self.library, self.options, &self.scopes)
    }
    pub(crate) fn authenticate(&self, auth: &mut dyn Authenticator) -> Result<BackupReadPermit> {
        let proof = if self.scopes.is_empty() {
            Access::public()
        } else {
            Access::authenticate(auth, self.scopes.clone())?
        };
        Ok(BackupReadPermit {
            library: self.library.clone(),
            generation: self.generation.clone(),
            proof,
            delegated: false,
            options: (
                self.options.include_original_ink,
                self.options.include_authorised_source_assets,
            ),
        })
    }
    pub(crate) fn authorize_backup(
        &self,
        authorizer: &mut dyn BackupAuthorizer,
    ) -> Result<BackupReadPermit> {
        let authority = self.authority()?;
        struct Delegated<'a> {
            authorizer: &'a mut dyn BackupAuthorizer,
            authority: &'a BackupAuthority,
        }
        impl Authenticator for Delegated<'_> {
            fn verify(&mut self, challenge: Id, _: &BTreeSet<Scope>) -> bool {
                self.authorizer.verify(challenge, self.authority)
            }
        }
        let proof = if self.scopes.is_empty() {
            if !authorizer.verify(Id::new(), &authority) {
                return Err(Error::Denied);
            }
            Access::public()
        } else {
            Access::authenticate(
                &mut Delegated {
                    authorizer,
                    authority: &authority,
                },
                self.scopes.clone(),
            )?
        };
        Ok(BackupReadPermit {
            library: self.library.clone(),
            generation: self.generation.clone(),
            proof,
            delegated: true,
            options: (
                self.options.include_original_ink,
                self.options.include_authorised_source_assets,
            ),
        })
    }
}
