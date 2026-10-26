//! Authorised, bounded content-to-content conversion. Derivatives never mutate
//! notebook content; the source revision remains explicit even after later edits.
use super::*;
use crate::{
    content::Content,
    processing::{Converted, Descriptor, Format, JobControl},
};

pub struct ConversionSource {
    slot: Arc<Slot>,
    block: Id,
    region: Option<Id>,
    revision: u64,
    transferred: bool,
}
pub struct HostConversionWork {
    slot: Arc<Slot>,
    block: Id,
    region: Option<Id>,
    revision: u64,
}
/// Immutable derivative with source identity and adapter fidelity diagnostics.
/// Access is checked again when obtaining the result, including after expiry.
pub struct HostCompletedConversion {
    authority: WorkAuthority,
    control: JobControl,
    block: Id,
    region: Option<Id>,
    revision: u64,
    descriptor: Descriptor,
    target: Format,
    output: Converted,
}
impl HostCompletedConversion {
    pub fn source(&self) -> (Id, Option<Id>, u64) {
        (self.block, self.region, self.revision)
    }
    pub fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }
    pub fn target(&self) -> Format {
        self.target.clone()
    }
    pub fn output(&self) -> Result<&Converted> {
        self.authority.check()?;
        self.control.check()?;
        Ok(&self.output)
    }
}
impl Drop for ConversionSource {
    fn drop(&mut self) {
        if !self.transferred {
            self.slot.cancel_idle();
        }
    }
}
impl Drop for HostConversionWork {
    fn drop(&mut self) {
        self.slot.cancel_idle();
    }
}
impl RecognitionScope {
    pub(in crate::host) fn conversion_source(
        self: &Arc<Self>,
        store: &Store,
        access: &Access,
        block: Id,
        region: Option<Id>,
    ) -> Result<ConversionSource> {
        {
            let state = self.state();
            if state.closed {
                return Err(Error::WorkerFailed);
            }
            if !state.capture.is_empty() || state.entries.len() >= self.capacity {
                return Err(Error::ResourceLimit);
            }
        }
        let stored = store.block(block, access)?;
        let content = if let Some(region) = region {
            stored
                .regions
                .iter()
                .find(|r| r.id == region)
                .ok_or(Error::Invalid("conversion region"))?
                .content
                .clone()
                .ok_or(Error::Unsupported)?
        } else {
            stored.content
        };
        if !matches!(
            content,
            Content::Text(_) | Content::Code { .. } | Content::Numeric(_) | Content::Maths(_)
        ) {
            return Err(Error::Unsupported);
        }
        let bytes = charge(&content)?;
        self.ensure_retirement_lane()?;
        let mut state = self.state();
        if state.closed {
            return Err(Error::WorkerFailed);
        }
        if !state.capture.is_empty()
            || state.entries.len() >= self.capacity
            || bytes
                > MAX_SOURCE_BYTES.saturating_sub(state.entries.values().map(|e| e.bytes).sum())
        {
            return Err(Error::ResourceLimit);
        }
        let ticket = Id::new();
        let slot = Arc::new(Slot {
            authority: WorkAuthority {
                owner: self.owner,
                ticket,
                valid: Arc::clone(&state.valid),
                until: access.expires_at(),
            },
            phase: Mutex::new(Phase::ConversionSource(Some(content))),
            lease: SlotLease {
                scope: Arc::clone(self),
                ticket,
            },
        });
        state.entries.insert(
            ticket,
            Entry {
                slot: Arc::downgrade(&slot),
                bytes,
                running: None,
                retiring: None,
                retire_pending: false,
            },
        );
        Ok(ConversionSource {
            slot,
            block,
            region,
            revision: stored.revision,
            transferred: false,
        })
    }
}
impl ConversionSource {
    pub fn ticket(&self) -> Id {
        self.slot.authority.ticket
    }
    pub fn source(&self) -> (Id, Option<Id>, u64) {
        (self.block, self.region, self.revision)
    }
    pub fn prepare(
        mut self,
        manager: &Manager,
        engine: &str,
        target: Format,
    ) -> Result<HostConversionWork> {
        self.slot.authority.check()?;
        {
            let mut phase = self.slot.phase.lock().map_err(|_| Error::WorkerFailed)?;
            let Phase::ConversionSource(content) = &mut *phase else {
                return Err(Error::Cancelled);
            };
            let work = manager
                .prepare_conversion(engine, content.take().ok_or(Error::Cancelled)?, target)?
                .bind_authority(self.slot.authority.clone());
            *phase = Phase::ConversionPrepared(Some(Box::new(work)));
        }
        self.transferred = true;
        Ok(HostConversionWork {
            slot: Arc::clone(&self.slot),
            block: self.block,
            region: self.region,
            revision: self.revision,
        })
    }
}
impl HostConversionWork {
    pub fn ticket(&self) -> Id {
        self.slot.authority.ticket
    }
    pub fn run(self, control: &JobControl) -> Result<HostCompletedConversion> {
        let work = {
            let mut state = self.slot.lease.scope.state();
            self.slot.authority.check()?;
            if state.closed {
                return Err(Error::Cancelled);
            }
            let mut phase = self.slot.phase.lock().map_err(|_| Error::WorkerFailed)?;
            let Phase::ConversionPrepared(work) = &mut *phase else {
                return Err(Error::Cancelled);
            };
            let work = work.take().ok_or(Error::Cancelled)?;
            state
                .entries
                .get_mut(&self.slot.authority.ticket)
                .ok_or(Error::Cancelled)?
                .running = Some(thread::current().id());
            *phase = Phase::Running;
            work
        };
        let _running = RunningGuard {
            slot: Arc::clone(&self.slot),
        };
        let descriptor = work.descriptor().clone();
        let target = work.target();
        let delivery_control = work.delivery_control(control);
        let output = work.run(control)?;
        charge(&(&descriptor, &output))?;
        self.slot.authority.check()?;
        Ok(HostCompletedConversion {
            authority: self.slot.authority.clone(),
            control: delivery_control,
            block: self.block,
            region: self.region,
            revision: self.revision,
            descriptor,
            target,
            output,
        })
    }
}
