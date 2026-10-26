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
