//! Off-writer recovered-copy work. No live-library SQL is executed by the reader,
//! decoder, private stage or fresh publication verifier.
use super::{Host, Message};
use crate::{
    backup::{RestorePublication, RestoreSummary, StagedBackupRestore},
    crypto::Credential,
    processing::{JobControl, WorkAuthority},
    security::Authenticator,
    store::Store,
    Error, Id, Result,
};
use serde::Serialize;
use std::{
    io::{self, Read},
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
    sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, Weak},
    thread::{self, ThreadId},
};
#[derive(Clone, Copy, Serialize)]
pub struct RestoreStatus {
    pub reserved: bool,
    pub running: bool,
    pub closed: bool,
}
pub struct RestoreReview {
    slot: Arc<Slot>,
    summary: RestoreSummary,
}
impl RestoreReview {
    pub fn summary(&self) -> &RestoreSummary {
        &self.summary
    }
    pub fn is_current(&self) -> bool {
        let s = self.slot.scope.state();
        !s.closed
            && s.ticket == Some(self.slot.ticket)
            && s.worker.is_none()
            && self.slot.authority.check().is_ok()
            && self.slot.stage.lock().is_ok_and(|stage| stage.is_some())
    }
}
struct State {
    closed: bool,
    ticket: Option<Id>,
    owner_live: bool,
    worker: Option<ThreadId>,
    slot: Weak<Slot>,
}
pub(super) struct RestoreScope {
    state: Mutex<State>,
    changed: Condvar,
    metadata_limit: usize,
}
impl RestoreScope {
    pub fn new(metadata_limit: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                closed: false,
                ticket: None,
                owner_live: false,
                worker: None,
                slot: Weak::new(),
            }),
            changed: Condvar::new(),
            metadata_limit,
        })
    }
    fn state(&self) -> MutexGuard<'_, State> {
        match self.state.lock() {
            Ok(s) => s,
            Err(p) => {
                let mut s = p.into_inner();
                s.closed = true;
                s
            }
        }
    }
    fn retire(&self, s: &mut State) {
        if !s.owner_live && s.worker.is_none() {
            s.ticket = None;
            s.slot = Weak::new();
        }
        self.changed.notify_all();
    }
    fn owner_done(&self, ticket: Id) {
        let mut s = self.state();
        if s.ticket == Some(ticket) {
            s.owner_live = false;
            self.retire(&mut s);
        }
    }
    fn worker_done(&self, ticket: Id) {
        {
            let mut s = self.state();
            if s.ticket == Some(ticket) {
                s.worker = None;
                self.retire(&mut s);
            }
        }
        self.clear_idle();
    }
    fn clear_idle(&self) {
        // Never release the final Slot/Permit reference under its state mutex.
        let slot = {
            let s = self.state();
            if s.worker.is_some() {
                return;
            }
            s.slot.upgrade()
        };
        let Some(slot) = slot else {
            return;
        };
        let stage = {
            let s = self.state();
            if s.worker.is_some() || s.ticket != Some(slot.ticket) {
                return;
            }
            if !s.closed && slot.authority.check().is_ok() {
                return;
            }
            slot.stage.lock().unwrap_or_else(|p| p.into_inner()).take()
        };
        drop(stage);
    }
    fn reserve(self: &Arc<Self>) -> Result<(Permit, Active)> {
        self.clear_idle();
        let mut s = self.state();
        if s.closed {
            return Err(Error::WorkerFailed);
        }
        if s.ticket.is_some() {
            return Err(Error::ResourceLimit);
        }
        let ticket = Id::new();
        s.ticket = Some(ticket);
        s.owner_live = true;
        s.worker = Some(thread::current().id());
        Ok((
            Permit {
                scope: Arc::clone(self),
                ticket,
            },
            Active {
                scope: Arc::clone(self),
                ticket,
            },
        ))
    }
    fn bind(&self, slot: &Arc<Slot>) -> Result<()> {
        let mut s = self.state();
        if s.closed || s.ticket != Some(slot.ticket) {
            return Err(Error::Cancelled);
        }
        slot.authority.check()?;
        s.slot = Arc::downgrade(slot);
        Ok(())
    }
    fn start(self: &Arc<Self>, slot: &Arc<Slot>) -> Result<(StageLease, Active)> {
        let mut s = self.state();
        if s.closed || s.ticket != Some(slot.ticket) || s.worker.is_some() {
            return Err(Error::Cancelled);
        }
        slot.authority.check()?;
        let stage = slot
            .stage
            .lock()
            .map_err(|_| Error::WorkerFailed)?
            .take()
            .ok_or(Error::Cancelled)?;
        s.worker = Some(thread::current().id());
        Ok((
            stage,
            Active {
                scope: Arc::clone(self),
                ticket: slot.ticket,
            },
        ))
    }
    pub fn close(&self) {
        self.state().closed = true;
        self.clear_idle();
        self.changed.notify_all();
    }
    pub fn wait(&self) {
        let mut s = self.state();
        while s.ticket.is_some() {
            s = self.changed.wait(s).unwrap_or_else(|p| p.into_inner());
        }
    }
    pub fn is_current_worker(&self) -> bool {
        self.state().worker == Some(thread::current().id())
    }
    fn status(&self) -> RestoreStatus {
        let s = self.state();
        RestoreStatus {
            reserved: s.ticket.is_some(),
            running: s.worker.is_some(),
            closed: s.closed,
        }
    }
}
struct Permit {
    scope: Arc<RestoreScope>,
    ticket: Id,
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.scope.owner_done(self.ticket);
    }
}
struct Active {
    scope: Arc<RestoreScope>,
    ticket: Id,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.scope.worker_done(self.ticket);
    }
}
// Stage descriptors/scratch retire before the permit can free admission.
struct StageLease {
    stage: StagedBackupRestore,
    _permit: Permit,
}
struct Slot {
    stage: Mutex<Option<StageLease>>,
    scope: Arc<RestoreScope>,
    ticket: Id,
    authority: WorkAuthority,
}
struct OwnedReader<R>(Option<R>);
impl<R> OwnedReader<R> {
    fn retire(&mut self) -> Result<()> {
        catch_unwind(AssertUnwindSafe(|| drop(self.0.take()))).map_err(|_| Error::WorkerFailed)
    }
}
impl<R> Drop for OwnedReader<R> {
    fn drop(&mut self) {
        let _ = self.retire();
    }
}
struct Attempt<R> {
    source: OwnedReader<R>,
    stage: Option<StagedBackupRestore>,
    permit: Option<Permit>,
    _active: Active,
}
struct Checked<'a, R> {
    source: &'a mut R,
    control: &'a JobControl,
}
impl<R: Read> Read for Checked<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.control
            .check()
            .map_err(|_| io::Error::other("Restore cancelled"))?;
        self.source.read(output)
    }
}
impl Host {
    pub fn restore_status(&self) -> RestoreStatus {
        self.restores.status()
    }
    /// Trusted optional-worker entry. Admitted input is owned through actual reader
    /// retirement, including read/drop panic. Rejected/unadmitted resources remain
    /// the caller's responsibility, as with the native backup transport.
    pub fn prepare_restore<R: Read>(
        &self,
        source: R,
        credential: Credential<'_>,
        max_plaintext_bytes: u64,
        private_directory: &Path,
        control: &JobControl,
    ) -> Result<RestoreReview> {
        let source = OwnedReader(Some(source));
        if self.writer_thread == thread::current().id() {
            return Err(Error::WorkerFailed);
        }
        let (permit, active) = self.restores.reserve()?;
        let ticket = permit.ticket;
        let mut attempt = Attempt {
            source,
            stage: None,
            permit: Some(permit),
            _active: active,
        };
        control.check()?;
        if max_plaintext_bytes == 0 {
            return Err(Error::ResourceLimit);
        }
        let (tx, rx) = mpsc::channel();
        self.mailbox
            .admit(Message::RestoreAuthority(ticket, tx), false, 128)?;
        let authority = rx.recv().map_err(|_| Error::WorkerFailed)??;
        let control = control.linked_authority(&authority);
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<()> {
            attempt.stage = Some(Store::restore_any_backup(
                Checked {
                    source: attempt.source.0.as_mut().ok_or(Error::Cancelled)?,
                    control: &control,
                },
                credential,
                max_plaintext_bytes,
                private_directory,
                &control,
            )?);
            Ok(())
        }))
        .unwrap_or(Err(Error::WorkerFailed));
        let retired = attempt.source.retire();
        result?;
        retired?;
        control.check()?;
        authority.check()?;
        let summary = attempt.stage.as_ref().ok_or(Error::Integrity)?.summary()?;
        if summary
            .protected_scopes
            .checked_add(1)
            .and_then(|n| n.checked_mul(64))
            .is_none_or(|n| n > self.restores.metadata_limit as u64)
        {
            return Err(Error::ResourceLimit);
        }
        let slot = Arc::new(Slot {
            stage: Mutex::new(Some(StageLease {
                stage: attempt.stage.take().ok_or(Error::Integrity)?,
                _permit: attempt.permit.take().ok_or(Error::Integrity)?,
            })),
            scope: Arc::clone(&self.restores),
            ticket,
            authority,
        });
        self.restores.bind(&slot)?;
        drop(attempt);
        let review = RestoreReview { slot, summary };
        if !review.is_current() {
            return Err(Error::Cancelled);
        }
        Ok(review)
    }
    /// Consumes the private stage; current live authoring does not become recovered
    /// content. Fresh proof and explicit acceptance precede atomic no-overwrite copy.
    pub fn publish_restore(
        &self,
        review: RestoreReview,
        destination: &Path,
        auth: &mut dyn Authenticator,
        confirmed: bool,
        control: &JobControl,
    ) -> Result<RestorePublication> {
        if self.writer_thread == thread::current().id() {
            return Err(Error::WorkerFailed);
        }
        if !Arc::ptr_eq(&review.slot.scope, &self.restores) {
            return Err(Error::Denied);
        }
        let (lease, active) = self.restores.start(&review.slot)?;
        let control = control.linked_authority(&review.slot.authority);
        let result = catch_unwind(AssertUnwindSafe(|| {
            review.slot.authority.check()?;
            lease.stage.publish(destination, auth, confirmed, &control)
        }))
        .unwrap_or(Err(Error::WorkerFailed));
        drop(lease);
        let current = control.check().and_then(|_| review.slot.authority.check());
        drop(active);
        match result {
            Ok(RestorePublication::Published) if current.is_err() => {
                Ok(RestorePublication::Uncertain)
            }
            value => value,
        }
    }
}
