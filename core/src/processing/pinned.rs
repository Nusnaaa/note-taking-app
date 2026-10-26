//! Guard the actual provider owner, including callbacks and destructor unwinding.
use super::*;
use crate::provisioning::ResourcePin;
fn call<T>(pin: &Arc<ResourcePin>, run: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(value) => value,
        Err(error) => {
            pin.failed();
            std::panic::resume_unwind(error)
        }
    }
}
struct PinnedRecogniser {
    inner: Option<Box<dyn Recogniser>>,
    pin: Arc<ResourcePin>,
}
impl Recogniser for PinnedRecogniser {
    fn probe(&self) -> Result<()> {
        factory(&Some(Arc::clone(&self.pin)), || {
            self.inner.as_ref().expect("provider").probe()
        })
    }
    fn recognise(
        &self,
        ink: &InkSnapshot,
        task: Task,
        control: &JobControl,
    ) -> Result<RecognitionOutput> {
        factory(&Some(Arc::clone(&self.pin)), || {
            self.inner
                .as_ref()
                .expect("provider")
                .recognise(ink, task, control)
        })
    }
}
impl Drop for PinnedRecogniser {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            call(&self.pin, || drop(inner));
        }
    }
}
struct PinnedConverter {
    inner: Option<Box<dyn Converter>>,
    pin: Arc<ResourcePin>,
}
impl Converter for PinnedConverter {
    fn probe(&self) -> Result<()> {
        factory(&Some(Arc::clone(&self.pin)), || {
            self.inner.as_ref().expect("provider").probe()
        })
    }
    fn formats(&self) -> Vec<Format> {
        call(&self.pin, || {
            self.inner.as_ref().expect("provider").formats()
        })
    }
    fn convert(
        &self,
        content: &Content,
        target: Format,
        control: &JobControl,
    ) -> Result<Converted> {
        factory(&Some(Arc::clone(&self.pin)), || {
            self.inner
                .as_ref()
                .expect("provider")
                .convert(content, target, control)
        })
    }
}
impl Drop for PinnedConverter {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            call(&self.pin, || drop(inner));
        }
    }
}
pub(crate) fn recogniser(
    adapter: Box<dyn Recogniser>,
    pin: Option<Arc<ResourcePin>>,
) -> Box<dyn Recogniser> {
    match pin {
        Some(pin) => Box::new(PinnedRecogniser {
            inner: Some(adapter),
            pin,
        }),
        None => adapter,
    }
}
pub(crate) fn converter(
    adapter: Box<dyn Converter>,
    pin: Option<Arc<ResourcePin>>,
) -> Box<dyn Converter> {
    match pin {
        Some(pin) => Box::new(PinnedConverter {
            inner: Some(adapter),
            pin,
        }),
        None => adapter,
    }
}
pub(crate) fn factory<T>(
    pin: &Option<Arc<ResourcePin>>,
    run: impl FnOnce() -> Result<T>,
) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(Err(Error::Io(error))) if pin.is_some() => {
            let safe = std::io::Error::from(error.kind());
            // A provider-owned error payload must retire before its reservation.
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(error))).is_err() {
                pin.as_ref().expect("guarded error").failed();
                return Err(Error::WorkerFailed);
            }
            Err(Error::Io(safe))
        }
        Ok(result) => result,
        Err(_) => {
            if let Some(pin) = pin {
                pin.failed();
            }
            Err(Error::WorkerFailed)
        }
    }
}

// Protect captured preconstructed providers even if resource loading fails before
// the factory is invoked. Drop the capture while its reservation still exists.
pub(crate) struct FactoryOwner<F> {
    factory: Option<F>,
    pin: Option<Arc<ResourcePin>>,
}
impl<F> FactoryOwner<F> {
    pub(crate) fn new(factory: F, pin: &Option<Arc<ResourcePin>>) -> Self {
        Self {
            factory: Some(factory),
            pin: pin.clone(),
        }
    }
    pub(crate) fn take(&mut self) -> F {
        self.factory.take().expect("single-use factory")
    }
}
impl<F> Drop for FactoryOwner<F> {
    fn drop(&mut self) {
        if let Some(factory) = self.factory.take() {
            if let Some(pin) = &self.pin {
                call(pin, || drop(factory));
            } else {
                drop(factory);
            }
        }
    }
}
