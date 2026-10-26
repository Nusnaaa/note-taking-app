//! Separate logical removal from caller-owned actual adapter retirement.
use super::*;

/// A removed entry that still pins its exact adapter/resources. It cannot enable
/// an engine or restore defaults. Drop on an optional retirement worker after
/// releasing any manager lock. Existing work may retain independent pins.
pub struct DetachedRuntime {
    entry: Entry,
}
impl DetachedRuntime {
    pub fn descriptor(&self) -> &Descriptor {
        &self.entry.descriptor
    }
}
impl Manager {
    /// Disable an exact current entry, invalidate old work and remove defaults,
    /// returning its resource owner without executing provider destruction.
    /// This is not an acknowledgement that every old worker/resource retired.
    pub fn detach(&mut self, id: &str) -> Result<DetachedRuntime> {
        let entry = self.entries.remove(id).ok_or(Error::EngineUnavailable)?;
        entry.retire();
        self.defaults.retain(|_, value| value != id);
        Ok(DetachedRuntime { entry })
    }
}

/// A probed runtime frozen for transfer. Preparation work from its source
/// manager is invalidated. Dispose of abandoned holders after unlocking.
pub struct ReadyRuntime {
    entry: Option<Entry>,
}
impl Drop for ReadyRuntime {
    fn drop(&mut self) {
        if let Some(entry) = &self.entry {
            entry.retire();
        }
    }
}
impl ReadyRuntime {
    pub fn descriptor(&self) -> Option<&Descriptor> {
        self.entry.as_ref().map(|entry| &entry.descriptor)
    }
}
impl Manager {
    pub fn take_ready(&mut self, id: &str) -> Result<ReadyRuntime> {
        let current = self.entries.get(id).ok_or(Error::EngineUnavailable)?;
        if current.readiness != Readiness::Ready || !current.validity.load(Ordering::Acquire) {
            return Err(Error::EngineUnavailable);
        }
        let entry = self.entries.remove(id).ok_or(Error::EngineUnavailable)?;
        entry.retire();
        self.defaults.retain(|_, value| value != id);
        Ok(ReadyRuntime { entry: Some(entry) })
    }
    /// Atomically publish an already probed candidate without callbacks. Failed
    /// validation keeps the candidate holder intact for off-lock disposal.
    pub fn install_ready(&mut self, ready: &mut ReadyRuntime) -> Result<Option<DetachedRuntime>> {
        let candidate = ready.entry.as_ref().ok_or(Error::Conflict)?;
        if self
            .entries
            .get(&candidate.descriptor.id)
            .is_some_and(|old| old.descriptor.family != candidate.descriptor.family)
        {
            return Err(Error::Conflict);
        }
        let candidate = ready.entry.take().ok_or(Error::Conflict)?;
        let id = candidate.descriptor.id.clone();
        self.defaults
            .retain(|task, value| value != &id || candidate.descriptor.tasks.contains(task));
        let previous = self.entries.insert(
            id,
            Entry::new(candidate.descriptor, candidate.adapter, Readiness::Ready),
        );
        Ok(previous.map(|entry| {
            entry.retire();
            DetachedRuntime { entry }
        }))
    }
}
