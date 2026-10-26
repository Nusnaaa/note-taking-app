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
        self.detach_optional(id).ok_or(Error::EngineUnavailable)
    }
    pub(crate) fn detach_optional(&mut self, id: &str) -> Option<DetachedRuntime> {
        self.defaults.retain(|_, value| value != id);
        self.entries.remove(id).map(|entry| {
            entry.retire();
            DetachedRuntime { entry }
        })
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
        Ok(self.ready_publication(ready)?.commit())
    }
    pub(crate) fn ready_publication<'a>(
        &'a mut self,
        ready: &'a mut ReadyRuntime,
    ) -> Result<ReadyPublication<'a>> {
        let candidate = ready.entry.as_ref().ok_or(Error::Conflict)?;
        if self
            .entries
            .get(&candidate.descriptor.id)
            .is_some_and(|old| old.descriptor.family != candidate.descriptor.family)
        {
            return Err(Error::Conflict);
        }
        Ok(ReadyPublication {
            manager: self,
            ready,
            defaults: Vec::new(),
        })
    }
}
/// Holds exclusive borrows through a callback-free durable metadata commit.
/// Dropping a failed publication releases references without disposing providers.
pub(crate) struct ReadyPublication<'a> {
    manager: &'a mut Manager,
    ready: &'a mut ReadyRuntime,
    defaults: Vec<Task>,
}
impl ReadyPublication<'_> {
    pub(crate) fn with_defaults(mut self, defaults: Vec<Task>) -> Result<Self> {
        let candidate = self.ready.entry.as_ref().ok_or(Error::Conflict)?;
        if !defaults.is_empty()
            && (candidate.descriptor.family != Family::Recogniser
                || defaults
                    .iter()
                    .any(|task| !candidate.descriptor.tasks.contains(task)))
        {
            return Err(Error::EngineUnavailable);
        }
        self.defaults = defaults;
        Ok(self)
    }
    pub(crate) fn commit(self) -> Option<DetachedRuntime> {
        // Construction validated this entry and exclusive borrows prevent removal.
        let candidate = self
            .ready
            .entry
            .take()
            .expect("validated ready publication");
        let id = candidate.descriptor.id.clone();
        self.manager
            .defaults
            .retain(|task, value| value != &id || candidate.descriptor.tasks.contains(task));
        for task in self.defaults {
            self.manager.defaults.insert(task, id.clone());
        }
        self.manager
            .entries
            .insert(
                id,
                Entry::new(candidate.descriptor, candidate.adapter, Readiness::Ready),
            )
            .map(|entry| {
                entry.retire();
                DetachedRuntime { entry }
            })
    }
}
