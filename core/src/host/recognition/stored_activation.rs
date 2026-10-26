//! Stored activation preparation owned by the host optional-work scope.
use super::activation::{finish_running, LifecycleInput, Running};
use super::*;
use crate::processing::{Converter, JobControl, Recogniser};
use crate::store::{EngineActivationPlan, PreparedEngineActivation};

pub struct HostEngineActivationPlan {
    slot: Arc<Slot>,
    transferred: bool,
}
pub struct HostPreparedEngineActivation {
    slot: Arc<Slot>,
}
impl Drop for HostEngineActivationPlan {
    fn drop(&mut self) {
        if !self.transferred {
            self.slot.cancel_idle();
        }
    }
}
impl Drop for HostPreparedEngineActivation {
    fn drop(&mut self) {
        self.slot.cancel_idle();
    }
}
impl RecognitionScope {
    pub(in crate::host) fn engine_activation_plan(
        self: &Arc<Self>,
        store: &Store,
        access: &Access,
        id: Id,
    ) -> Result<HostEngineActivationPlan> {
        let plan = store.engine_activation_plan(id, access)?;
        let slot =
            self.lifecycle_slot(store, access, LifecycleInput::StoredPlan(Box::new(plan)))?;
        Ok(HostEngineActivationPlan {
            slot,
            transferred: false,
        })
    }
}
impl HostEngineActivationPlan {
    pub fn ticket(&self) -> Id {
        self.slot.authority.ticket
    }
    fn prepare(
        mut self,
        control: &JobControl,
        run: impl FnOnce(EngineActivationPlan, &JobControl) -> Result<PreparedEngineActivation>,
    ) -> Result<HostPreparedEngineActivation> {
        let plan = {
            let mut state = self.slot.lease.scope.state();
            self.slot.authority.check()?;
            if state.closed {
                return Err(Error::Cancelled);
            }
            let mut phase = self.slot.phase.lock().map_err(|_| Error::WorkerFailed)?;
            let Phase::StoredPlan(plan) = &mut *phase else {
                return Err(Error::Cancelled);
            };
            let plan = plan.take().ok_or(Error::Cancelled)?;
            state
                .entries
                .get_mut(&self.ticket())
                .ok_or(Error::Cancelled)?
                .running = Some(thread::current().id());
            *phase = Phase::Running;
            plan
        };
        self.transferred = true;
        let mut running = Running {
            slot: Arc::clone(&self.slot),
            armed: true,
        };
        let control = control.linked_authority(&self.slot.authority);
        let prepared =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(*plan, &control)))
                .map_err(|_| Error::WorkerFailed)??;
        finish_running(
            &self.slot,
            Phase::StoredPrepared {
                _prepared: Box::new(prepared),
            },
            &mut running,
        )?;
        Ok(HostPreparedEngineActivation {
            slot: Arc::clone(&self.slot),
        })
    }
    /// The provider is passed only to this optional worker, never to the writer.
    pub fn prepare_recogniser(
        self,
        adapter: Box<dyn Recogniser>,
        max_load_bytes: u64,
        control: &JobControl,
    ) -> Result<HostPreparedEngineActivation> {
        self.prepare(control, move |plan, control| {
            plan.prepare_recogniser(adapter, max_load_bytes, control)
        })
    }
    pub fn prepare_converter(
        self,
        adapter: Box<dyn Converter>,
        max_load_bytes: u64,
        control: &JobControl,
    ) -> Result<HostPreparedEngineActivation> {
        self.prepare(control, move |plan, control| {
            plan.prepare_converter(adapter, max_load_bytes, control)
        })
    }
}
impl HostPreparedEngineActivation {
    pub fn ticket(&self) -> Id {
        self.slot.authority.ticket
    }
}
