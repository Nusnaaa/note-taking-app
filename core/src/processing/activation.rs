//! Owned activation callbacks. A caller can release its manager lock before
//! probing and before dropping the returned resource-pinning completion.
use super::*;

pub struct ActivationWork {
    descriptor: Descriptor,
    adapter: Adapter,
    epoch: Arc<AtomicBool>,
    transferred: bool,
}
impl Drop for ActivationWork {
    fn drop(&mut self) {
        if !self.transferred {
            self.epoch.store(false, Ordering::Release);
        }
    }
}
pub struct CompletedActivation {
    id: String,
    epoch: Arc<AtomicBool>,
    outcome: Option<Result<()>>,
    applied: bool,
    control: Option<JobControl>,
    // Keep callbacks/resources alive until the caller retires this holder,
    // after releasing the manager lock, including a stale failed commit.
    _adapter: Adapter,
}
impl Drop for CompletedActivation {
    fn drop(&mut self) {
        self.epoch.store(false, Ordering::Release);
    }
}
impl CompletedActivation {
    pub(super) fn take_legacy_error(&mut self) -> Option<Error> {
        if self.applied {
            self.outcome.take().and_then(Result::err)
        } else {
            None
        }
    }
}
// Preserve public error classes without moving provider-owned error payloads
// (including custom io::Error destructors) into a manager-locked caller.
fn classify(error: &Error) -> Error {
    match error {
        Error::Invalid(field) => Error::Invalid(field),
        Error::Unavailable => Error::Unavailable,
        Error::Denied => Error::Denied,
        Error::Conflict => Error::Conflict,
        Error::ReviewRequired => Error::ReviewRequired,
        Error::Io(error) => Error::Io(std::io::Error::from(error.kind())),
        Error::Storage(_) | Error::Json(_) | Error::WorkerFailed => Error::WorkerFailed,
        Error::Unsupported => Error::Unsupported,
        Error::Cancelled => Error::Cancelled,
        Error::Timeout => Error::Timeout,
        Error::EngineUnavailable => Error::EngineUnavailable,
        Error::ResourceLimit => Error::ResourceLimit,
        Error::Integrity => Error::Integrity,
        Error::Interrupted => Error::Interrupted,
    }
}
impl ActivationWork {
    pub fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }
    pub fn run(self, control: &JobControl) -> CompletedActivation {
        let control = control.linked(Arc::clone(&self.epoch));
        self.run_inner(Some(control))
    }
    pub(super) fn run_unbounded(self) -> CompletedActivation {
        self.run_inner(None)
    }
    fn run_inner(mut self, control: Option<JobControl>) -> CompletedActivation {
        let check = || {
            if !self.epoch.load(Ordering::Acquire) {
                return Err(Error::Cancelled);
            }
            control.as_ref().map_or(Ok(()), JobControl::check)
        };
        let outcome = check()
            .and_then(|()| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &self.adapter {
                    Adapter::Recogniser(adapter) => adapter.probe(),
                    Adapter::Converter(adapter) => adapter.probe(),
                }))
                .map_err(|_| Error::WorkerFailed)
                .and_then(|result| result)
            })
            .and_then(|()| check());
        self.transferred = true;
        CompletedActivation {
            id: self.descriptor.id.clone(),
            epoch: Arc::clone(&self.epoch),
            outcome: Some(outcome),
            applied: false,
            control,
            _adapter: self.adapter.clone(),
        }
    }
}
impl Manager {
    /// Consent/integrity checks retire the old readiness epoch, then return a
    /// pinned probe without running provider code under the manager borrow.
    /// Pending/cancelled activation remains non-Ready (Failed), as during the
    /// previous synchronous activation's verification/probe interval.
    pub fn prepare_activation(
        &mut self,
        id: &str,
        resource: Option<&[u8]>,
        consented: bool,
    ) -> Result<ActivationWork> {
        if !consented {
            return Err(Error::Denied);
        }
        let entry = self.entries.get_mut(id).ok_or(Error::EngineUnavailable)?;
        entry.retire();
        entry.validity = Arc::new(AtomicBool::new(false));
        entry.readiness = Readiness::Failed;
        if let Some(expected) = &entry.descriptor.resource_sha256 {
            if resource.is_none_or(|r| digest(r) != *expected) {
                return Err(Error::Integrity);
            }
        } else if resource.is_some() {
            return Err(Error::Invalid("unexpected model data"));
        }
        entry.activation_epoch = Arc::new(AtomicBool::new(true));
        Ok(ActivationWork {
            descriptor: entry.descriptor.clone(),
            adapter: entry.adapter.clone(),
            epoch: Arc::clone(&entry.activation_epoch),
            transferred: false,
        })
    }
    /// No provider callback or final adapter destruction occurs here. Keep the
    /// completion alive until after releasing any manager lock. A successful
    /// commit consumes its outcome; stale/foreign tokens cannot replace state.
    pub fn complete_activation(&mut self, completed: &mut CompletedActivation) -> Result<()> {
        let entry = self
            .entries
            .get_mut(&completed.id)
            .ok_or(Error::EngineUnavailable)?;
        if !Arc::ptr_eq(&entry.activation_epoch, &completed.epoch)
            || !completed.epoch.load(Ordering::Acquire)
        {
            return Err(Error::Cancelled);
        }
        if completed.applied {
            return Err(Error::Conflict);
        }
        let outcome = completed
            .outcome
            .as_ref()
            .ok_or(Error::Conflict)?
            .as_ref()
            .copied()
            .map_err(classify);
        completed.applied = true;
        let outcome = completed
            .control
            .as_ref()
            .map_or(Ok(()), JobControl::check)
            .and(outcome);
        entry.validity.store(outcome.is_ok(), Ordering::Release);
        entry.readiness = if outcome.is_ok() {
            Readiness::Ready
        } else {
            Readiness::Failed
        };
        completed.epoch.store(false, Ordering::Release);
        outcome
    }
}
