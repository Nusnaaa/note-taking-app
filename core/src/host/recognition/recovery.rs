//! Read-only depot discovery held by the same optional lifecycle as imports.
use super::activation::{finish_running, LifecycleInput, Running};
use super::resource_import::{take_running, worker};
use super::*;
use crate::{processing::JobControl, provisioning::RecoveryPage};
use std::path::Path;
pub struct HostEngineRecoveryPlan {
    slot: Arc<Slot>,
}
impl Drop for HostEngineRecoveryPlan {
    fn drop(&mut self) {
        self.slot.cancel_idle();
    }
}
impl RecognitionScope {
    pub(in crate::host) fn engine_recovery_plan(
        self: &Arc<Self>,
        store: &Store,
        access: &Access,
    ) -> Result<HostEngineRecoveryPlan> {
        let plan = store.engine_recovery_plan(access)?;
        Ok(HostEngineRecoveryPlan {
            slot: self.lifecycle_slot(
                store,
                access,
                LifecycleInput::RecoveryPlan(Box::new(plan)),
            )?,
        })
    }
}
impl HostEngineRecoveryPlan {
    pub fn scan(
        self,
        root: &Path,
        after: Option<Id>,
        limit: usize,
        control: &JobControl,
    ) -> Result<RecoveryPage> {
        let plan = take_running(&self.slot, |phase| match phase {
            Phase::RecoveryPlan(value) => value.take(),
            _ => None,
        })?;
        let mut running = Running {
            slot: Arc::clone(&self.slot),
            armed: true,
        };
        let linked = control.linked_authority(&self.slot.authority);
        let page = worker(|| plan.scan(root, after, limit, &linked))?;
        finish_running(&self.slot, Phase::Finished, &mut running)?;
        Ok(page)
    }
}
