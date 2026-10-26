//! Explicit unpublished-import discard with durable consent/decision history.
//! File preparation/deletion is worker work; Store only validates and writes metadata.
use super::*;
use crate::{processing::JobControl, provisioning::discard::DiscardFiles};
use std::{sync::Arc, time::Instant};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceActionKind {
    DiscardImport,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceActionState {
    Pending,
    Completed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineResourceAction {
    pub format: u32,
    pub id: Id,
    pub version: Id,
    pub library: Id,
    pub kind: ResourceActionKind,
    pub state: ResourceActionState,
    pub descriptor: Descriptor,
    pub total: u64,
    pub committed: u64,
    pub import_consent_ms: i64,
    pub requested_ms: i64,
}
impl EngineResourceAction {
    pub(crate) fn validate(&self) -> Result<()> {
        Manager::validate(&self.descriptor)?;
        let hash = self
            .descriptor
            .resource_sha256
            .as_deref()
            .ok_or(Error::Integrity)?;
        if self.format != 1
            || self.total == 0
            || self.total > 16 * 1024 * 1024 * 1024
            || self.committed > self.total
            || hash.len() != 64
            || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            || serde_json::to_vec(self)?.len() > 128 * 1024
        {
            return Err(Error::Integrity);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct EngineResourceActions {
    pub actions: Vec<EngineResourceAction>,
    pub next: Option<Id>,
}
pub struct EngineDiscardPlan {
    library: Id,
    version: Id,
    id: Id,
    requested_ms: i64,
    expires: Option<Instant>,
    prior: Option<EngineResourceAction>,
}
pub struct PreparedEngineDiscard {
    files: DiscardFiles,
    record: EngineResourceAction,
    key: Arc<()>,
    expires: Option<Instant>,
}
pub struct EngineDiscardPermit {
    record: EngineResourceAction,
    key: Arc<()>,
}
pub struct DiscardedEngineImport {
    _files: DiscardFiles,
    record: EngineResourceAction,
    expires: Option<Instant>,
    control: JobControl,
}
fn check(expires: Option<Instant>, control: &JobControl) -> Result<()> {
    if expires.is_some_and(|until| Instant::now() >= until) {
        return Err(Error::Denied);
    }
    control.check()
}
impl EngineDiscardPlan {
    pub fn id(&self) -> Id {
        self.id
    }
    pub fn prepare(self, root: &Path, control: &JobControl) -> Result<PreparedEngineDiscard> {
        check(self.expires, control)?;
        let prior = self
            .prior
            .as_ref()
            .map(|r| (&r.descriptor, r.total, r.committed, r.import_consent_ms));
        let files = DiscardFiles::prepare(root, self.version, &self.library.to_string(), prior)?;
        check(self.expires, control)?;
        let record = if let Some(prior) = self.prior {
            prior
        } else {
            EngineResourceAction {
                format: 1,
                id: self.id,
                version: self.version,
                library: self.library,
                kind: ResourceActionKind::DiscardImport,
                state: ResourceActionState::Pending,
                descriptor: files.descriptor().clone(),
                total: files.total(),
                committed: files.committed(),
                import_consent_ms: files.consent_ms()?,
                requested_ms: self.requested_ms,
            }
        };
        record.validate()?;
        Ok(PreparedEngineDiscard {
            files,
            record,
            key: Arc::new(()),
            expires: self.expires,
        })
    }
}
impl PreparedEngineDiscard {
    pub fn action(&self) -> &EngineResourceAction {
        &self.record
    }
    pub fn remove(
        self,
        permit: EngineDiscardPermit,
        control: &JobControl,
    ) -> Result<DiscardedEngineImport> {
        check(self.expires, control)?;
        if !Arc::ptr_eq(&self.key, &permit.key) || self.record != permit.record {
            return Err(Error::Conflict);
        }
        self.files.remove(|| check(self.expires, control))?;
        Ok(DiscardedEngineImport {
            _files: self.files,
            record: self.record,
            expires: self.expires,
            control: control.clone(),
        })
    }
}
impl DiscardedEngineImport {
    pub fn action(&self) -> &EngineResourceAction {
        &self.record
    }
}
impl Store {
    fn unpublished(db: &Connection, version: Id) -> Result<()> {
        if db.query_row(
            "SELECT EXISTS(SELECT 1 FROM engine_versions WHERE id=?1)",
            [version.to_string()],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::Conflict);
        }
        Ok(())
    }
    fn read_resource_action(db: &Connection, id: Id) -> Result<EngineResourceAction> {
        let (version, payload): (String, String) = db
            .query_row(
                "SELECT version,payload FROM engine_resource_actions WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;
        let record: EngineResourceAction = serde_json::from_str(&payload)?;
        record.validate()?;
        if record.id != id || record.version.to_string() != version {
            return Err(Error::Integrity);
        }
        Ok(record)
    }
    pub fn engine_resource_action(&self, id: Id, access: &Access) -> Result<EngineResourceAction> {
        Self::require_app(&self.db, access)?;
        Self::read_resource_action(&self.db, id)
    }
    pub fn engine_resource_actions(
        &self,
        after: Option<Id>,
        limit: usize,
        access: &Access,
    ) -> Result<EngineResourceActions> {
        Self::require_app(&self.db, access)?;
        if !(1..=128).contains(&limit) {
            return Err(Error::ResourceLimit);
        }
        let mut q = self
            .db
            .prepare("SELECT id FROM engine_resource_actions WHERE id>?1 ORDER BY id LIMIT ?2")?;
        let ids = q.query_map(
            rusqlite::params![
                after.map(|id| id.to_string()).unwrap_or_default(),
                limit as i64 + 1
            ],
            |r| r.get::<_, String>(0),
        )?;
        let mut actions = Vec::new();
        let mut charge = 0usize;
        let mut next = None;
        for id in ids {
            if actions.len() == limit {
                next = actions.last().map(|r: &EngineResourceAction| r.id);
                break;
            }
            let action = Self::read_resource_action(&self.db, Id::parse(&id?)?)?;
            let bytes = serde_json::to_vec(&action)?.len();
            if charge > (4 * 1024 * 1024usize).saturating_sub(bytes) {
                next = actions.last().map(|r| r.id);
                break;
            }
            charge += bytes;
            actions.push(action);
        }
        Ok(EngineResourceActions { actions, next })
    }
    pub fn engine_discard_plan(
        &self,
        version: Id,
        confirmed: bool,
        resume_action: Option<Id>,
        access: &Access,
    ) -> Result<EngineDiscardPlan> {
        Self::require_app(&self.db, access)?;
        if !confirmed {
            return Err(Error::Denied);
        }
        Self::unpublished(&self.db, version)?;
        let library = Id::parse(&Self::setting(&self.db, "library_id")?)?;
        let prior = resume_action
            .map(|id| Self::read_resource_action(&self.db, id))
            .transpose()?;
        if prior.as_ref().is_some_and(|r| {
            r.version != version || r.library != library || r.state != ResourceActionState::Pending
        }) {
            return Err(Error::Conflict);
        }
        Ok(EngineDiscardPlan {
            library,
            version,
            id: prior.as_ref().map(|r| r.id).unwrap_or_default(),
            requested_ms: prior
                .as_ref()
                .map(|r| r.requested_ms)
                .unwrap_or_else(now_ms),
            expires: access.expires_at(),
            prior,
        })
    }
    /// Prepared keeps exclusive ownership alive throughout this metadata-only write.
    pub fn authorise_engine_discard(
        &mut self,
        prepared: &PreparedEngineDiscard,
        control: &JobControl,
        access: &Access,
    ) -> Result<EngineDiscardPermit> {
        Self::require_app(&self.db, access)?;
        check(prepared.expires, control)?;
        let record = &prepared.record;
        record.validate()?;
        if Id::parse(&Self::setting(&self.db, "library_id")?)? != record.library {
            return Err(Error::Conflict);
        }
        self.write(|db| {
            Self::require_app(db, access)?;
            check(prepared.expires, control)?;
            Self::unpublished(db, record.version)?;
            let existing = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM engine_resource_actions WHERE id=?1)",
                [record.id.to_string()],
                |r| r.get::<_, bool>(0),
            )?;
            if existing {
                if Self::read_resource_action(db, record.id)? != *record {
                    return Err(Error::Conflict);
                }
            } else {
                db.execute(
                    "INSERT INTO engine_resource_actions VALUES(?1,?2,?3)",
                    rusqlite::params![
                        record.id.to_string(),
                        record.version.to_string(),
                        serde_json::to_string(record)?
                    ],
                )?;
            }
            check(prepared.expires, control)?;
            Ok(())
        })?;
        Ok(EngineDiscardPermit {
            record: record.clone(),
            key: Arc::clone(&prepared.key),
        })
    }
    /// Physical removal already happened; error does not mean files were restored.
    pub fn complete_engine_discard(
        &mut self,
        discarded: &DiscardedEngineImport,
        access: &Access,
    ) -> Result<EngineResourceAction> {
        Self::require_app(&self.db, access)?;
        check(discarded.expires, &discarded.control)?;
        if Id::parse(&Self::setting(&self.db, "library_id")?)? != discarded.record.library {
            return Err(Error::Conflict);
        }
        let mut done = discarded.record.clone();
        done.state = ResourceActionState::Completed;
        if Self::read_resource_action(&self.db, done.id)? == done {
            return Ok(done);
        }
        self.write(|db| {
            Self::require_app(db, access)?;
            check(discarded.expires, &discarded.control)?;
            let current = Self::read_resource_action(db, done.id)?;
            if current != discarded.record && current != done {
                return Err(Error::Conflict);
            }
            db.execute(
                "UPDATE engine_resource_actions SET payload=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&done)?, done.id.to_string()],
            )?;
            check(discarded.expires, &discarded.control)?;
            Ok(())
        })?;
        Ok(done)
    }
}
