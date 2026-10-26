//! Stored activation preparation owned by the host optional-work scope.
use super::activation::{finish_running, LifecycleInput, Running};
use super::*;
use crate::processing::{Converter, DetachedRuntime, JobControl, Manager, Recogniser};
use crate::store::{EngineActivationPlan, PreparedEngineActivation};
use std::sync::{TryLockError, Weak};

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
            Phase::StoredPrepared(Arc::new(StoredCandidate(Mutex::new(CandidateData {
                prepared,
                previous: None,
            })))),
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

// These cells never escape the private scope. While publishing, the phase pins
// the cell until the writer has released its temporary reference and Manager.
pub(super) struct StoredCandidate(Mutex<CandidateData>);
struct CandidateData {
    prepared: PreparedEngineActivation,
    previous: Option<DetachedRuntime>,
}
pub(in crate::host) struct EnginePublication {
    slot: Weak<Slot>,
    manager: Weak<Mutex<Manager>>,
}
impl HostPreparedEngineActivation {
    pub(in crate::host) fn publication(&self, manager: &Arc<Mutex<Manager>>) -> EnginePublication {
        EnginePublication {
            slot: Arc::downgrade(&self.slot),
            manager: Arc::downgrade(manager),
        }
    }
}
// Created before temporary cell/Manager references. During unwinding those
// references retire first; this guard then queues the pinned SDK cell off-writer.
struct Publishing {
    slot: Arc<Slot>,
    armed: bool,
}
impl Publishing {
    fn finish(&mut self, success: bool) {
        let current = {
            let mut state = self.slot.lease.scope.state();
            let mut phase = self.slot.phase.lock().unwrap_or_else(|p| p.into_inner());
            let old = std::mem::replace(&mut *phase, Phase::Finished);
            if let Phase::StoredPublishing(candidate) = old {
                *phase = if success {
                    Phase::StoredPublished {
                        _candidate: candidate,
                    }
                } else {
                    Phase::StoredPrepared(candidate)
                };
            }
            if let Some(entry) = state.entries.get_mut(&self.slot.authority.ticket) {
                entry.running = None;
            }
            !state.closed && self.slot.authority.check().is_ok()
        };
        self.armed = false;
        if !current {
            self.slot.cancel_idle();
        }
    }
}
impl Drop for Publishing {
    fn drop(&mut self) {
        if self.armed {
            self.finish(false);
            self.slot.cancel_idle();
        }
    }
}
impl RecognitionScope {
    pub(in crate::host) fn publish_engine(
        &self,
        store: &mut Store,
        access: &Access,
        publication: EnginePublication,
    ) -> Result<()> {
        let slot = publication.slot.upgrade().ok_or(Error::Cancelled)?;
        self.check_optional_authority(&slot.authority)?;
        // The synchronous caller retains a strong Manager lease until reply.
        // Never wait for its mutex on the database writer.
        let manager = publication.manager.upgrade().ok_or(Error::Cancelled)?;
        let mut manager_guard = match manager.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return Err(Error::ResourceLimit),
            Err(TryLockError::Poisoned(_)) => return Err(Error::WorkerFailed),
        };
        let mut running = Publishing {
            slot: Arc::clone(&slot),
            armed: false,
        };
        let candidate = {
            let mut state = self.state();
            self.check_optional_authority(&slot.authority)?;
            if state.closed {
                return Err(Error::Cancelled);
            }
            let mut phase = slot.phase.lock().map_err(|_| Error::WorkerFailed)?;
            let Phase::StoredPrepared(candidate) = &*phase else {
                return Err(Error::Conflict);
            };
            let candidate = Arc::clone(candidate);
            state
                .entries
                .get_mut(&slot.authority.ticket)
                .ok_or(Error::Cancelled)?
                .running = Some(thread::current().id());
            *phase = Phase::StoredPublishing(Arc::clone(&candidate));
            running.armed = true;
            candidate
        };
        let result = {
            let mut data = candidate.0.lock().map_err(|_| Error::WorkerFailed)?;
            match store.commit_engine_activation(&mut data.prepared, &mut manager_guard, access) {
                Ok(previous) => {
                    data.previous = previous;
                    Ok(())
                }
                Err(error) => Err(error),
            }
        };
        // No SDK reference may remain on this thread when retirement is enabled.
        drop(candidate);
        drop(manager_guard);
        drop(manager);
        // A successful durable commit stays successful even if pen cancellation
        // arrives after the transaction's final control check.
        running.finish(result.is_ok());
        result
    }
}
