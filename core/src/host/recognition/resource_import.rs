//! Host-owned resource import. Publication borrows metadata on the writer.
//! Actual reader/error/ownership disposal stays on optional retirement workers.
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
pub(super) fn take_running<T>(
    slot: &Arc<Slot>,
    take: impl FnOnce(&mut Phase) -> Option<T>,
) -> Result<T> {
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
pub(super) fn worker<T>(run: impl FnOnce() -> Result<T>) -> Result<T> {
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
pub(super) struct ImportCandidate(Mutex<crate::store::PreparedEngineImport>);
struct Publishing {
    slot: Arc<Slot>,
    armed: bool,
}
impl Publishing {
    fn finish(&mut self) {
        let current = {
            let mut state = self.slot.lease.scope.state();
            let mut phase = self.slot.phase.lock().unwrap_or_else(|p| p.into_inner());
            if let Phase::ImportPublishing(candidate) =
                std::mem::replace(&mut *phase, Phase::Finished)
            {
                *phase = Phase::ImportPrepared(candidate);
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
            self.finish();
            self.slot.cancel_idle();
        }
    }
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
        let mut publishing = Publishing {
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
            let Phase::ImportPrepared(candidate) = &*phase else {
                return Err(Error::Cancelled);
            };
            let candidate = Arc::clone(candidate);
            state
                .entries
                .get_mut(&slot.authority.ticket)
                .ok_or(Error::Cancelled)?
                .running = Some(thread::current().id());
            *phase = Phase::ImportPublishing(Arc::clone(&candidate));
            publishing.armed = true;
            candidate
        };
        let result = {
            let prepared = candidate.0.lock().map_err(|_| Error::WorkerFailed)?;
            store.commit_engine_import(&prepared, access)
        };
        // The phase pins the ownership guard until writer temporaries have gone.
        drop(candidate);
        publishing.finish();
        // Preserve known SQL success even when cancellation races its reply.
        result
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
            Phase::ImportPrepared(Arc::new(ImportCandidate(Mutex::new(prepared)))),
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

#[cfg(test)]
mod ownership_tests {
    use super::*;
    use crate::{
        host::Host,
        provisioning::{resource_digest, Transfer},
    };
    use std::{
        io::Cursor,
        time::{Duration, Instant},
    };
    #[test]
    fn capture_cannot_retire_the_import_cell_while_sql_publication_is_in_progress() {
        let root = tempfile::tempdir().unwrap();
        let host = Arc::new(Host::open(root.path().join("library.db"), 8).unwrap());
        let bytes = b"owned import race fixture";
        let descriptor=serde_json::from_value(serde_json::json!({"id":"synthetic-import-race","version":"1","adapter_version":"1","family":"Recogniser","input":"OrderedStrokesV1","tasks":["Prose"],"internals":"StrokeOnlyVerified","offline":true,"content_egress":false,"personal_use_evidence":"app-owned import race fixture","redistributable_resources":false,"resource_sha256":resource_digest(bytes),"runtime_contract":"stem-processing-v1"})).unwrap();
        let control = JobControl::new(Duration::from_secs(30)).unwrap();
        let models = root.path().join("models");
        let transfer = host
            .engine_import_plan(descriptor, bytes.len() as u64, true)
            .unwrap()
            .start(&models, None, &control)
            .unwrap();
        let id = transfer.id();
        let prepared = Arc::new(transfer.prepare(Cursor::new(bytes), &control).unwrap());
        let candidate = {
            let phase = prepared.slot.phase.lock().unwrap();
            let Phase::ImportPrepared(candidate) = &*phase else {
                panic!("unprepared fixture")
            };
            Arc::clone(candidate)
        };
        let data = candidate.0.lock().unwrap();
        let publisher = Arc::clone(&host);
        let publication = Arc::clone(&prepared);
        let job = thread::spawn(move || publisher.commit_engine_import(&publication));
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if matches!(
                *prepared.slot.phase.lock().unwrap(),
                Phase::ImportPublishing(_)
            ) {
                break;
            }
            if Instant::now() >= until {
                drop(data);
                drop(candidate);
                let _ = job.join();
                panic!("publication did not enter");
            }
            thread::yield_now();
        }
        let pen = Id::new();
        host.set_capture_priority(pen, true).unwrap();
        host.set_capture_priority(pen, false).unwrap();
        let pinned = matches!(
            *prepared.slot.phase.lock().unwrap(),
            Phase::ImportPublishing(_)
        );
        assert!(matches!(
            Transfer::resume(&models, id),
            Err(Error::ResourceLimit)
        ));
        drop(data);
        drop(candidate);
        assert!(job.join().unwrap().is_err());
        assert!(pinned, "capture released a publishing import cell");
        drop(prepared);
        let until = Instant::now() + Duration::from_secs(10);
        while host.recognition_status().reserved != 0 {
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            Transfer::resume(&models, id).unwrap().committed_bytes(),
            bytes.len() as u64
        );
        assert!(host.engine_catalogue(None, 8).unwrap().versions.is_empty());
        host.close().unwrap();
    }
}
