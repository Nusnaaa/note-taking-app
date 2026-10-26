//! Host-owned resource import. Only metadata reaches the writer. Reader callbacks
//! and error/resource disposal stay inside the optional worker's running scope.
use super::activation::{finish_running, LifecycleInput, Running};
use super::*;
use crate::{
    processing::{Descriptor, JobControl},
    store::{EngineImportPlan, EngineVersion},
};
use std::{io::Read, path::Path};

pub struct HostEngineImportPlan {
    slot: Arc<Slot>,
    transferred: bool,
}
pub struct HostEngineImportTransfer {
    slot: Arc<Slot>,
    id: Id,
    committed: u64,
    transferred: bool,
}
pub struct HostPreparedEngineImport {
    slot: Arc<Slot>,
    id: Id,
}
pub(in crate::host) struct ImportPublication {
    slot: Weak<Slot>,
}
impl Drop for HostEngineImportPlan {
    fn drop(&mut self) {
        if !self.transferred {
            self.slot.cancel_idle();
        }
    }
}
impl Drop for HostEngineImportTransfer {
    fn drop(&mut self) {
        if !self.transferred {
            self.slot.cancel_idle();
        }
    }
}
impl Drop for HostPreparedEngineImport {
    fn drop(&mut self) {
        self.slot.cancel_idle();
    }
}
fn take_running<T>(slot: &Arc<Slot>, take: impl FnOnce(&mut Phase) -> Option<T>) -> Result<T> {
    let mut state = slot.lease.scope.state();
    slot.authority.check()?;
    if state.closed {
        return Err(Error::Cancelled);
    }
    let mut phase = slot.phase.lock().map_err(|_| Error::WorkerFailed)?;
    let value = take(&mut phase).ok_or(Error::Cancelled)?;
    state
        .entries
        .get_mut(&slot.authority.ticket)
        .ok_or(Error::Cancelled)?
        .running = Some(thread::current().id());
    *phase = Phase::Running;
    Ok(value)
}
// In particular, do not let a provider-owned io::Error payload escape the running
// entry and release library ownership before its actual destructor returns.
fn worker<T>(run: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match run() {
        Err(Error::Io(error)) => {
            let safe = io::Error::from(error.kind());
            drop(error);
            Err(Error::Io(safe))
        }
        result => result,
    }))
    .map_err(|_| Error::WorkerFailed)?
}
impl RecognitionScope {
    pub(in crate::host) fn engine_import_plan(
        self: &Arc<Self>,
        store: &Store,
        access: &Access,
        descriptor: Descriptor,
        total: u64,
        consented: bool,
    ) -> Result<HostEngineImportPlan> {
        let plan = store.engine_import_plan(descriptor, total, consented, access)?;
        let slot =
            self.lifecycle_slot(store, access, LifecycleInput::ImportPlan(Box::new(plan)))?;
        Ok(HostEngineImportPlan {
            slot,
            transferred: false,
        })
    }
    pub(in crate::host) fn commit_engine_import(
        &self,
        store: &mut Store,
        access: &Access,
        publication: ImportPublication,
    ) -> Result<EngineVersion> {
        let slot = publication.slot.upgrade().ok_or(Error::Cancelled)?;
        self.check_optional_authority(&slot.authority)?;
        let prepared = take_running(&slot, |phase| match phase {
            Phase::ImportPrepared(value) => value.take(),
            _ => None,
        })?;
        let mut running = Running {
            slot: Arc::clone(&slot),
            armed: true,
        };
        let result = store.commit_engine_import(&prepared, access);
        let settled = finish_running(&slot, Phase::ImportPrepared(Some(prepared)), &mut running);
        // Pen cancellation after the final transaction check cannot turn a known
        // committed metadata receipt into a rollback/no-effect claim.
        match result {
            Ok(version) => Ok(version),
            Err(error) => {
                let _ = settled;
                Err(error)
            }
        }
    }
}
impl HostEngineImportPlan {
    /// Create or resume app-private files off the writer. The returned stable ID
    /// is available before opening/reading the selected model input.
    pub fn start(
        mut self,
        root: &Path,
        resume: Option<Id>,
        control: &JobControl,
    ) -> Result<HostEngineImportTransfer> {
        let plan: Box<EngineImportPlan> = take_running(&self.slot, |phase| match phase {
            Phase::ImportPlan(value) => value.take(),
            _ => None,
        })?;
        self.transferred = true;
        let mut running = Running {
            slot: Arc::clone(&self.slot),
            armed: true,
        };
        let control = control.linked_authority(&self.slot.authority);
        let transfer = worker(|| match resume {
            Some(id) => plan.resume(root, id, &control),
            None => plan.begin(root, &control),
        })?;
        let id = transfer.id();
        let committed = transfer.committed_bytes();
        finish_running(
            &self.slot,
            Phase::ImportTransfer(Some(Box::new(transfer))),
            &mut running,
        )?;
        Ok(HostEngineImportTransfer {
            slot: Arc::clone(&self.slot),
            id,
            committed,
            transferred: false,
        })
    }
}
impl HostEngineImportTransfer {
    pub fn id(&self) -> Id {
        self.id
    }
    pub fn committed_bytes(&self) -> u64 {
        self.committed
    }

    /// Reader begins at the explicitly resumed durable offset. Its owned resource
    /// and any error payload retire before running admission is released.
    pub fn prepare<R: Read>(
        self,
        reader: R,
        control: &JobControl,
    ) -> Result<HostPreparedEngineImport> {
        self.prepare_with(control, move |_| Ok(reader))
    }
    /// Construct the owned input only after admission and pass the exact linked
    /// host/capture/access control to foreign input callbacks.
    pub fn prepare_with<R: Read>(
        mut self,
        control: &JobControl,
        factory: impl FnOnce(&JobControl) -> Result<R>,
    ) -> Result<HostPreparedEngineImport> {
        let mut transfer = take_running(&self.slot, |phase| match phase {
            Phase::ImportTransfer(value) => value.take(),
            _ => None,
        })?;
        self.transferred = true;
        let mut running = Running {
            slot: Arc::clone(&self.slot),
            armed: true,
        };
        let control = control.linked_authority(&self.slot.authority);
        let prepared = worker(move || {
            let mut reader = factory(&control)?;
            transfer.copy_remaining(&mut reader, &control)?;
            transfer.finish(&control)
        })?;
        finish_running(
            &self.slot,
            Phase::ImportPrepared(Some(Box::new(prepared))),
            &mut running,
        )?;
        Ok(HostPreparedEngineImport {
            slot: Arc::clone(&self.slot),
            id: self.id,
        })
    }
}
impl HostPreparedEngineImport {
    pub fn id(&self) -> Id {
        self.id
    }
    pub(in crate::host) fn publication(&self) -> ImportPublication {
        ImportPublication {
            slot: Arc::downgrade(&self.slot),
        }
    }
}
