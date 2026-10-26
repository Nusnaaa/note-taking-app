//! Metadata-only import authority, resumable optional-worker file preparation,
//! then callback-free transactional staging. No SDK is constructed or enabled.
use super::*;
use crate::{processing::JobControl, provisioning::Transfer};
use std::{path::PathBuf, time::Instant};

pub struct EngineImportPlan {
    library: String,
    generation: String,
    descriptor: Descriptor,
    total: u64,
    expires: Option<Instant>,
}
pub struct EngineImportTransfer {
    plan: EngineImportPlan,
    transfer: Transfer,
}
pub struct PreparedEngineImport {
    plan: EngineImportPlan,
    version: EngineVersion,
    consent_ms: i64,
    control: JobControl,
    _ownership: std::sync::Arc<crate::provisioning::TransferOwnership>,
}
impl EngineImportPlan {
    pub fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }
    fn check(&self, control: &JobControl) -> Result<()> {
        if self.expires.is_some_and(|until| Instant::now() >= until) {
            return Err(Error::Denied);
        }
        control.check()
    }
    /// File creation belongs on an optional worker. The root is app-owned and
    /// must exclude OS cloud backup; it is never accepted from notebook JSON.
    pub fn begin(self, root: &Path, control: &JobControl) -> Result<EngineImportTransfer> {
        self.check(control)?;
        let transfer = Transfer::begin_bound(
            root,
            self.descriptor.clone(),
            self.total,
            self.library.clone(),
        )?;
        Ok(EngineImportTransfer {
            plan: self,
            transfer,
        })
    }
    /// Validate library/manifest/recorded consent before opening/truncating a
    /// resumed part. Legacy unbound transfers require explicit separate review.
    pub fn resume(self, root: &Path, id: Id, control: &JobControl) -> Result<EngineImportTransfer> {
        self.check(control)?;
        let transfer =
            Transfer::resume_bound(root, id, &self.library, &self.descriptor, self.total)?;
        self.check(control)?;
        Ok(EngineImportTransfer {
            plan: self,
            transfer,
        })
    }
}
impl EngineImportTransfer {
    /// Stable receipt available before reading the selected source. Failure/drop
    /// preserves resumable files and their actual durable offset, not SQL state.
    pub fn id(&self) -> Id {
        self.transfer.id()
    }
    pub fn committed_bytes(&self) -> u64 {
        self.transfer.committed_bytes()
    }
    pub fn append(&mut self, offset: u64, bytes: &[u8], control: &JobControl) -> Result<u64> {
        self.plan.check(control)?;
        let committed = self.transfer.append(offset, bytes)?;
        self.plan.check(control)?;
        Ok(committed)
    }
    /// Reader starts at committed_bytes. Require exact remaining length/EOF;
    /// cancellation cannot force an uncooperative reader to return.
    pub fn copy_remaining(&mut self, reader: &mut dyn Read, control: &JobControl) -> Result<()> {
        let mut buffer = zeroize::Zeroizing::new([0u8; 65536]);
        loop {
            self.plan.check(control)?;
            let remaining = self.transfer.total_bytes() - self.committed_bytes();
            let maximum = (remaining as u128).min(buffer.len() as u128) as usize;
            if maximum == 0 {
                let extra = match reader.read(&mut buffer[..1]) {
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    result => result?,
                };
                self.plan.check(control)?;
                return if extra == 0 {
                    Ok(())
                } else {
                    Err(Error::Integrity)
                };
            }
            let n = match reader.read(&mut buffer[..maximum]) {
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                result => result?,
            };
            self.plan.check(control)?;
            if n == 0 {
                return Err(Error::Integrity);
            }
            self.append(self.committed_bytes(), &buffer[..n], control)?;
        }
    }
    pub fn finish(mut self, control: &JobControl) -> Result<PreparedEngineImport> {
        let plan = &self.plan;
        let path: PathBuf = self.transfer.finish_checked(|| plan.check(control))?;
        let consent_ms = self.transfer.consent_ms().ok_or(Error::Integrity)?;
        let version = EngineVersion {
            id: self.transfer.id(),
            descriptor: self.plan.descriptor.clone(),
            state: Installation::Staged,
            local_resource: Some(path.to_str().ok_or(Error::Invalid("resource path"))?.into()),
            bytes: self.plan.total,
            installed_ms: None,
        };
        Ok(PreparedEngineImport {
            plan: self.plan,
            version,
            consent_ms,
            control: control.clone(),
            _ownership: self.transfer.ownership(),
        })
    }
}
impl PreparedEngineImport {
    pub fn version(&self) -> &EngineVersion {
        &self.version
    }
    /// Recorded in the private transfer ledger beside the model bytes. That
    /// ledger is installation state, not a notebook backup or licence grant.
    pub fn consent_ms(&self) -> i64 {
        self.consent_ms
    }
}
impl Store {
    pub fn engine_import_plan(
        &self,
        descriptor: Descriptor,
        total: u64,
        consented: bool,
        access: &Access,
    ) -> Result<EngineImportPlan> {
        Self::require_app(&self.db, access)?;
        if !consented {
            return Err(Error::Denied);
        }
        match descriptor.family {
            crate::processing::Family::Recogniser => Manager::validate_recogniser(&descriptor)?,
            crate::processing::Family::Converter => Manager::validate_converter(&descriptor)?,
        }
        Transfer::validate_manifest(&descriptor, total)?;
        Ok(EngineImportPlan {
            library: Self::setting(&self.db, "library_id")?,
            generation: Self::setting(&self.db, "generation")?,
            descriptor,
            total,
            expires: access.expires_at(),
        })
    }
    /// Metadata only: no reads, hashes, filesystem sync, SDK callback or disposal.
    /// Stable transfer IDs make an already acknowledged identical stage retry
    /// return current metadata without reviving a removed version.
    pub fn commit_engine_import(
        &mut self,
        prepared: &PreparedEngineImport,
        access: &Access,
    ) -> Result<EngineVersion> {
        Self::require_app(&self.db, access)?;
        prepared.plan.check(&prepared.control)?;
        if Self::setting(&self.db, "library_id")? != prepared.plan.library {
            return Err(Error::Conflict);
        }
        if self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM engine_resource_actions WHERE version=?1)",
            [prepared.version.id.to_string()],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::Conflict);
        }
        let exists: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM engine_versions WHERE id=?1)",
            [prepared.version.id.to_string()],
            |r| r.get(0),
        )?;
        if exists {
            let current = self.engine_version(prepared.version.id, access)?;
            if current.descriptor != prepared.version.descriptor
                || current.local_resource != prepared.version.local_resource
                || current.bytes != prepared.version.bytes
                || current.state == Installation::Removed
            {
                return Err(Error::Conflict);
            }
            return Ok(current);
        }
        if Self::setting(&self.db, "generation")? != prepared.plan.generation {
            return Err(Error::Conflict);
        }
        self.write(|db| {
            Self::require_app(db, access)?;
            prepared.plan.check(&prepared.control)?;
            if Self::setting(db, "generation")? != prepared.plan.generation {
                return Err(Error::Conflict);
            }
            db.execute(
                "INSERT INTO engine_versions VALUES(?1,?2,?3)",
                rusqlite::params![
                    prepared.version.id.to_string(),
                    prepared.version.descriptor.id,
                    serde_json::to_string(&prepared.version)?
                ],
            )?;
            prepared.plan.check(&prepared.control)?;
            Ok(prepared.version.clone())
        })
    }
}
