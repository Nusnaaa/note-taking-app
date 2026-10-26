//! Optional host-owned activation. The writer reserves metadata only. Resource
//! verification/probe callbacks and actual cleanup belong to optional workers.
use super::*;
use crate::processing::{JobControl, Manager};

pub struct HostActivationSource {
    slot: Arc<Slot>,
    transferred: bool,
}
pub struct HostActivationWork {
    slot: Arc<Slot>,
    transferred: bool,
}
pub struct HostCompletedActivation {
    slot: Arc<Slot>,
}
impl Drop for HostActivationSource {
    fn drop(&mut self) {
        if !self.transferred {
            self.slot.cancel_idle();
        }
    }
}
impl Drop for HostActivationWork {
    fn drop(&mut self) {
        if !self.transferred {
            self.slot.cancel_idle();
        }
    }
}
impl Drop for HostCompletedActivation {
    fn drop(&mut self) {
        self.slot.cancel_idle();
    }
}
struct Running {
    slot: Arc<Slot>,
    armed: bool,
}
impl Drop for Running {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let old = {
            let mut phase = self.slot.phase.lock().unwrap_or_else(|p| p.into_inner());
            std::mem::replace(&mut *phase, Phase::Finished)
        };
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(old))).is_err();
        self.slot.lease.scope.release(self.slot.authority.ticket);
        if failed {
            self.slot.lease.scope.close();
        }
    }
}
fn finish_running(slot: &Arc<Slot>, phase: Phase, running: &mut Running) -> Result<()> {
    let current = {
        let mut state = slot.lease.scope.state();
        let mut stored = slot.phase.lock().unwrap_or_else(|p| p.into_inner());
        *stored = phase; // previous Running owns no callback/resource payload
        let current = !state.closed && slot.authority.check().is_ok();
        if let Some(entry) = state.entries.get_mut(&slot.authority.ticket) {
            entry.running = None;
        } else {
            return Err(Error::Cancelled);
        }
        current
    };
    running.armed = false;
    if !current {
        slot.cancel_idle();
        return Err(Error::Cancelled);
    }
    Ok(())
}
impl RecognitionScope {
    pub(in crate::host) fn activation_source(
        self: &Arc<Self>,
        store: &Store,
        access: &Access,
    ) -> Result<HostActivationSource> {
        Store::require_app(&store.db, access)?;
        {
            let state = self.state();
            if state.closed {
                return Err(Error::WorkerFailed);
            }
            if !state.capture.is_empty() || state.entries.len() >= self.capacity {
                return Err(Error::ResourceLimit);
            }
        }
        self.ensure_retirement_lane()?;
        let mut state = self.state();
        if state.closed {
            return Err(Error::WorkerFailed);
        }
        if !state.capture.is_empty()
            || state.entries.len() >= self.capacity
            || 128 > MAX_SOURCE_BYTES.saturating_sub(state.entries.values().map(|e| e.bytes).sum())
        {
            return Err(Error::ResourceLimit);
        }
        let ticket = Id::new();
        let slot = Arc::new(Slot {
            authority: WorkAuthority {
                owner: self.owner,
                ticket,
                valid: Arc::clone(&state.valid),
                until: access.expires_at(),
            },
            phase: Mutex::new(Phase::ActivationInput),
            lease: SlotLease {
                scope: Arc::clone(self),
                ticket,
            },
        });
        state.entries.insert(
            ticket,
            Entry {
                slot: Arc::downgrade(&slot),
                bytes: 128,
                running: None,
                retiring: None,
                retire_pending: false,
            },
        );
        Ok(HostActivationSource {
            slot,
            transferred: false,
        })
    }
    pub(in crate::host) fn complete_activation(
        &self,
        manager: &mut Manager,
        completed: &HostCompletedActivation,
    ) -> Result<()> {
        let state = self.state();
        if completed.slot.authority.owner != self.owner {
            return Err(Error::Denied);
        }
        completed.slot.authority.check()?;
        if state.closed {
            return Err(Error::Cancelled);
        }
        let mut phase = completed
            .slot
            .phase
            .lock()
            .map_err(|_| Error::WorkerFailed)?;
        let Phase::ActivationResult(Some(result)) = &mut *phase else {
            return Err(Error::Cancelled);
        };
        manager.complete_activation(result)
    }
}
impl HostActivationSource {
    pub fn ticket(&self) -> Id {
        self.slot.authority.ticket
    }
    /// Call on an optional worker. No scope/phase lock remains held while the
    /// manager hashes caller-owned local resource bytes.
    pub fn prepare(
        mut self,
        manager: &mut Manager,
        id: &str,
        resource: Option<&[u8]>,
        consented: bool,
    ) -> Result<HostActivationWork> {
        self.slot.authority.check()?;
        if !consented {
            return Err(Error::Denied);
        }
        let bytes = charge(manager.descriptor(id).ok_or(Error::EngineUnavailable)?)?;
        {
            let mut state = self.slot.lease.scope.state();
            self.slot.authority.check()?;
            if state.closed {
                return Err(Error::Cancelled);
            }
            let old = state
                .entries
                .get(&self.ticket())
                .ok_or(Error::Cancelled)?
                .bytes;
            let used: usize = state.entries.values().map(|e| e.bytes).sum();
            if bytes > MAX_SOURCE_BYTES.saturating_sub(used.saturating_sub(old)) {
                return Err(Error::ResourceLimit);
            }
            let mut phase = self.slot.phase.lock().map_err(|_| Error::WorkerFailed)?;
            if !matches!(*phase, Phase::ActivationInput) {
                return Err(Error::Cancelled);
            }
            let entry = state
                .entries
                .get_mut(&self.ticket())
                .ok_or(Error::Cancelled)?;
            entry.bytes = bytes;
            entry.running = Some(thread::current().id());
            *phase = Phase::Running;
        }
        let mut running = Running {
            slot: Arc::clone(&self.slot),
            armed: true,
        };
        let work = manager.prepare_activation(id, resource, true)?;
        finish_running(
            &self.slot,
            Phase::ActivationPrepared(Some(Box::new(work))),
            &mut running,
        )?;
        self.transferred = true;
        Ok(HostActivationWork {
            slot: Arc::clone(&self.slot),
            transferred: false,
        })
    }
}
impl HostActivationWork {
    pub fn ticket(&self) -> Id {
        self.slot.authority.ticket
    }
    pub fn run(mut self, control: &JobControl) -> Result<HostCompletedActivation> {
        let work = {
            let mut state = self.slot.lease.scope.state();
            self.slot.authority.check()?;
            if state.closed {
                return Err(Error::Cancelled);
            }
            let mut phase = self.slot.phase.lock().map_err(|_| Error::WorkerFailed)?;
            let Phase::ActivationPrepared(work) = &mut *phase else {
                return Err(Error::Cancelled);
            };
            let work = work.take().ok_or(Error::Cancelled)?;
            state
                .entries
                .get_mut(&self.ticket())
                .ok_or(Error::Cancelled)?
                .running = Some(thread::current().id());
            *phase = Phase::Running;
            work
        };
        self.transferred = true;
        let mut running = Running {
            slot: Arc::clone(&self.slot),
            armed: true,
        };
        let completed = (*work).run(&control.linked_authority(&self.slot.authority));
        finish_running(
            &self.slot,
            Phase::ActivationResult(Some(Box::new(completed))),
            &mut running,
        )?;
        Ok(HostCompletedActivation {
            slot: Arc::clone(&self.slot),
        })
    }
}
impl HostCompletedActivation {
    pub fn ticket(&self) -> Id {
        self.slot.authority.ticket
    }
}
