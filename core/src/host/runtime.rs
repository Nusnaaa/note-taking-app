//! One host-owned shared engine manager. Private controller leases never escape
//! an operation; close retires SDK owners on a dedicated non-writer thread.
use crate::{processing::Manager, Error, Result};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle, ThreadId};

struct State {
    closed: bool,
    users: usize,
    manager: Option<Arc<Mutex<Manager>>>,
}
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}
pub(super) struct RuntimeRegistry {
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<Result<()>>>>,
    worker_id: ThreadId,
}
pub(super) struct RuntimeLease {
    shared: Arc<Shared>,
    manager: Option<Arc<Mutex<Manager>>>,
}
impl RuntimeRegistry {
    pub fn new() -> Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                closed: false,
                users: 0,
                manager: Some(Arc::new(Mutex::new(Manager::new()))),
            }),
            changed: Condvar::new(),
        });
        let retire = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("stem-runtime-retirement".into())
            .spawn(move || {
                let manager = {
                    let mut state = retire.state.lock().unwrap_or_else(|p| p.into_inner());
                    while !state.closed || state.users != 0 {
                        state = retire
                            .changed
                            .wait(state)
                            .unwrap_or_else(|p| p.into_inner());
                    }
                    state.manager.take()
                };
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(manager)))
                    .map_err(|_| Error::WorkerFailed)
            })?;
        Ok(Self {
            shared,
            worker_id: worker.thread().id(),
            worker: Mutex::new(Some(worker)),
        })
    }
    pub fn lease(&self) -> Result<RuntimeLease> {
        let mut state = self.shared.state.lock().map_err(|_| Error::WorkerFailed)?;
        if state.closed {
            return Err(Error::Cancelled);
        }
        let manager = Arc::clone(state.manager.as_ref().ok_or(Error::Cancelled)?);
        state.users = state.users.checked_add(1).ok_or(Error::ResourceLimit)?;
        Ok(RuntimeLease {
            shared: Arc::clone(&self.shared),
            manager: Some(manager),
        })
    }
    pub fn close(&self) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .closed = true;
        self.shared.changed.notify_all();
    }
    pub fn is_current_worker(&self) -> bool {
        self.worker_id == thread::current().id()
    }
    pub fn wait(&self) -> Result<()> {
        if self.is_current_worker() {
            return Err(Error::WorkerFailed);
        }
        let mut worker = self.worker.lock().map_err(|_| Error::WorkerFailed)?;
        if let Some(worker) = worker.take() {
            worker.join().map_err(|_| Error::WorkerFailed)??;
        }
        Ok(())
    }
}
impl Drop for RuntimeRegistry {
    fn drop(&mut self) {
        self.close();
        let _ = self.wait();
    }
}
impl RuntimeLease {
    pub fn manager(&self) -> &Arc<Mutex<Manager>> {
        self.manager.as_ref().expect("live private runtime lease")
    }
}
impl Drop for RuntimeLease {
    fn drop(&mut self) {
        // The registry pins the Manager until the counter reaches zero. Release
        // the temporary Arc first, so its final SDK destructor runs on retirement.
        drop(self.manager.take());
        let mut state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        state.users -= 1;
        self.shared.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::{JobControl, Recogniser, RecognitionOutput, Task};
    use crate::store::InkSnapshot;
    use std::sync::{mpsc, Weak};
    use std::time::Duration;
    struct Model {
        shared: Weak<Shared>,
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl Recogniser for Model {
        fn probe(&self) -> Result<()> {
            Ok(())
        }
        fn recognise(&self, _: &InkSnapshot, _: Task, _: &JobControl) -> Result<RecognitionOutput> {
            Err(Error::EngineUnavailable)
        }
    }
    impl Drop for Model {
        fn drop(&mut self) {
            assert_eq!(thread::current().name(), Some("stem-runtime-retirement"));
            assert!(self.shared.upgrade().unwrap().state.try_lock().is_ok());
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }
    }
    #[test]
    fn close_rejects_new_leases_and_waits_for_actual_last_lease_then_off_lock_sdk_drop() {
        let runtime = RuntimeRegistry::new().unwrap();
        let lease = runtime.lease().unwrap();
        let (entered, dropping) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let descriptor = crate::processing::Descriptor {
            id: "private-test".into(),
            version: "1".into(),
            adapter_version: "1".into(),
            family: crate::processing::Family::Recogniser,
            input: crate::processing::InputContract::OrderedStrokesV1,
            tasks: vec![Task::Prose],
            internals: crate::processing::Internals::StrokeOnlyVerified,
            offline: true,
            content_egress: false,
            personal_use_evidence: Some("app-owned lifecycle fixture".into()),
            redistributable_resources: false,
            resource_sha256: None,
            runtime_contract: "stem-processing-v1".into(),
        };
        lease
            .manager()
            .lock()
            .unwrap()
            .register_recogniser(
                descriptor,
                Box::new(Model {
                    shared: Arc::downgrade(&runtime.shared),
                    entered,
                    release: Mutex::new(gate),
                }),
            )
            .unwrap();
        runtime.close();
        assert!(matches!(runtime.lease(), Err(Error::Cancelled)));
        assert!(matches!(
            dropping.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        drop(lease);
        dropping.recv_timeout(Duration::from_secs(10)).unwrap();
        release.send(()).unwrap();
        runtime.wait().unwrap();
        runtime.close();
        runtime.wait().unwrap();
    }
}
