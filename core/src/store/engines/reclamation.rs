//! Reviewed deletion of logically Removed model files with durable retry history.
use super::*;
pub use crate::provisioning::reclamation::{
    ConsentProvenance, ReclamationConsent, ReclamationFacts, ResourceEntry, ResourceEntryKind,
};
use crate::{
    processing::JobControl,
    provisioning::reclamation::{ReclamationFiles, ReclamationLocation},
};
use std::{sync::Arc, time::Instant};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReclamationDecision {
    pub reviewed_ms: i64,
    pub facts: ReclamationFacts,
    pub consent: ReclamationConsent,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineReclamation {
    pub format: u32,
    pub id: Id,
    pub version: Id,
    pub library: Id,
    pub descriptor: Descriptor,
    pub total: u64,
    pub requested_ms: i64,
    pub state: ResourceActionState,
    pub reviews: Vec<ReclamationDecision>,
}
impl EngineReclamation {
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
            || hash.len() != 64
            || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            || self.reviews.is_empty()
            || self.reviews.len() > 64
            || serde_json::to_vec(self)?.len() > 128 * 1024
        {
            return Err(Error::Integrity);
        }
        for review in &self.reviews {
            review.facts.accept(review.consent)?;
            for entry in [
                &review.facts.resource,
                &review.facts.part,
                &review.facts.ledger,
            ] {
                if (entry.kind == ResourceEntryKind::File) != entry.bytes.is_some() {
                    return Err(Error::Integrity);
                }
            }
            let facts = &review.facts;
            if facts.binding_review_required
                != (facts.consent_provenance == ConsentProvenance::Unknown)
                || facts.original_import_consent_ms.is_none()
                    != (facts.consent_provenance == ConsentProvenance::Unknown)
                || facts.consent_provenance == ConsentProvenance::BoundLedger
                    && facts.ledger.kind != ResourceEntryKind::File
                || facts.consent_provenance == ConsentProvenance::RecordedDecision
                    && [&facts.resource, &facts.part, &facts.ledger]
                        .iter()
                        .any(|e| e.kind != ResourceEntryKind::Missing)
            {
                return Err(Error::Integrity);
            }
        }
        Ok(())
    }
    pub(crate) fn matches_version(&self, v: &EngineVersion) -> Result<()> {
        if v.id != self.version
            || v.state != Installation::Removed
            || v.descriptor != self.descriptor
            || v.bytes != self.total
        {
            return Err(Error::Integrity);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct EngineReclamations {
    pub actions: Vec<EngineReclamation>,
    pub next: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EngineReclamationSummary {
    pub action: Id,
    pub version: Id,
    pub descriptor: Descriptor,
    pub total: u64,
    pub facts: ReclamationFacts,
}
pub struct EngineReclamationPlan {
    library: Id,
    version: EngineVersion,
    id: Id,
    requested_ms: i64,
    expires: Option<Instant>,
    prior: Option<EngineReclamation>,
}
pub struct EngineReclamationReview {
    plan: EngineReclamationPlan,
    location: ReclamationLocation,
    summary: EngineReclamationSummary,
}
pub struct PreparedEngineReclamation {
    files: ReclamationFiles,
    version: EngineVersion,
    record: EngineReclamation,
    prior: Option<EngineReclamation>,
    key: Arc<()>,
    expires: Option<Instant>,
}
pub struct EngineReclamationPermit {
    record: EngineReclamation,
    key: Arc<()>,
}
pub struct ReclaimedEngineResource {
    _files: ReclamationFiles,
    version: EngineVersion,
    record: EngineReclamation,
    expires: Option<Instant>,
    control: JobControl,
}
fn check(expires: Option<Instant>, control: &JobControl) -> Result<()> {
    if expires.is_some_and(|until| Instant::now() >= until) {
        return Err(Error::Denied);
    }
    control.check()
}
impl EngineReclamationPlan {
    pub fn id(&self) -> Id {
        self.id
    }
    pub fn review(self, root: &Path, control: &JobControl) -> Result<EngineReclamationReview> {
        check(self.expires, control)?;
        let original = self.prior.as_ref().and_then(|r| {
            r.reviews
                .iter()
                .rev()
                .find_map(|r| r.facts.original_import_consent_ms)
        });
        let expected = self
            .version
            .local_resource
            .as_deref()
            .ok_or(Error::ReviewRequired)?;
        let (location, facts) = ReclamationLocation::review(
            root,
            self.version.id,
            self.library,
            self.version.descriptor.clone(),
            self.version.bytes,
            expected,
            original,
        )?;
        check(self.expires, control)?;
        let summary = EngineReclamationSummary {
            action: self.id,
            version: self.version.id,
            descriptor: self.version.descriptor.clone(),
            total: self.version.bytes,
            facts,
        };
        Ok(EngineReclamationReview {
            plan: self,
            location,
            summary,
        })
    }
}
impl EngineReclamationReview {
    pub fn summary(&self) -> &EngineReclamationSummary {
        &self.summary
    }
    pub fn prepare(
        self,
        consent: ReclamationConsent,
        control: &JobControl,
    ) -> Result<PreparedEngineReclamation> {
        check(self.plan.expires, control)?;
        let files = self.location.prepare(&self.summary.facts, consent)?;
        check(self.plan.expires, control)?;
        let mut record = self
            .plan
            .prior
            .clone()
            .unwrap_or_else(|| EngineReclamation {
                format: 1,
                id: self.plan.id,
                version: self.plan.version.id,
                library: self.plan.library,
                descriptor: self.plan.version.descriptor.clone(),
                total: self.plan.version.bytes,
                requested_ms: self.plan.requested_ms,
                state: ResourceActionState::Pending,
                reviews: Vec::new(),
            });
        record.reviews.push(ReclamationDecision {
            reviewed_ms: now_ms(),
            facts: self.summary.facts,
            consent,
        });
        record.validate()?;
        Ok(PreparedEngineReclamation {
            files,
            version: self.plan.version,
            record,
            prior: self.plan.prior,
            key: Arc::new(()),
            expires: self.plan.expires,
        })
    }
}
impl PreparedEngineReclamation {
    pub fn action(&self) -> &EngineReclamation {
        &self.record
    }
    pub fn remove(
        self,
        permit: EngineReclamationPermit,
        control: &JobControl,
    ) -> Result<ReclaimedEngineResource> {
        check(self.expires, control)?;
        if !Arc::ptr_eq(&self.key, &permit.key) || self.record != permit.record {
            return Err(Error::Conflict);
        }
        self.files.remove(|| check(self.expires, control))?;
        Ok(ReclaimedEngineResource {
            _files: self.files,
            version: self.version,
            record: self.record,
            expires: self.expires,
            control: control.clone(),
        })
    }
}
impl ReclaimedEngineResource {
    pub fn action(&self) -> &EngineReclamation {
        &self.record
    }
}
impl Store {
    fn reclamation_version(db: &Connection, expected: &EngineVersion) -> Result<()> {
        let current: EngineVersion = serde_json::from_str(
            &db.query_row(
                "SELECT payload FROM engine_versions WHERE id=?1",
                [expected.id.to_string()],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .ok_or(Error::Unavailable)?,
        )?;
        if current != *expected
            || current.state != Installation::Removed
            || db.query_row(
                "SELECT EXISTS(SELECT 1 FROM engine_active WHERE version=?1)",
                [expected.id.to_string()],
                |r| r.get::<_, bool>(0),
            )?
        {
            return Err(Error::Conflict);
        }
        Ok(())
    }
    fn read_reclamation(db: &Connection, id: Id) -> Result<EngineReclamation> {
        let (version, payload): (String, String) = db
            .query_row(
                "SELECT version,payload FROM engine_reclamations WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;
        if payload.len() > 128 * 1024 {
            return Err(Error::ResourceLimit);
        }
        let record: EngineReclamation = serde_json::from_str(&payload)?;
        record.validate()?;
        if record.id != id || record.version.to_string() != version {
            return Err(Error::Integrity);
        }
        Ok(record)
    }
    pub fn engine_reclamation(&self, id: Id, access: &Access) -> Result<EngineReclamation> {
        Self::require_app(&self.db, access)?;
        Self::read_reclamation(&self.db, id)
    }
    pub fn engine_reclamations(
        &self,
        after: Option<Id>,
        limit: usize,
        access: &Access,
    ) -> Result<EngineReclamations> {
        Self::require_app(&self.db, access)?;
        if !(1..=128).contains(&limit) {
            return Err(Error::ResourceLimit);
        }
        let mut query = self
            .db
            .prepare("SELECT id FROM engine_reclamations WHERE id>?1 ORDER BY id LIMIT ?2")?;
        let ids = query.query_map(
            rusqlite::params![
                after.map(|v| v.to_string()).unwrap_or_default(),
                limit as i64 + 1
            ],
            |r| r.get::<_, String>(0),
        )?;
        let mut actions: Vec<EngineReclamation> = Vec::new();
        let mut next = None;
        let mut bytes = 0usize;
        for id in ids {
            if actions.len() == limit {
                next = actions.last().map(|r| r.id);
                break;
            }
            let record = Self::read_reclamation(&self.db, Id::parse(&id?)?)?;
            let size = serde_json::to_vec(&record)?.len();
            if bytes > (4 * 1024 * 1024usize).saturating_sub(size) {
                next = actions.last().map(|r| r.id);
                break;
            }
            bytes += size;
            actions.push(record);
        }
        Ok(EngineReclamations { actions, next })
    }
    pub fn engine_reclamation_plan(
        &self,
        version: Id,
        resume: Option<Id>,
        access: &Access,
    ) -> Result<EngineReclamationPlan> {
        let version = self.engine_version(version, access)?;
        Self::reclamation_version(&self.db, &version)?;
        if version.local_resource.is_none()
            || version.descriptor.resource_sha256.is_none()
            || version.bytes == 0
        {
            return Err(Error::Unavailable);
        }
        let library = Id::parse(&Self::setting(&self.db, "library_id")?)?;
        let prior = resume
            .map(|id| Self::read_reclamation(&self.db, id))
            .transpose()?;
        if let Some(prior) = &prior {
            prior.matches_version(&version)?;
            if prior.library != library || prior.state != ResourceActionState::Pending {
                return Err(Error::Conflict);
            }
        }
        Ok(EngineReclamationPlan {
            library,
            id: prior.as_ref().map(|r| r.id).unwrap_or_default(),
            requested_ms: prior
                .as_ref()
                .map(|r| r.requested_ms)
                .unwrap_or_else(now_ms),
            expires: access.expires_at(),
            version,
            prior,
        })
    }
    pub fn authorise_engine_reclamation(
        &mut self,
        prepared: &PreparedEngineReclamation,
        control: &JobControl,
        access: &Access,
    ) -> Result<EngineReclamationPermit> {
        Self::require_app(&self.db, access)?;
        check(prepared.expires, control)?;
        prepared.record.validate()?;
        if Id::parse(&Self::setting(&self.db, "library_id")?)? != prepared.record.library {
            return Err(Error::Conflict);
        }
        self.write(|db| {
            Self::require_app(db, access)?;
            check(prepared.expires, control)?;
            Self::reclamation_version(db, &prepared.version)?;
            let present = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM engine_reclamations WHERE id=?1)",
                [prepared.record.id.to_string()],
                |r| r.get::<_, bool>(0),
            )?;
            if present {
                let old = Self::read_reclamation(db, prepared.record.id)?;
                if old != prepared.record && Some(&old) != prepared.prior.as_ref() {
                    return Err(Error::Conflict);
                }
                db.execute(
                    "UPDATE engine_reclamations SET payload=?1 WHERE id=?2",
                    rusqlite::params![
                        serde_json::to_string(&prepared.record)?,
                        prepared.record.id.to_string()
                    ],
                )?;
            } else {
                if prepared.prior.is_some() {
                    return Err(Error::Conflict);
                }
                db.execute(
                    "INSERT INTO engine_reclamations VALUES(?1,?2,?3)",
                    rusqlite::params![
                        prepared.record.id.to_string(),
                        prepared.record.version.to_string(),
                        serde_json::to_string(&prepared.record)?
                    ],
                )?;
            }
            check(prepared.expires, control)?;
            Ok(())
        })?;
        Ok(EngineReclamationPermit {
            record: prepared.record.clone(),
            key: Arc::clone(&prepared.key),
        })
    }
    /// Completed acknowledges deletion of known names at the reviewed private
    /// location, not a measurement of freed device blocks or global SDK shutdown.
    /// Keep the private historical target locator; it is stripped from backups.
    pub fn complete_engine_reclamation(
        &mut self,
        reclaimed: &ReclaimedEngineResource,
        access: &Access,
    ) -> Result<EngineReclamation> {
        Self::require_app(&self.db, access)?;
        check(reclaimed.expires, &reclaimed.control)?;
        if Id::parse(&Self::setting(&self.db, "library_id")?)? != reclaimed.record.library {
            return Err(Error::Conflict);
        }
        let mut done = reclaimed.record.clone();
        done.state = ResourceActionState::Completed;
        if Self::read_reclamation(&self.db, done.id)? == done {
            return Ok(done);
        }
        self.write(|db| {
            Self::require_app(db, access)?;
            check(reclaimed.expires, &reclaimed.control)?;
            Self::reclamation_version(db, &reclaimed.version)?;
            let current = Self::read_reclamation(db, done.id)?;
            if current != reclaimed.record && current != done {
                return Err(Error::Conflict);
            }
            db.execute(
                "UPDATE engine_reclamations SET payload=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&done)?, done.id.to_string()],
            )?;
            check(reclaimed.expires, &reclaimed.control)?;
            Ok(())
        })?;
        Ok(done)
    }
}
