//! Files stay on optional workers; SQL only borrows a phase-pinned candidate.
use super::activation::{finish_running, LifecycleInput, Running};
use super::resource_import::{take_running, worker};
use super::*;
use crate::{processing::JobControl, store::*};
use std::path::Path;

// Bounded record, ledger descriptor and metadata-only permit can coexist.
pub(super) const RESERVED_BYTES: usize = 1024 * 1024;
pub struct HostEngineReclamationPlan {
    slot: Arc<Slot>,
    id: Id,
    transferred: bool,
}
pub struct HostPreparedEngineReclamation {
    slot: Arc<Slot>,
    id: Id,
    transferred: bool,
}
pub struct HostReclaimedEngineResource {
    slot: Arc<Slot>,
    id: Id,
}
pub(super) struct ReclamationCandidate(Mutex<Data>);
enum Data {
    Reviewed(Box<EngineReclamationReview>),
    Prepared {
        value: Box<PreparedEngineReclamation>,
        permit: Option<Box<EngineReclamationPermit>>,
    },
    Removed(Box<ReclaimedEngineResource>),
    Empty,
}
pub(in crate::host) struct ReclamationPublication {
    slot: Weak<Slot>,
    control: JobControl,
}
impl Drop for HostEngineReclamationPlan {
    fn drop(&mut self) {
        if !self.transferred {
            self.slot.cancel_idle();
        }
    }
}
impl Drop for HostPreparedEngineReclamation {
    fn drop(&mut self) {
        if !self.transferred {
            self.slot.cancel_idle();
        }
    }
}
impl Drop for HostReclaimedEngineResource {
    fn drop(&mut self) {
        self.slot.cancel_idle();
    }
}
pub struct HostReviewedEngineReclamation {
    slot: Arc<Slot>,
    summary: EngineReclamationSummary,
    transferred: bool,
}
impl Drop for HostReviewedEngineReclamation {
    fn drop(&mut self) {
        if !self.transferred {
            self.slot.cancel_idle();
        }
    }
}
impl HostEngineReclamationPlan {
    pub fn action_id(&self) -> Id {
        self.id
    }
    /// Read-only worker review; no resource/transfer reservation until consent.
    pub fn review(
        mut self,
        root: &Path,
        control: &JobControl,
    ) -> Result<HostReviewedEngineReclamation> {
        let plan = take_running(&self.slot, |phase| match phase {
            Phase::ReclamationPlan(plan) => plan.take(),
            _ => None,
        })?;
        self.transferred = true;
        let mut running = Running {
            slot: Arc::clone(&self.slot),
            armed: true,
        };
        let control = control.linked_authority(&self.slot.authority);
        let reviewed = worker(|| plan.review(root, &control))?;
        let summary = reviewed.summary().clone();
        finish_running(
            &self.slot,
            Phase::ReclamationCandidate(Arc::new(ReclamationCandidate(Mutex::new(
                Data::Reviewed(Box::new(reviewed)),
            )))),
            &mut running,
        )?;
        Ok(HostReviewedEngineReclamation {
            slot: Arc::clone(&self.slot),
            summary,
            transferred: false,
        })
    }
}
impl HostReviewedEngineReclamation {
    pub fn summary(&self) -> &EngineReclamationSummary {
        &self.summary
    }
    /// Consumes the native review. Consent cannot bypass current SDK ownership.
    pub fn prepare(
        mut self,
        consent: ReclamationConsent,
        control: &JobControl,
    ) -> Result<HostPreparedEngineReclamation> {
        let candidate = take_running(&self.slot, |phase| match phase {
            Phase::ReclamationCandidate(candidate) => Some(Arc::clone(candidate)),
            _ => None,
        })?;
        self.transferred = true;
        let mut running = Running {
            slot: Arc::clone(&self.slot),
            armed: true,
        };
        let control = control.linked_authority(&self.slot.authority);
        let prepared = worker(|| {
            let data = std::mem::replace(
                &mut *candidate.0.lock().map_err(|_| Error::WorkerFailed)?,
                Data::Empty,
            );
            let result = match data {
                Data::Reviewed(review) => review.prepare(consent, &control),
                _ => Err(Error::Conflict),
            };
            drop(candidate);
            result
        })?;
        finish_running(
            &self.slot,
            Phase::ReclamationCandidate(Arc::new(ReclamationCandidate(Mutex::new(
                Data::Prepared {
                    value: Box::new(prepared),
                    permit: None,
                },
            )))),
            &mut running,
        )?;
        Ok(HostPreparedEngineReclamation {
            slot: Arc::clone(&self.slot),
            id: self.summary.action,
            transferred: false,
        })
    }
}
impl HostPreparedEngineReclamation {
    pub fn action_id(&self) -> Id {
        self.id
    }
    pub(in crate::host) fn publication(&self, control: &JobControl) -> ReclamationPublication {
        ReclamationPublication {
            slot: Arc::downgrade(&self.slot),
            control: control.linked_authority(&self.slot.authority),
        }
    }
    /// Requires a durable Host authorisation first. Errors may follow physical deletion.
    pub fn remove(mut self, control: &JobControl) -> Result<HostReclaimedEngineResource> {
        let candidate = take_running(&self.slot, |phase| match phase {
            Phase::ReclamationCandidate(candidate) => Some(Arc::clone(candidate)),
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
            Phase::ReclamationCandidate(Arc::new(ReclamationCandidate(Mutex::new(Data::Removed(
                Box::new(removed),
            ))))),
            &mut running,
        )?;
        Ok(HostReclaimedEngineResource {
            slot: Arc::clone(&self.slot),
            id: self.id,
        })
    }
}
impl HostReclaimedEngineResource {
    pub fn action_id(&self) -> Id {
        self.id
    }
    pub(in crate::host) fn publication(&self, control: &JobControl) -> ReclamationPublication {
        ReclamationPublication {
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
            if let Phase::ReclamationPublishing(candidate) =
                std::mem::replace(&mut *phase, Phase::Finished)
            {
                *phase = Phase::ReclamationCandidate(candidate);
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
    pub(in crate::host) fn engine_reclamation_plan(
        self: &Arc<Self>,
        store: &Store,
        access: &Access,
        version: Id,
        resume: Option<Id>,
    ) -> Result<HostEngineReclamationPlan> {
        let plan = store.engine_reclamation_plan(version, resume, access)?;
        let id = plan.id();
        Ok(HostEngineReclamationPlan {
            slot: self.lifecycle_slot(
                store,
                access,
                LifecycleInput::ReclamationPlan(Box::new(plan)),
            )?,
            id,
            transferred: false,
        })
    }
    pub(in crate::host) fn publish_reclamation(
        &self,
        store: &mut Store,
        access: &Access,
        publication: ReclamationPublication,
        complete: bool,
    ) -> Result<EngineReclamation> {
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
            let Phase::ReclamationCandidate(candidate) = &*phase else {
                return Err(Error::Cancelled);
            };
            let candidate = Arc::clone(candidate);
            state
                .entries
                .get_mut(&slot.authority.ticket)
                .ok_or(Error::Cancelled)?
                .running = Some(thread::current().id());
            *phase = Phase::ReclamationPublishing(Arc::clone(&candidate));
            publishing.armed = true;
            candidate
        };
        let result = {
            let mut data = candidate.0.lock().map_err(|_| Error::WorkerFailed)?;
            match (&mut *data, complete) {
                (Data::Prepared { value, permit }, false) => {
                    let approved =
                        store.authorise_engine_reclamation(value, &publication.control, access)?;
                    *permit = Some(Box::new(approved));
                    Ok(value.action().clone())
                }
                (Data::Removed(removed), true) => {
                    store.complete_engine_reclamation(removed, access)
                }
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
        let mut store = Store::open(root.path().join("library.db")).unwrap();
        let mut descriptor: crate::processing::Descriptor = serde_json::from_str(r#"{"id":"synthetic-discard-pin","version":"1","adapter_version":"1","family":"Recogniser","input":"OrderedStrokesV1","tasks":["Prose"],"internals":"StrokeOnlyVerified","offline":true,"content_egress":false,"personal_use_evidence":"app-owned phase ownership fixture","redistributable_resources":false,"resource_sha256":null,"runtime_contract":"stem-processing-v1"}"#).unwrap();
        descriptor.resource_sha256 = Some(resource_digest(b"model"));
        let control = || JobControl::new(Duration::from_secs(30)).unwrap();
        let models = root.path().join("models");
        let mut transfer = store
            .engine_import_plan(descriptor, 5, true, &Access::public())
            .unwrap()
            .begin(&models, &control())
            .unwrap();
        transfer.append(0, b"model", &control()).unwrap();
        let completed = transfer.finish(&control()).unwrap();
        let version = store
            .commit_engine_import(&completed, &Access::public())
            .unwrap();
        drop(completed);
        let id = version.id;
        let removal = store.engine_removal_plan(id, &Access::public()).unwrap();
        drop(
            store
                .commit_engine_removal(&removal, &mut Manager::new(), &control(), &Access::public())
                .unwrap(),
        );
        drop(store);
        let host = Arc::new(Host::open(root.path().join("library.db"), 8).unwrap());
        let prepared = host
            .engine_reclamation_plan(id, None)
            .unwrap()
            .review(&models, &control())
            .unwrap()
            .prepare(
                ReclamationConsent {
                    confirmed: true,
                    reviewed_unknown_binding: false,
                    reviewed_failed_retirement: false,
                    reviewed_symlinks: false,
                },
                &control(),
            )
            .unwrap();
        let action_id = prepared.action_id();
        let slot = Arc::clone(&prepared.slot);
        let writer = Arc::clone(&host);
        let publish: Box<dyn FnOnce() -> Result<EngineReclamation> + Send> = if complete {
            host.authorise_engine_reclamation(&prepared, &control())
                .unwrap();
            let removed = prepared.remove(&control()).unwrap();
            Box::new(move || {
                writer.complete_engine_reclamation(
                    &removed,
                    &JobControl::new(Duration::from_secs(30)).unwrap(),
                )
            })
        } else {
            Box::new(move || {
                writer.authorise_engine_reclamation(
                    &prepared,
                    &JobControl::new(Duration::from_secs(30)).unwrap(),
                )
            })
        };
        let candidate = match &*slot.phase.lock().unwrap() {
            Phase::ReclamationCandidate(candidate) => Arc::clone(candidate),
            _ => panic!("missing candidate"),
        };
        let data = candidate.0.lock().unwrap();
        let job = thread::spawn(publish);
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if matches!(*slot.phase.lock().unwrap(), Phase::ReclamationPublishing(_)) {
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
        let pinned = matches!(*slot.phase.lock().unwrap(), Phase::ReclamationPublishing(_));
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
                host.engine_reclamation(action_id).unwrap().state,
                ResourceActionState::Pending
            );
            assert!(!models.join(format!("{id}.part")).exists());
        } else {
            assert!(host
                .engine_reclamations(None, 8)
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
    fn capture_cannot_retire_an_authorising_reclamation_cell() {
        check_writer_pin(false);
    }
    #[test]
    fn capture_cannot_retire_a_completing_reclamation_cell() {
        check_writer_pin(true);
    }
}
