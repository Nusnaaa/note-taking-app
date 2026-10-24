//! Native backup job orchestration. The writer only reviews/pulls/validates;
//! encryption, sink callbacks and sink retirement stay on the caller's worker.
use super::{Host, Message, RecognitionScope};
use crate::{
    backup::{BackupOptions, StreamBackupCursor, StreamBackupPlan},
    crypto::stream::EncryptStream,
    processing::{JobControl, WorkAuthority},
    security::{Access, Authenticator},
    store::Store,
    Error, Id, Result,
};
use serde::Serialize;
use std::{
    io::Write,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, Weak},
    thread::{self, ThreadId},
};
use zeroize::Zeroizing;
#[derive(Clone, Serialize)]
pub struct BackupSummary {
    pub includes_original_ink: bool,
    pub source_assets: usize,
    pub omissions: Vec<String>,
}
#[derive(Clone, Copy, Serialize)]
pub struct BackupStatus {
    pub reserved: bool,
    pub running: bool,
    pub closed: bool,
}
pub struct BackupReceipt {
    recovery_key: Zeroizing<String>,
}
impl BackupReceipt {
    pub fn recovery_key(&self) -> &str {
        &self.recovery_key
    }
}
pub struct BackupReview {
    slot: Arc<Slot>,
    summary: BackupSummary,
}
impl BackupReview {
    pub fn summary(&self) -> &BackupSummary {
        &self.summary
    }
    /// Admission hint only: finish still rechecks authority and generation.
    /// Lets trusted transports retire stale opaque handles without retaining
    /// cancelled metadata or evicting a newer review.
    pub fn is_current(&self) -> bool {
        let s = self.slot.lease.scope.state();
        !s.closed
            && s.ticket == Some(self.slot.lease.ticket)
            && s.worker.is_none()
            && self.slot.authority.check().is_ok()
            && self.slot.plan.lock().is_ok_and(|p| p.is_some())
    }
}
struct State {
    closed: bool,
    ticket: Option<Id>,
    owner_live: bool,
    worker: Option<ThreadId>,
    actor: bool,
    prepared: Weak<Slot>,
}
pub(super) struct BackupScope {
    state: Mutex<State>,
    changed: Condvar,
    metadata_limit: usize,
}
impl BackupScope {
    pub fn new(metadata_limit: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                closed: false,
                ticket: None,
                owner_live: false,
                worker: None,
                actor: false,
                prepared: Weak::new(),
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
    fn retire_if_done(&self, s: &mut State) {
        if !s.owner_live && s.worker.is_none() && !s.actor {
            s.ticket = None;
            s.prepared = Weak::new();
        }
        self.changed.notify_all();
    }
    fn owner_done(&self, ticket: Id) {
        let mut s = self.state();
        if s.ticket == Some(ticket) {
            s.owner_live = false;
            self.retire_if_done(&mut s);
        }
    }
    fn worker_done(&self, ticket: Id) {
        let mut s = self.state();
        if s.ticket == Some(ticket) {
            s.worker = None;
            self.retire_if_done(&mut s);
        }
    }
    fn actor_done(&self, ticket: Id) {
        let mut s = self.state();
        if s.ticket == Some(ticket) {
            s.actor = false;
            self.retire_if_done(&mut s);
        }
    }
    fn clear_idle(&self, force: bool) {
        // Retain the upgraded slot after releasing the state lock: its final
        // Permit destructor also locks state and must never run under it.
        let slot = {
            let s = self.state();
            if s.worker.is_some() {
                return;
            }
            s.prepared.upgrade()
        };
        let Some(slot) = slot else {
            return;
        };
        if !force && slot.authority.check().is_ok() {
            return;
        }
        let plan = {
            let s = self.state();
            if s.worker.is_some() || s.ticket != Some(slot.lease.ticket) {
                return;
            }
            slot.plan.lock().unwrap_or_else(|p| p.into_inner()).take()
        };
        if let Some(plan) = plan {
            drop(plan);
            self.owner_done(slot.lease.ticket);
        }
    }
    fn reserve(self: &Arc<Self>) -> Result<Permit> {
        // Called on the optional review worker, not the input thread. Reclaim
        // cancelled idle metadata before admitting another plan.
        self.clear_idle(false);
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
        Ok(Permit {
            scope: Arc::clone(self),
            ticket,
        })
    }
    fn bind(&self, slot: &Arc<Slot>) -> Result<()> {
        let mut s = self.state();
        if s.closed || s.ticket != Some(slot.lease.ticket) {
            return Err(Error::Cancelled);
        }
        s.prepared = Arc::downgrade(slot);
        Ok(())
    }
    fn start(self: &Arc<Self>, slot: &Arc<Slot>) -> Result<(StreamBackupPlan, Active)> {
        let mut s = self.state();
        if s.closed || s.ticket != Some(slot.lease.ticket) || s.worker.is_some() {
            return Err(Error::Cancelled);
        }
        slot.authority.check()?;
        let plan = slot
            .plan
            .lock()
            .map_err(|_| Error::WorkerFailed)?
            .take()
            .ok_or(Error::Cancelled)?;
        s.worker = Some(thread::current().id());
        Ok((
            plan,
            Active {
                scope: Arc::clone(self),
                ticket: slot.lease.ticket,
            },
        ))
    }
    fn attach(self: &Arc<Self>, ticket: Id) -> Result<ActorLease> {
        let mut s = self.state();
        if s.closed || s.ticket != Some(ticket) || s.worker.is_none() || s.actor {
            return Err(Error::Cancelled);
        }
        s.actor = true;
        Ok(ActorLease {
            scope: Arc::clone(self),
            ticket,
        })
    }
    pub fn close(&self) {
        {
            let mut s = self.state();
            s.closed = true;
        }
        self.clear_idle(true);
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
    fn status(&self) -> BackupStatus {
        let s = self.state();
        BackupStatus {
            reserved: s.ticket.is_some(),
            running: s.worker.is_some(),
            closed: s.closed,
        }
    }
}
pub(super) struct Permit {
    scope: Arc<BackupScope>,
    ticket: Id,
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.scope.owner_done(self.ticket);
    }
}
struct Active {
    scope: Arc<BackupScope>,
    ticket: Id,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.scope.worker_done(self.ticket);
    }
}
struct ActorLease {
    scope: Arc<BackupScope>,
    ticket: Id,
}
impl Drop for ActorLease {
    fn drop(&mut self) {
        self.scope.actor_done(self.ticket);
    }
}
struct Slot {
    plan: Mutex<Option<StreamBackupPlan>>,
    authority: WorkAuthority,
    lease: Permit,
}
// Field drop order keeps the worker counted through owned sink/slot retirement.
struct Running<W: Write> {
    sink: Option<W>,
    _slot: Arc<Slot>,
    _active: Active,
}
struct CursorState {
    cursor: StreamBackupCursor,
    authority: WorkAuthority,
    ticket: Id,
    _lease: ActorLease,
}
pub(super) enum Request {
    Review {
        options: BackupOptions,
        permit: Permit,
        reply: mpsc::Sender<Result<BackupReview>>,
    },
    Open {
        plan: Box<StreamBackupPlan>,
        authority: WorkAuthority,
        auth: Box<dyn Authenticator + Send>,
        control: JobControl,
        reply: mpsc::Sender<Result<()>>,
    },
    Pull {
        ticket: Id,
        control: JobControl,
        reply: mpsc::Sender<Result<Option<Vec<u8>>>>,
    },
    Validate {
        ticket: Id,
        control: JobControl,
        reply: mpsc::Sender<Result<()>>,
    },
    Release {
        ticket: Id,
        reply: mpsc::Sender<Result<()>>,
    },
}
pub(super) struct BackupActor {
    scope: Arc<BackupScope>,
    cursor: Option<CursorState>,
}
impl BackupActor {
    pub fn new(scope: Arc<BackupScope>) -> Self {
        Self {
            scope,
            cursor: None,
        }
    }
    pub fn clear(&mut self) {
        drop(self.cursor.take());
    }
    fn state(&mut self, ticket: Id) -> Result<&mut CursorState> {
        let s = self.cursor.as_mut().ok_or(Error::Unavailable)?;
        if s.ticket != ticket {
            return Err(Error::Denied);
        }
        s.authority.check()?;
        Ok(s)
    }
    pub fn handle(
        &mut self,
        request: Request,
        store: &Store,
        access: &Access,
        recognition: &RecognitionScope,
        complete: impl FnOnce(bool),
    ) {
        macro_rules! answer {
            ($reply:expr,$body:expr) => {{
                let result =
                    catch_unwind(AssertUnwindSafe($body)).unwrap_or(Err(Error::WorkerFailed));
                complete(result.is_err());
                let _ = $reply.send(result);
            }};
        }
        match request {
            Request::Review {
                options,
                permit,
                reply,
            } => answer!(reply, || {
                let authority = recognition.optional_authority(access, permit.ticket)?;
                let plan = store.prepare_stream_backup(options, access)?;
                if plan.admission_bytes()? > self.scope.metadata_limit {
                    return Err(Error::ResourceLimit);
                }
                authority.check()?;
                let summary = BackupSummary {
                    includes_original_ink: plan.includes_original_ink(),
                    source_assets: plan.source_assets(),
                    omissions: plan.omissions().to_vec(),
                };
                let slot = Arc::new(Slot {
                    plan: Mutex::new(Some(plan)),
                    authority,
                    lease: permit,
                });
                self.scope.bind(&slot)?;
                Ok(BackupReview { slot, summary })
            }),
            Request::Open {
                plan,
                authority,
                mut auth,
                control,
                reply,
            } => answer!(reply, || {
                let result = (|| {
                    recognition.check_optional_authority(&authority)?;
                    control.check()?;
                    if self.cursor.is_some() {
                        return Err(Error::ResourceLimit);
                    }
                    let lease = self.scope.attach(authority.ticket)?;
                    let cursor =
                        store.open_stream_backup_cursor(*plan, auth.as_mut(), true, &control)?;
                    authority.check()?;
                    control.check()?;
                    self.cursor = Some(CursorState {
                        cursor,
                        ticket: authority.ticket,
                        authority,
                        _lease: lease,
                    });
                    Ok(())
                })();
                drop(auth);
                result
            }),
            Request::Pull {
                ticket,
                control,
                reply,
            } => answer!(reply, || {
                let s = self.state(ticket)?;
                control.check()?;
                let chunk = s.cursor.next_chunk(store, access, &control)?;
                s.authority.check()?;
                Ok(chunk)
            }),
            Request::Validate {
                ticket,
                control,
                reply,
            } => answer!(reply, || {
                let s = self.state(ticket)?;
                s.cursor.validate_complete(store, access, &control)?;
                s.authority.check()?;
                Ok(())
            }),
            Request::Release { ticket, reply } => answer!(reply, || {
                if self.cursor.as_ref().is_some_and(|s| s.ticket != ticket) {
                    return Err(Error::Denied);
                }
                self.clear();
                Ok(())
            }),
        }
    }
}
impl Host {
    pub fn backup_status(&self) -> BackupStatus {
        self.backups.status()
    }
    /// Trusted core review, off the input thread. No ordinary JSON can construct
    /// a review, output claim, protected proof or sink capability.
    pub fn review_backup(&self, options: BackupOptions) -> Result<BackupReview> {
        if self.writer_thread == thread::current().id() {
            return Err(Error::WorkerFailed);
        }
        let permit = self.backups.reserve()?;
        let (tx, rx) = mpsc::channel();
        self.mailbox.admit(
            Message::Backup(Box::new(Request::Review {
                options,
                permit,
                reply: tx,
            })),
            false,
            256,
        )?;
        rx.recv().map_err(|_| Error::WorkerFailed)?
    }
    /// Consumes and retires the owned sink before acknowledging a recovery key.
    /// Generic Write flush is not an OS file-sync/publication guarantee.
    pub fn finish_backup<W: Write>(
        &self,
        review: BackupReview,
        password: &str,
        auth: Box<dyn Authenticator + Send>,
        confirmed: bool,
        sink: W,
        control: &JobControl,
    ) -> Result<BackupReceipt> {
        if self.writer_thread == thread::current().id() {
            return Err(Error::WorkerFailed);
        }
        if !Arc::ptr_eq(&review.slot.lease.scope, &self.backups) {
            return Err(Error::Denied);
        }
        if !confirmed {
            return Err(Error::ReviewRequired);
        }
        control.check()?;
        let (plan, active) = self.backups.start(&review.slot)?;
        let authority = review.slot.authority.clone();
        let ticket = authority.ticket;
        let control = control.linked(Arc::clone(&authority.valid));
        let mut running = Running {
            sink: Some(sink),
            _slot: review.slot,
            _active: active,
        };
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<Zeroizing<String>> {
            let (tx, rx) = mpsc::channel();
            self.mailbox.admit(
                Message::Backup(Box::new(Request::Open {
                    plan: Box::new(plan),
                    authority: authority.clone(),
                    auth,
                    control: control.clone(),
                    reply: tx,
                })),
                false,
                256,
            )?;
            rx.recv().map_err(|_| Error::WorkerFailed)??;
            authority.check()?;
            control.check()?;
            let mut encrypted =
                EncryptStream::new(running.sink.as_mut().ok_or(Error::WorkerFailed)?, password)?;
            loop {
                authority.check()?;
                control.check()?;
                let (tx, rx) = mpsc::channel();
                self.mailbox.admit(
                    Message::Backup(Box::new(Request::Pull {
                        ticket,
                        control: control.clone(),
                        reply: tx,
                    })),
                    false,
                    128,
                )?;
                let chunk = rx.recv().map_err(|_| Error::WorkerFailed)??;
                authority.check()?;
                control.check()?;
                let Some(chunk) = chunk else {
                    break;
                };
                encrypted.write_all(&chunk)?;
            }
            self.validate_backup(ticket, &control)?;
            authority.check()?;
            let (_, key) = encrypted.finish()?;
            authority.check()?;
            control.check()?;
            Ok(key)
        }))
        .unwrap_or(Err(Error::WorkerFailed));
        // Catch callback panics before dropping user-owned W, then catch its
        // destructor separately: a write panic must not unwind through W::drop.
        let retired = catch_unwind(AssertUnwindSafe(|| drop(running.sink.take())))
            .map_err(|_| Error::WorkerFailed);
        let result = result.and_then(|key| {
            retired?;
            authority.check()?;
            control.check()?;
            self.validate_backup(ticket, &control)?;
            Ok(BackupReceipt { recovery_key: key })
        });
        let (tx, rx) = mpsc::channel();
        let cleanup = self
            .mailbox
            .admit_backup_cleanup(Request::Release { ticket, reply: tx })
            .and_then(|_| rx.recv().map_err(|_| Error::WorkerFailed)?);
        let result = result.and_then(|receipt| {
            cleanup?;
            authority.check()?;
            control.check()?;
            Ok(receipt)
        });
        drop(running); // sink already retired, then slot, then worker guard
        result
    }
    fn validate_backup(&self, ticket: Id, control: &JobControl) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        self.mailbox.admit(
            Message::Backup(Box::new(Request::Validate {
                ticket,
                control: control.clone(),
                reply: tx,
            })),
            false,
            128,
        )?;
        rx.recv().map_err(|_| Error::WorkerFailed)?
    }
}
