//! Files stay on optional workers; SQL only borrows a phase-pinned candidate.
use super::activation::{finish_running, LifecycleInput, Running};
use super::resource_import::{take_running, worker};
use super::*;
use crate::{processing::JobControl, store::*};
use std::path::Path;

// Bounded record, ledger descriptor and metadata-only permit can coexist.
pub(super) const RESERVED_BYTES: usize = 512 * 1024;
pub struct HostEngineDiscardPlan {
    slot: Arc<Slot>,
    id: Id,
    transferred: bool,
}
pub struct HostPreparedEngineDiscard {
    slot: Arc<Slot>,
    id: Id,
    transferred: bool,
}
pub struct HostDiscardedEngineImport {
    slot: Arc<Slot>,
    id: Id,
}
pub(super) struct DiscardCandidate(Mutex<Data>);
enum Data {
    Prepared {
        value: Box<PreparedEngineDiscard>,
        permit: Option<Box<EngineDiscardPermit>>,
    },
    Removed(Box<DiscardedEngineImport>),
    Empty,
}
pub(in crate::host) struct DiscardPublication {
    slot: Weak<Slot>,
    control: JobControl,
}
impl Drop for HostEngineDiscardPlan {
    fn drop(&mut self) {
        if !self.transferred {
            self.slot.cancel_idle();
        }
    }
}
impl Drop for HostPreparedEngineDiscard {
    fn drop(&mut self) {
        if !self.transferred {
            self.slot.cancel_idle();
        }
    }
}
impl Drop for HostDiscardedEngineImport {
    fn drop(&mut self) {
        self.slot.cancel_idle();
    }
}
impl HostEngineDiscardPlan {
    pub fn action_id(&self) -> Id {
        self.id
    }
    pub fn prepare(
        mut self,
        root: &Path,
        control: &JobControl,
    ) -> Result<HostPreparedEngineDiscard> {
        let plan = take_running(&self.slot, |phase| match phase {
            Phase::DiscardPlan(plan) => plan.take(),
            _ => None,
        })?;
        self.transferred = true;
        let mut running = Running {
            slot: Arc::clone(&self.slot),
            armed: true,
        };
        let control = control.linked_authority(&self.slot.authority);
        let prepared = worker(|| plan.prepare(root, &control))?;
        finish_running(
            &self.slot,
            Phase::DiscardCandidate(Arc::new(DiscardCandidate(Mutex::new(Data::Prepared {
                value: Box::new(prepared),
                permit: None,
            })))),
            &mut running,
        )?;
        Ok(HostPreparedEngineDiscard {
            slot: Arc::clone(&self.slot),
            id: self.id,
            transferred: false,
        })
    }
}
impl HostPreparedEngineDiscard {
    pub fn action_id(&self) -> Id {
        self.id
    }
    pub(in crate::host) fn publication(&self, control: &JobControl) -> DiscardPublication {
        DiscardPublication {
            slot: Arc::downgrade(&self.slot),
            control: control.linked_authority(&self.slot.authority),
        }
    }
    /// Requires a durable Host authorisation first. Errors may follow physical deletion.
    pub fn remove(mut self, control: &JobControl) -> Result<HostDiscardedEngineImport> {
        let candidate = take_running(&self.slot, |phase| match phase {
            Phase::DiscardCandidate(candidate) => Some(Arc::clone(candidate)),
            _ => None,
        })?;
        self.transferred = true;
        let mut running = Running {
            slot: Arc::clone(&self.slot),
            armed: true,
        };
        let control = control.linked_authority(&self.slot.authority);
        let removed = worker(|| {
            let data = std::mem::replace(
                &mut *candidate.0.lock().map_err(|_| Error::WorkerFailed)?,
                Data::Empty,
            );
            let result = match data {
                Data::Prepared {
                    value,
                    permit: Some(permit),
                } => value.remove(*permit, &control),
                _ => Err(Error::Denied),
            };
            drop(candidate);
            result
        })?;
        finish_running(
            &self.slot,
            Phase::DiscardCandidate(Arc::new(DiscardCandidate(Mutex::new(Data::Removed(
                Box::new(removed),
            ))))),
            &mut running,
        )?;
        Ok(HostDiscardedEngineImport {
            slot: Arc::clone(&self.slot),
            id: self.id,
        })
    }
}
impl HostDiscardedEngineImport {
    pub fn action_id(&self) -> Id {
        self.id
    }
    pub(in crate::host) fn publication(&self, control: &JobControl) -> DiscardPublication {
        DiscardPublication {
            slot: Arc::downgrade(&self.slot),
            control: control.linked_authority(&self.slot.authority),
        }
    }
}
struct Publishing {
    slot: Arc<Slot>,
    armed: bool,
}
impl Publishing {
    fn finish(&mut self) {
        let current = {
            let mut state = self.slot.lease.scope.state();
            let mut phase = self.slot.phase.lock().unwrap_or_else(|p| p.into_inner());
            if let Phase::DiscardPublishing(candidate) =
                std::mem::replace(&mut *phase, Phase::Finished)
            {
                *phase = Phase::DiscardCandidate(candidate);
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
    pub(in crate::host) fn engine_discard_plan(
        self: &Arc<Self>,
        store: &Store,
        access: &Access,
        version: Id,
        confirmed: bool,
        resume: Option<Id>,
    ) -> Result<HostEngineDiscardPlan> {
        let plan = store.engine_discard_plan(version, confirmed, resume, access)?;
        let id = plan.id();
        Ok(HostEngineDiscardPlan {
            slot: self.lifecycle_slot(
                store,
                access,
                LifecycleInput::DiscardPlan(Box::new(plan)),
            )?,
            id,
            transferred: false,
        })
    }
    pub(in crate::host) fn publish_discard(
        &self,
        store: &mut Store,
        access: &Access,
        publication: DiscardPublication,
        complete: bool,
    ) -> Result<EngineResourceAction> {
        let slot = publication.slot.upgrade().ok_or(Error::Cancelled)?;
        self.check_optional_authority(&slot.authority)?;
        publication.control.check()?;
        Store::require_app(&store.db, access)?;
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
            let Phase::DiscardCandidate(candidate) = &*phase else {
                return Err(Error::Cancelled);
            };
            let candidate = Arc::clone(candidate);
            state
                .entries
                .get_mut(&slot.authority.ticket)
                .ok_or(Error::Cancelled)?
                .running = Some(thread::current().id());
            *phase = Phase::DiscardPublishing(Arc::clone(&candidate));
            publishing.armed = true;
            candidate
        };
        let result = {
            let mut data = candidate.0.lock().map_err(|_| Error::WorkerFailed)?;
            match (&mut *data, complete) {
                (Data::Prepared { value, permit }, false) => {
                    let approved =
                        store.authorise_engine_discard(value, &publication.control, access)?;
                    *permit = Some(Box::new(approved));
                    Ok(value.action().clone())
                }
                (Data::Removed(removed), true) => store.complete_engine_discard(removed, access),
                _ => Err(Error::Conflict),
            }
        };
        // No file guard may be the writer's last reference when cancellation retires it.
        drop(candidate);
        publishing.finish();
        // Preserve known SQL success even if cancellation races its reply.
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{host::Host, provisioning::resource_digest};
    use std::time::{Duration, Instant};
    fn check_writer_pin(complete: bool) {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path().join("library.db")).unwrap();
        let mut descriptor: crate::processing::Descriptor = serde_json::from_str(r#"{"id":"synthetic-discard-pin","version":"1","adapter_version":"1","family":"Recogniser","input":"OrderedStrokesV1","tasks":["Prose"],"internals":"StrokeOnlyVerified","offline":true,"content_egress":false,"personal_use_evidence":"app-owned phase ownership fixture","redistributable_resources":false,"resource_sha256":null,"runtime_contract":"stem-processing-v1"}"#).unwrap();
        descriptor.resource_sha256 = Some(resource_digest(b"model"));
        let control = || JobControl::new(Duration::from_secs(30)).unwrap();
        let models = root.path().join("models");
        let transfer = store
            .engine_import_plan(descriptor, 5, true, &Access::public())
            .unwrap()
            .begin(&models, &control())
            .unwrap();
        let id = transfer.id();
        drop(transfer);
        drop(store);
        let host = Arc::new(Host::open(root.path().join("library.db"), 8).unwrap());
        let prepared = host
            .engine_discard_plan(id, true, None)
            .unwrap()
            .prepare(&models, &control())
            .unwrap();
        let action_id = prepared.action_id();
        let slot = Arc::clone(&prepared.slot);
        let writer = Arc::clone(&host);
        let publish: Box<dyn FnOnce() -> Result<EngineResourceAction> + Send> = if complete {
            host.authorise_engine_discard(&prepared, &control())
                .unwrap();
            let removed = prepared.remove(&control()).unwrap();
            Box::new(move || {
                writer.complete_engine_discard(
                    &removed,
                    &JobControl::new(Duration::from_secs(30)).unwrap(),
                )
            })
        } else {
            Box::new(move || {
                writer.authorise_engine_discard(
                    &prepared,
                    &JobControl::new(Duration::from_secs(30)).unwrap(),
                )
            })
        };
        let candidate = match &*slot.phase.lock().unwrap() {
            Phase::DiscardCandidate(candidate) => Arc::clone(candidate),
            _ => panic!("missing candidate"),
        };
        let data = candidate.0.lock().unwrap();
        let job = thread::spawn(publish);
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if matches!(*slot.phase.lock().unwrap(), Phase::DiscardPublishing(_)) {
                break;
            }
            if Instant::now() >= until {
                drop(data);
                let _ = job.join();
                panic!("writer did not enter publication");
            }
            thread::yield_now();
        }
        let pen = Id::new();
        host.set_capture_priority(pen, true).unwrap();
        host.set_capture_priority(pen, false).unwrap();
        let pinned = matches!(*slot.phase.lock().unwrap(), Phase::DiscardPublishing(_));
        let coordination = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(models.join(format!("{id}.owner.lock")))
            .unwrap();
        assert!(matches!(
            coordination.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(coordination);
        drop(data);
        assert!(job.join().unwrap().is_err());
        assert!(
            pinned,
            "idle cancellation retired a writer-borrowed file guard"
        );
        if complete {
            assert_eq!(
                host.engine_resource_action(action_id).unwrap().state,
                ResourceActionState::Pending
            );
            assert!(!models.join(format!("{id}.part")).exists());
        } else {
            assert!(host
                .engine_resource_actions(None, 8)
                .unwrap()
                .actions
                .is_empty());
            assert!(models.join(format!("{id}.part")).exists());
        }
        drop(slot);
        drop(candidate);
        host.close().unwrap();
    }
    #[test]
    fn capture_cannot_retire_an_authorising_discard_cell() {
        check_writer_pin(false);
    }
    #[test]
    fn capture_cannot_retire_a_completing_discard_cell() {
        check_writer_pin(true);
    }
}
