//! The phase pins detached SDK owners until the writer drops all temporary
//! Manager/candidate references and enables off-writer retirement.
use super::activation::LifecycleInput;
use super::*;
use crate::{
    processing::{DetachedRuntime, JobControl},
    store::EngineRemovalPlan,
};
use std::sync::TryLockError;
pub struct HostEngineRemovalPlan {
    slot: Arc<Slot>,
}
impl Drop for HostEngineRemovalPlan {
    fn drop(&mut self) {
        self.slot.cancel_idle();
    }
}
pub(super) struct RemovalCandidate(Mutex<RemovalData>);
struct RemovalData {
    plan: EngineRemovalPlan,
    retired: Option<DetachedRuntime>,
}
impl RemovalCandidate {
    pub(super) fn new(plan: EngineRemovalPlan) -> Self {
        Self(Mutex::new(RemovalData {
            plan,
            retired: None,
        }))
    }
}
pub(in crate::host) struct RemovalPublication {
    slot: Weak<Slot>,
    manager: Weak<Mutex<Manager>>,
    control: JobControl,
}
impl HostEngineRemovalPlan {
    pub(in crate::host) fn publication(
        &self,
        manager: &Arc<Mutex<Manager>>,
        control: &JobControl,
    ) -> RemovalPublication {
        RemovalPublication {
            slot: Arc::downgrade(&self.slot),
            manager: Arc::downgrade(manager),
            control: control.linked_authority(&self.slot.authority),
        }
    }
}
struct Publishing {
    slot: Arc<Slot>,
    armed: bool,
}
impl Publishing {
    fn finish(&mut self, success: bool) {
        let current = {
            let mut state = self.slot.lease.scope.state();
            let mut phase = self.slot.phase.lock().unwrap_or_else(|p| p.into_inner());
            if let Phase::RemovalPublishing(candidate) =
                std::mem::replace(&mut *phase, Phase::Finished)
            {
                *phase = if success {
                    Phase::RemovalPublished {
                        _candidate: candidate,
                    }
                } else {
                    Phase::RemovalPrepared(candidate)
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
    pub(in crate::host) fn engine_removal_plan(
        self: &Arc<Self>,
        store: &Store,
        access: &Access,
        id: Id,
    ) -> Result<HostEngineRemovalPlan> {
        let plan = store.engine_removal_plan(id, access)?;
        Ok(HostEngineRemovalPlan {
            slot: self.lifecycle_slot(
                store,
                access,
                LifecycleInput::RemovalPlan(Box::new(plan)),
            )?,
        })
    }
    pub(in crate::host) fn remove_engine(
        &self,
        store: &mut Store,
        access: &Access,
        publication: RemovalPublication,
    ) -> Result<()> {
        let slot = publication.slot.upgrade().ok_or(Error::Cancelled)?;
        self.check_optional_authority(&slot.authority)?;
        publication.control.check()?;
        let mut running = Publishing {
            slot: Arc::clone(&slot),
            armed: false,
        };
        let manager = publication.manager.upgrade().ok_or(Error::Cancelled)?;
        let mut guard = match manager.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return Err(Error::ResourceLimit),
            Err(TryLockError::Poisoned(_)) => return Err(Error::WorkerFailed),
        };
        let candidate = {
            let mut state = self.state();
            self.check_optional_authority(&slot.authority)?;
            if state.closed {
                return Err(Error::Cancelled);
            }
            let mut phase = slot.phase.lock().map_err(|_| Error::WorkerFailed)?;
            let Phase::RemovalPrepared(candidate) = &*phase else {
                return Err(Error::Conflict);
            };
            let candidate = Arc::clone(candidate);
            state
                .entries
                .get_mut(&slot.authority.ticket)
                .ok_or(Error::Cancelled)?
                .running = Some(thread::current().id());
            *phase = Phase::RemovalPublishing(Arc::clone(&candidate));
            running.armed = true;
            candidate
        };
        let result = {
            let mut data = candidate.0.lock().map_err(|_| Error::WorkerFailed)?;
            match store.commit_engine_removal(&data.plan, &mut guard, &publication.control, access)
            {
                Ok(retired) => {
                    data.retired = retired;
                    Ok(())
                }
                Err(error) => Err(error),
            }
        };
        drop(candidate);
        drop(guard);
        drop(manager);
        // Keep a known durable success even when cancellation races its reply.
        running.finish(result.is_ok());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Host;
    use std::time::{Duration, Instant};
    #[test]
    fn capture_cannot_retire_a_removal_cell_while_the_writer_is_publishing() {
        let root = tempfile::tempdir().unwrap();
        let descriptor = serde_json::from_str(r#"{"id":"synthetic-removal","version":"1","adapter_version":"1","family":"Recogniser","input":"OrderedStrokesV1","tasks":["Prose"],"internals":"StrokeOnlyVerified","offline":true,"content_egress":false,"personal_use_evidence":"app-owned removal race fixture","redistributable_resources":false,"resource_sha256":null,"runtime_contract":"stem-processing-v1"}"#).unwrap();
        let path = root.path().join("library.db");
        let mut store = Store::open(&path).unwrap();
        let version = store
            .stage_engine(descriptor, None, root.path(), 0, true, &Access::public())
            .unwrap();
        drop(store);
        let host = Arc::new(Host::open(path, 8).unwrap());
        let plan = Arc::new(host.engine_removal_plan(version.id).unwrap());
        let candidate = {
            let phase = plan.slot.phase.lock().unwrap();
            let Phase::RemovalPrepared(candidate) = &*phase else {
                panic!("unprepared fixture");
            };
            Arc::clone(candidate)
        };
        let data = candidate.0.lock().unwrap();
        let publisher = Arc::clone(&host);
        let publication = Arc::clone(&plan);
        let job = thread::spawn(move || {
            publisher.remove_prepared_engine(
                &publication,
                &JobControl::new(Duration::from_secs(30)).unwrap(),
            )
        });
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if matches!(
                *plan.slot.phase.lock().unwrap(),
                Phase::RemovalPublishing(_)
            ) {
                break;
            }
            if Instant::now() >= until {
                drop(data);
                let _ = job.join();
                panic!("publication did not enter");
            }
            thread::yield_now();
        }
        let pen = Id::new();
        host.set_capture_priority(pen, true).unwrap();
        host.set_capture_priority(pen, false).unwrap();
        let pinned = matches!(
            *plan.slot.phase.lock().unwrap(),
            Phase::RemovalPublishing(_)
        );
        drop(data);
        assert!(job.join().unwrap().is_err());
        assert!(pinned, "idle cancellation released a publishing cell");
        assert_eq!(
            host.engine_catalogue(None, 8).unwrap().versions[0]
                .version
                .state,
            crate::store::Installation::Staged
        );
        drop(plan);
        drop(candidate);
        host.close().unwrap();
    }
    #[test]
    fn busy_private_manager_returns_before_the_manager_guard_is_released() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path().join("library.db")).unwrap();
        let descriptor = serde_json::from_str(r#"{"id":"synthetic-busy","version":"1","adapter_version":"1","family":"Recogniser","input":"OrderedStrokesV1","tasks":["Prose"],"internals":"StrokeOnlyVerified","offline":true,"content_egress":false,"personal_use_evidence":"app-owned busy fixture","redistributable_resources":false,"resource_sha256":null,"runtime_contract":"stem-processing-v1"}"#).unwrap();
        let version = store
            .stage_engine(descriptor, None, root.path(), 0, true, &Access::public())
            .unwrap();
        drop(store);
        let host = Arc::new(Host::open(root.path().join("library.db"), 8).unwrap());
        let plan = Arc::new(host.engine_removal_plan(version.id).unwrap());
        let lease = host.runtime.lease().unwrap();
        let guard = lease.manager().lock().unwrap();
        let publisher = Arc::clone(&host);
        let publication = Arc::clone(&plan);
        let (tx, rx) = mpsc::channel();
        let job = thread::spawn(move || {
            tx.send(publisher.remove_prepared_engine(
                &publication,
                &JobControl::new(Duration::from_secs(30)).unwrap(),
            ))
            .unwrap()
        });
        let result = rx.recv_timeout(Duration::from_secs(10));
        drop(guard);
        drop(lease);
        job.join().unwrap();
        assert!(matches!(result, Ok(Err(Error::ResourceLimit))));
        host.remove_prepared_engine(&plan, &JobControl::new(Duration::from_secs(30)).unwrap())
            .unwrap();
        drop(plan);
        host.close().unwrap();
    }
}
