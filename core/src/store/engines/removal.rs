//! Callback-free transactional logical removal. Resource files are retained until
//! explicit cleanup can prove every runtime/resource lease has retired.
use super::*;
use crate::processing::{DetachedRuntime, JobControl};
pub struct EngineRemovalPlan {
    library: String,
    generation: String,
    version: EngineVersion,
}
impl EngineRemovalPlan {
    pub fn version(&self) -> &EngineVersion {
        &self.version
    }
}
impl Store {
    pub fn engine_removal_plan(&self, id: Id, access: &Access) -> Result<EngineRemovalPlan> {
        let version = self.engine_version(id, access)?;
        Ok(EngineRemovalPlan {
            library: Self::setting(&self.db, "library_id")?,
            generation: Self::setting(&self.db, "generation")?,
            version,
        })
    }
    /// Success returns the detached owner without provider destruction. Drop it
    /// off the writer, after releasing the Manager lock. Old work may still pin it.
    pub fn commit_engine_removal(
        &mut self,
        plan: &EngineRemovalPlan,
        manager: &mut Manager,
        control: &JobControl,
        access: &Access,
    ) -> Result<Option<DetachedRuntime>> {
        Self::require_app(&self.db, access)?;
        control.check()?;
        if Self::setting(&self.db, "library_id")? != plan.library
            || Self::setting(&self.db, "generation")? != plan.generation
            || self.engine_version(plan.version.id, access)? != plan.version
        {
            return Err(Error::Conflict);
        }
        let mut version = plan.version.clone();
        let active = self
            .active_engine(&version.descriptor.id, access)?
            .is_some_and(|v| v.id == version.id);
        if active
            && manager
                .descriptor(&version.descriptor.id)
                .is_some_and(|d| d != &version.descriptor)
        {
            return Err(Error::Conflict);
        }
        if version.state == Installation::Removed {
            return Ok(None);
        }
        version.state = Installation::Removed;
        self.write(|db| {
            Self::require_app(db, access)?;
            control.check()?;
            if Self::setting(db, "generation")? != plan.generation {
                return Err(Error::Conflict);
            }
            db.execute(
                "UPDATE engine_versions SET payload=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&version)?, version.id.to_string()],
            )?;
            db.execute(
                "DELETE FROM engine_active WHERE version=?1",
                [version.id.to_string()],
            )?;
            if active {
                db.execute(
                    "DELETE FROM engine_defaults WHERE engine=?1",
                    [&version.descriptor.id],
                )?;
            }
            control.check()?;
            Ok(())
        })?;
        Ok(if active {
            manager.detach_optional(&version.descriptor.id)
        } else {
            None
        })
    }
}
