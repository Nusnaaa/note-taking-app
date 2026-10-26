//! Shared-manager metadata and durable defaults. No SDK callback/disposal occurs
//! here, and the writer never waits for the Manager mutex.
use super::*;
use crate::{
    processing::{Manager, Readiness, Task},
    store::{EngineCatalogueRow, Installation},
};
use std::sync::{TryLockError, Weak};
#[derive(Debug, Clone, Serialize)]
pub struct EngineRuntimeRow {
    #[serde(flatten)]
    pub version: EngineCatalogueRow,
    pub runtime_ready: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct EngineDefault {
    pub task: Task,
    pub engine: String,
    pub runtime_ready: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct EngineCatalogueSnapshot {
    pub versions: Vec<EngineRuntimeRow>,
    pub next: Option<Id>,
    pub defaults: Vec<EngineDefault>,
}
pub(super) fn with_manager<T>(
    store: &mut Store,
    access: &Access,
    manager: Weak<Mutex<Manager>>,
    run: impl FnOnce(&mut Store, &mut Manager) -> Result<T>,
) -> Result<T> {
    Store::require_app(&store.db, access)?;
    // The optional caller pins a registry lease through the reply. Drop these
    // temporary references/guards before the actor sends its metadata result.
    let manager = manager.upgrade().ok_or(Error::Cancelled)?;
    let mut guard = match manager.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::WouldBlock) => return Err(Error::ResourceLimit),
        Err(TryLockError::Poisoned(_)) => return Err(Error::WorkerFailed),
    };
    run(store, &mut guard)
}
pub(super) fn catalogue(
    store: &mut Store,
    access: &Access,
    after: Option<Id>,
    limit: usize,
    manager: Weak<Mutex<Manager>>,
) -> Result<EngineCatalogueSnapshot> {
    with_manager(store, access, manager, |store, manager| {
        let page = store.engine_catalogue(after, limit, access)?;
        let versions = page
            .versions
            .into_iter()
            .map(|version| {
                let runtime_ready = version.active
                    && version.state == Installation::Installed
                    && manager.readiness(&version.descriptor.id) == Readiness::Ready
                    && manager.descriptor(&version.descriptor.id) == Some(&version.descriptor);
                EngineRuntimeRow {
                    version,
                    runtime_ready,
                }
            })
            .collect();
        let mut defaults = Vec::new();
        for (task, engine) in store.engine_defaults(access)? {
            let active = store.active_engine(&engine, access)?;
            let runtime_ready = manager.default_for(task) == Some(engine.as_str())
                && active.as_ref().is_some_and(|v| {
                    manager.descriptor(&engine) == Some(&v.descriptor)
                        && manager.readiness(&engine) == Readiness::Ready
                        && v.descriptor.tasks.contains(&task)
                });
            defaults.push(EngineDefault {
                task,
                engine,
                runtime_ready,
            });
        }
        Ok(EngineCatalogueSnapshot {
            versions,
            next: page.next,
            defaults,
        })
    })
}
