//! Bounded app-authorised installation metadata, independent of loaded SDK state.
use super::*;
#[derive(Debug, Clone, Serialize)]
pub struct EngineCatalogueRow {
    pub id: Id,
    pub descriptor: Descriptor,
    pub state: Installation,
    pub bytes: u64,
    pub installed_ms: Option<i64>,
    pub active: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct EngineCatalogue {
    pub versions: Vec<EngineCatalogueRow>,
    pub next: Option<Id>,
}
impl Store {
    pub fn engine_catalogue(
        &self,
        after: Option<Id>,
        limit: usize,
        access: &Access,
    ) -> Result<EngineCatalogue> {
        Self::require_app(&self.db, access)?;
        if limit == 0 || limit > 128 {
            return Err(Error::ResourceLimit);
        }
        let mut query = self.db.prepare("SELECT v.id,v.engine,v.payload,a.version FROM engine_versions v LEFT JOIN engine_active a ON a.engine=v.engine WHERE v.id>?1 ORDER BY v.id LIMIT ?2")?;
        let mut rows = query.query(rusqlite::params![
            after.map(|id| id.to_string()).unwrap_or_default(),
            limit as i64 + 1
        ])?;
        let mut versions = Vec::new();
        let mut used = 0usize;
        let mut next = None;
        while let Some(row) = rows.next()? {
            if versions.len() == limit {
                next = versions.last().map(|v: &EngineCatalogueRow| v.id);
                break;
            }
            let id = Id::parse(&row.get::<_, String>(0)?)?;
            let engine: String = row.get(1)?;
            let version: EngineVersion = serde_json::from_str(&row.get::<_, String>(2)?)?;
            if version.id != id || version.descriptor.id != engine {
                return Err(Error::Integrity);
            }
            let active = row
                .get::<_, Option<String>>(3)?
                .is_some_and(|value| value == id.to_string());
            if active && version.state != Installation::Installed {
                return Err(Error::Integrity);
            }
            let summary = EngineCatalogueRow {
                id,
                descriptor: version.descriptor,
                state: version.state,
                bytes: version.bytes,
                installed_ms: version.installed_ms,
                active,
            };
            let charge = serde_json::to_vec(&summary)?.len();
            if charge > 4 * 1024 * 1024 {
                return Err(Error::ResourceLimit);
            }
            if used > (4 * 1024 * 1024usize).saturating_sub(charge) {
                next = versions.last().map(|v| v.id);
                break;
            }
            used += charge;
            versions.push(summary);
        }
        Ok(EngineCatalogue { versions, next })
    }
}
