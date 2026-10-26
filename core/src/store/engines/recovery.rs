//! Capture current app authority without touching the private resource directory.
use super::*;
use crate::{processing::JobControl, provisioning::RecoveryPage};
use std::time::Instant;
pub struct EngineRecoveryPlan {
    library: String,
    expires: Option<Instant>,
}
impl Store {
    pub fn engine_recovery_plan(&self, access: &Access) -> Result<EngineRecoveryPlan> {
        Self::require_app(&self.db, access)?;
        Ok(EngineRecoveryPlan {
            library: Self::setting(&self.db, "library_id")?,
            expires: access.expires_at(),
        })
    }
}
impl EngineRecoveryPlan {
    pub fn scan(
        self,
        root: &Path,
        after: Option<Id>,
        limit: usize,
        control: &JobControl,
    ) -> Result<RecoveryPage> {
        crate::provisioning::recovery::scan(root, &self.library, after, limit, || {
            if self.expires.is_some_and(|until| Instant::now() >= until) {
                return Err(Error::Denied);
            }
            control.check()
        })
    }
}
