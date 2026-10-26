//! Durable, consented local installation. Model resources are never notebook data
//! or executable plugins. Activation stages a probe before changing a live adapter.
use super::*;
mod activation;
mod catalogue;
mod import;
mod recovery;
pub use activation::{EngineActivationPlan, PreparedEngineActivation};
pub use catalogue::{EngineCatalogue, EngineCatalogueRow};
pub use import::{EngineImportPlan, EngineImportTransfer, PreparedEngineImport};
pub use recovery::EngineRecoveryPlan;

use crate::{
    digest, now_ms,
    processing::{Descriptor, Manager, Readiness, Recogniser, Task},
};

use sha2::{Digest, Sha256};

use std::{
    io::{Read, Write},
    path::Path,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Installation {
    Staged,
    Installed,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineVersion {
    pub id: Id,
    pub descriptor: Descriptor,
    pub state: Installation,
    pub local_resource: Option<String>,
    pub bytes: u64,
    pub installed_ms: Option<i64>,
}

impl Store {
    /// Stream into a private staging name, verify hash, then publish metadata.
    /// The root is an app-owned local directory supplied by the trusted host.
    pub fn stage_engine(
        &mut self,
        descriptor: Descriptor,
        resource: Option<&mut dyn Read>,
        root: &Path,
        max_bytes: u64,
        consented: bool,
        access: &Access,
    ) -> Result<EngineVersion> {
        Self::require_app(&self.db, access)?;

        if !consented {
            return Err(Error::Denied);
        }

        Manager::validate(&descriptor)?;

        match descriptor.family {
            crate::processing::Family::Recogniser
                if descriptor.input == crate::processing::InputContract::OrderedStrokesV1
                    && descriptor.internals == crate::processing::Internals::StrokeOnlyVerified
                    && !descriptor.tasks.is_empty() => {}
            crate::processing::Family::Converter
                if descriptor.input == crate::processing::InputContract::EditableV1 => {}
            _ => return Err(Error::EngineUnavailable),
        }

        if descriptor.resource_sha256.is_some() != resource.is_some() {
            return Err(Error::Invalid("engine resource contract"));
        }

        let mut v = EngineVersion {
            id: Id::new(),
            descriptor,
            state: Installation::Staged,
            local_resource: None,
            bytes: 0,
            installed_ms: None,
        };

        if let Some(reader) = resource {
            std::fs::create_dir_all(root)?;

            let path = root.join(format!("{}.resource", v.id));

            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)?;

            let result = (|| -> Result<()> {
                let mut hash = Sha256::new();

                let mut buffer = [0u8; 65536];

                loop {
                    let n = reader.read(&mut buffer)?;

                    if n == 0 {
                        break;
                    }

                    v.bytes = v.bytes.checked_add(n as u64).ok_or(Error::ResourceLimit)?;

                    if v.bytes > max_bytes {
                        return Err(Error::ResourceLimit);
                    }

                    file.write_all(&buffer[..n])?;

                    hash.update(&buffer[..n]);
                }

                file.sync_all()?;

                if format!("{:x}", hash.finalize())
                    != *v
                        .descriptor
                        .resource_sha256
                        .as_ref()
                        .expect("resource contract")
                {
                    return Err(Error::Integrity);
                }

                Ok(())
            })();

            drop(file);

            if let Err(error) = result {
                let _ = std::fs::remove_file(&path);

                return Err(error);
            }

            v.local_resource = Some(path.to_str().ok_or(Error::Invalid("resource path"))?.into());
        }

        let result = self.write(|db| {
            Self::require_app(db, access)?;

            db.execute(
                "INSERT INTO engine_versions VALUES(?1,?2,?3)",
                rusqlite::params![
                    v.id.to_string(),
                    v.descriptor.id,
                    serde_json::to_string(&v)?
                ],
            )?;

            Ok(v.clone())
        });

        if result.is_err() {
            if let Some(path) = &v.local_resource {
                let _ = std::fs::remove_file(path);
            }
        }

        result
    }

    fn read_engine(db: &Connection, id: Id, access: &Access) -> Result<EngineVersion> {
        Self::require_app(db, access)?;

        let (engine, json): (String, String) = db
            .query_row(
                "SELECT engine,payload FROM engine_versions WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;

        let version: EngineVersion = serde_json::from_str(&json)?;
        if version.id != id || version.descriptor.id != engine {
            return Err(Error::Integrity);
        }
        Ok(version)
    }

    pub fn engine_version(&self, id: Id, access: &Access) -> Result<EngineVersion> {
        Self::read_engine(&self.db, id, access)
    }

    /// Installed is durable provenance, never a promise that the current process
    /// can convert. Every new Manager must load and probe its own adapter.
    pub fn active_engine(&self, engine: &str, access: &Access) -> Result<Option<EngineVersion>> {
        Self::require_app(&self.db, access)?;

        let id: Option<String> = self
            .db
            .query_row(
                "SELECT version FROM engine_active WHERE engine=?1",
                [engine],
                |r| r.get(0),
            )
            .optional()?;

        let version = id
            .map(|id| self.engine_version(Id::parse(&id)?, access))
            .transpose()?;
        if version
            .as_ref()
            .is_some_and(|v| v.descriptor.id != engine || v.state != Installation::Installed)
        {
            return Err(Error::Integrity);
        }
        Ok(version)
    }

    pub fn activate_engine(
        &mut self,
        id: Id,
        manager: &mut Manager,
        adapter: Box<dyn Recogniser>,
        max_load_bytes: u64,
        access: &Access,
    ) -> Result<()> {
        let generation = Self::setting(&self.db, "generation")?;
        let mut v = self.engine_version(id, access)?;

        if v.state == Installation::Removed {
            return Err(Error::Unavailable);
        }

        let resource = if let Some(path) = &v.local_resource {
            let file = std::fs::File::open(path)?;

            if file.metadata()?.len() > max_load_bytes {
                return Err(Error::ResourceLimit);
            }

            let mut bytes = Vec::new();

            file.take(max_load_bytes.saturating_add(1))
                .read_to_end(&mut bytes)?;

            if bytes.len() as u64 > max_load_bytes
                || Some(digest(&bytes)) != v.descriptor.resource_sha256
            {
                return Err(Error::Integrity);
            }

            Some(zeroize::Zeroizing::new(bytes))
        } else {
            None
        };

        let defaults = self
            .engine_defaults(access)?
            .into_iter()
            .filter_map(|(task, engine)| (engine == v.descriptor.id).then_some(task))
            .collect();
        let prepared = Manager::prepare_replacement(
            v.descriptor.clone(),
            adapter,
            resource.as_ref().map(|b| b.as_slice()),
        )?;

        v.state = Installation::Installed;

        v.installed_ms = Some(now_ms());

        self.write(|db| {
            Self::require_app(db, access)?;
            if Self::setting(db, "generation")? != generation {
                return Err(Error::Conflict);
            }
            db.execute(
                "UPDATE engine_versions SET payload=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&v)?, id.to_string()],
            )?;
            db.execute(
                "INSERT INTO engine_active VALUES(?1,?2) ON CONFLICT(engine) DO UPDATE SET \
                version=excluded.version",
                rusqlite::params![v.descriptor.id, id.to_string()],
            )?;
            Ok(())
        })?;

        // Infallible swap follows successful durable commit. The old adapter remains
        // usable throughout failed verification, probe, or transaction rollback.
        manager.commit_replacement(prepared, defaults);

        Ok(())
    }

    pub fn activate_converter(
        &mut self,
        id: Id,
        manager: &mut Manager,
        adapter: Box<dyn crate::processing::Converter>,
        max_load_bytes: u64,
        access: &Access,
    ) -> Result<()> {
        let generation = Self::setting(&self.db, "generation")?;
        let mut v = self.engine_version(id, access)?;

        if v.state == Installation::Removed {
            return Err(Error::Unavailable);
        }

        let resource = if let Some(path) = &v.local_resource {
            let file = std::fs::File::open(path)?;

            if file.metadata()?.len() > max_load_bytes {
                return Err(Error::ResourceLimit);
            }

            let mut bytes = Vec::new();

            file.take(max_load_bytes.saturating_add(1))
                .read_to_end(&mut bytes)?;

            if bytes.len() as u64 > max_load_bytes
                || Some(digest(&bytes)) != v.descriptor.resource_sha256
            {
                return Err(Error::Integrity);
            }

            Some(zeroize::Zeroizing::new(bytes))
        } else {
            None
        };

        let prepared = Manager::prepare_converter(
            v.descriptor.clone(),
            adapter,
            resource.as_ref().map(|b| b.as_slice()),
        )?;

        v.state = Installation::Installed;

        v.installed_ms = Some(now_ms());

        self.write(|db| {
            Self::require_app(db, access)?;
            if Self::setting(db, "generation")? != generation {
                return Err(Error::Conflict);
            }
            db.execute(
                "UPDATE engine_versions SET payload=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&v)?, id.to_string()],
            )?;
            db.execute(
                "INSERT INTO engine_active VALUES(?1,?2) ON CONFLICT(engine) DO UPDATE SET \
                version=excluded.version",
                rusqlite::params![v.descriptor.id, id.to_string()],
            )?;
            Ok(())
        })?;

        // Infallible swap follows successful durable commit. The old adapter remains
        // usable throughout failed verification, probe, or transaction rollback.
        manager.commit_converter(prepared);

        Ok(())
    }

    pub fn select_engine(
        &mut self,
        task: Task,
        engine: &str,
        manager: &mut Manager,
        access: &Access,
    ) -> Result<()> {
        let generation = Self::setting(&self.db, "generation")?;
        let v = self
            .active_engine(engine, access)?
            .ok_or(Error::EngineUnavailable)?;

        if manager.readiness(engine) != Readiness::Ready
            || !manager.descriptors().contains(&v.descriptor)
            || !v.descriptor.tasks.contains(&task)
        {
            return Err(Error::EngineUnavailable);
        }

        manager.validate_selection(task, engine)?;
        self.write(|db| {
            Self::require_app(db, access)?;
            if Self::setting(db, "generation")? != generation {
                return Err(Error::Conflict);
            }
            db.execute(
                "INSERT INTO engine_defaults VALUES(?1,?2) ON CONFLICT(task) DO UPDATE SET \
                engine=excluded.engine",
                rusqlite::params![serde_json::to_string(&task)?, engine],
            )?;
            Ok(())
        })?;

        manager.select(task, engine)
    }

    pub fn engine_defaults(&self, access: &Access) -> Result<Vec<(Task, String)>> {
        Self::require_app(&self.db, access)?;

        let mut q = self
            .db
            .prepare("SELECT task,engine FROM engine_defaults ORDER BY task")?;

        let mut values = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (task, engine) = row?;

            let task = serde_json::from_str(&task)?;
            if engine.is_empty() || engine.len() > 256 || !seen.insert(task) {
                return Err(Error::Integrity);
            }
            values.push((task, engine));
        }

        Ok(values)
    }

    /// Removal disables selection and conversion; immutable resources are retained
    /// for explicit cleanup after all native resource leases have ended.
    pub fn remove_engine(&mut self, id: Id, manager: &mut Manager, access: &Access) -> Result<()> {
        let generation = Self::setting(&self.db, "generation")?;
        let mut v = self.engine_version(id, access)?;

        let active = self
            .active_engine(&v.descriptor.id, access)?
            .is_some_and(|a| a.id == id);

        v.state = Installation::Removed;

        self.write(|db| {
            Self::require_app(db, access)?;
            if Self::setting(db, "generation")? != generation {
                return Err(Error::Conflict);
            }

            db.execute(
                "UPDATE engine_versions SET payload=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&v)?, id.to_string()],
            )?;

            db.execute(
                "DELETE FROM engine_active WHERE version=?1",
                [id.to_string()],
            )?;

            if active {
                db.execute(
                    "DELETE FROM engine_defaults WHERE engine=?1",
                    [&v.descriptor.id],
                )?;
            }

            Ok(())
        })?;

        if active {
            let _ = manager.remove(&v.descriptor.id);
        }

        Ok(())
    }
}
