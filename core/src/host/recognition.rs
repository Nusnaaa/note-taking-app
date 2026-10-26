//! Bounded trusted native recognition transport. Callbacks never run on the writer.
use crate::{
    processing::{CompletedRecognition, Manager, RecognitionWork, Task, WorkAuthority},
    security::Access,
    store::{InkSnapshot, Store},
    Error, Id, Result,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Condvar, Mutex, MutexGuard, Weak,
    },
    thread::{self, ThreadId},
};
mod activation;
pub(super) mod resource_import;
pub(super) use resource_import::ImportPublication;
pub use resource_import::{
    HostEngineImportPlan, HostEngineImportTransfer, HostPreparedEngineImport,
};
pub(super) mod stored_activation;
pub use activation::{
    HostActivationSource, HostActivationWork, HostCompletedActivation, HostDetachedRuntime,
    HostDetachmentSource,
};
pub(super) use stored_activation::EnginePublication;
pub use stored_activation::{HostEngineActivationPlan, HostPreparedEngineActivation};
const MAX_FRAME: usize = 8 * 1024 * 1024;
const MAX_SOURCE_BYTES: usize = 32 * 1024 * 1024;
pub(super) fn charge(value: &impl Serialize) -> Result<usize> {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > (MAX_FRAME - 256).saturating_sub(self.0) {
                return Err(io::Error::other("processing frame limit"));
            }
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut count = Counter(0);
    serde_json::to_writer(&mut count, value).map_err(|_| Error::ResourceLimit)?;
    Ok(count.0 + 256)
}
struct Entry {
    slot: Weak<Slot>,
    bytes: usize,
    running: Option<ThreadId>,
    retiring: Option<ThreadId>,
    retire_pending: bool,
}
struct State {
    valid: Arc<AtomicBool>,
    closed: bool,
    entries: BTreeMap<Id, Entry>,
    capture: BTreeSet<Id>,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RecognitionStatus {
    pub reserved: usize,
    pub running: usize,
    pub closed: bool,
    pub capture_active: bool,
}
pub(super) struct RecognitionScope {
    owner: Id,
    capacity: usize,
    state: Mutex<State>,
    changed: Condvar,
    retire_sender: Mutex<Option<mpsc::Sender<Retirement>>>,
    retire_worker: Mutex<Option<thread::JoinHandle<()>>>,
    spawn_retirement: fn(mpsc::Receiver<Retirement>) -> io::Result<thread::JoinHandle<()>>,
}
struct Retirement {
    scope: Arc<RecognitionScope>,
    ticket: Id,
    payload: Phase,
}
fn retire_now(task: Retirement) {
    task.scope.note_cleanup_started(task.ticket);
    let failed =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(task.payload))).is_err();
    task.scope.release(task.ticket);
    if failed {
        task.scope.close();
    }
}
enum Phase {
    Source(Option<InkSnapshot>),
    Prepared(Option<Box<RecognitionWork>>),
    ActivationInput,
    StoredPlan(Option<Box<crate::store::EngineActivationPlan>>),
    ImportPlan(Option<Box<crate::store::EngineImportPlan>>),
    ImportTransfer(Option<Box<crate::store::EngineImportTransfer>>),
    ImportPrepared(Option<Box<crate::store::PreparedEngineImport>>),
    StoredPrepared(Arc<stored_activation::StoredCandidate>),
    StoredPublishing(Arc<stored_activation::StoredCandidate>),
    StoredPublished {
        _candidate: Arc<stored_activation::StoredCandidate>,
    },
    DetachmentInput,
    RuntimeDetached {
        _runtime: Box<crate::processing::DetachedRuntime>,
    },
    ActivationPrepared(Option<Box<crate::processing::ActivationWork>>),
    ActivationResult(Option<Box<crate::processing::CompletedActivation>>),
    Running,
    Finished,
}
struct Slot {
    authority: WorkAuthority,
    phase: Mutex<Phase>,
    lease: SlotLease,
}
impl Drop for Slot {
    fn drop(&mut self) {
        self.lease.scope.note_retiring(self.authority.ticket);
    }
}
// Drop the payload/runtime before releasing admission. The scope only keeps weak
// slots, so dropping a caller's unrun source/work releases it without a leak.
struct SlotLease {
    scope: Arc<RecognitionScope>,
    ticket: Id,
}
impl Drop for SlotLease {
    fn drop(&mut self) {
        self.scope.release_idle_owner(self.ticket);
    }
}
impl Slot {
    fn cancel_idle(&self) {
        let old = {
            let mut phase = self.phase.lock().unwrap_or_else(|p| p.into_inner());
            if matches!(
                *phase,
                Phase::Running | Phase::StoredPublishing(_) | Phase::Finished
            ) {
                return;
            }
            std::mem::replace(&mut *phase, Phase::Finished)
        };
        self.lease.scope.retire(self.authority.ticket, old);
    }
}
impl RecognitionScope {
    pub fn new(capacity: usize) -> Self {
        Self {
            owner: Id::new(),
            capacity,
            state: Mutex::new(State {
                valid: Arc::new(AtomicBool::new(true)),
                closed: false,
                entries: BTreeMap::new(),
                capture: BTreeSet::new(),
            }),
            changed: Condvar::new(),
            retire_sender: Mutex::new(None),
            retire_worker: Mutex::new(None),
            spawn_retirement: |receiver| {
                thread::Builder::new()
                    .name("stem-processing-retirement".into())
                    .spawn(move || {
                        while let Ok(task) = receiver.recv() {
                            retire_now(task);
                        }
                    })
            },
        }
    }
    fn ensure_retirement_lane(&self) -> Result<()> {
        let mut sender = self.retire_sender.lock().map_err(|_| Error::WorkerFailed)?;
        if sender.is_none() {
            let (tx, receiver) = mpsc::channel();
            let worker = (self.spawn_retirement)(receiver)?;
            *self.retire_worker.lock().unwrap_or_else(|p| p.into_inner()) = Some(worker);
            *sender = Some(tx);
        }
        Ok(())
    }
    fn state(&self) -> MutexGuard<'_, State> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poison) => {
                let mut state = poison.into_inner();
                state.closed = true;
                state.valid.store(false, Ordering::Release);
                state
            }
        }
    }
    pub(super) fn optional_authority(&self, access: &Access, ticket: Id) -> Result<WorkAuthority> {
        let state = self.state();
        if state.closed {
            return Err(Error::WorkerFailed);
        }
        if !state.capture.is_empty() {
            return Err(Error::ResourceLimit);
        }
        Ok(WorkAuthority {
            owner: self.owner,
            ticket,
            valid: Arc::clone(&state.valid),
            until: access.expires_at(),
        })
    }
    pub(super) fn check_optional_authority(&self, authority: &WorkAuthority) -> Result<()> {
        if authority.owner != self.owner {
            return Err(Error::Denied);
        }
        authority.check()
    }
    pub fn status(&self) -> RecognitionStatus {
        let state = self.state();
        RecognitionStatus {
            reserved: state.entries.len(),
            running: state
                .entries
                .values()
                .filter(|entry| entry.running.is_some())
                .count(),
            closed: state.closed,
            capture_active: !state.capture.is_empty(),
        }
    }
    fn cancel(&self, close: bool) {
        let slots = {
            let mut state = self.state();
            state.closed |= close;
            state.valid.store(false, Ordering::Release);
            if !state.closed {
                state.valid = Arc::new(AtomicBool::new(true));
            }
            state
                .entries
                .values()
                .filter_map(|entry| entry.slot.upgrade())
                .collect::<Vec<_>>()
        };
        for slot in slots {
            slot.cancel_idle();
        }
    }
    /// Trusted input-session signal. No document read, callback or writer wait.
    /// Repeated begin/end are idempotent; one contact cannot release another.
    pub fn capture_priority(&self, session: Id, active: bool) -> Result<()> {
        let (slots, overflow) = {
            let mut state = self.state();
            if !active {
                state.capture.remove(&session);
                return Ok(());
            }
            if state.closed {
                return Ok(()); // optional processing is already disabled
            }
            if state.capture.contains(&session) {
                return Ok(());
            }
            let overflow = state.capture.len() >= 16;
            if overflow {
                // Never continue optional inference after losing a capture signal.
                state.closed = true;
            } else {
                state.capture.insert(session);
            }
            state.valid.store(false, Ordering::Release);
            if !state.closed {
                state.valid = Arc::new(AtomicBool::new(true));
            }
            (
                state
                    .entries
                    .values()
                    .filter_map(|e| e.slot.upgrade())
                    .collect::<Vec<_>>(),
                overflow,
            )
        };
        for slot in slots {
            slot.cancel_idle();
        }
        if overflow {
            Err(Error::ResourceLimit)
        } else {
            Ok(())
        }
    }
    pub fn invalidate(&self) {
        self.cancel(false);
    }
    pub fn close(&self) {
        self.cancel(true);
    }
    pub fn is_current_worker(&self) -> bool {
        self.state().entries.values().any(|entry| {
            entry.running == Some(thread::current().id())
                || entry.retiring == Some(thread::current().id())
        })
    }
    pub fn wait_workers(&self) {
        let mut state = self.state();
        while !state.entries.is_empty() {
            state = self.changed.wait(state).unwrap_or_else(|p| p.into_inner());
        }
        drop(state);
        self.retire_sender
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(worker) = self
            .retire_worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            let _ = worker.join();
        }
    }
    fn note_retiring(&self, ticket: Id) {
        if let Some(entry) = self.state().entries.get_mut(&ticket) {
            if !entry.retire_pending {
                entry.retiring = Some(thread::current().id());
            }
        }
    }
    fn note_cleanup_started(&self, ticket: Id) {
        if let Some(entry) = self.state().entries.get_mut(&ticket) {
            entry.retiring = Some(thread::current().id());
        }
    }
    fn release_idle_owner(&self, ticket: Id) {
        let mut state = self.state();
        if state
            .entries
            .get(&ticket)
            .is_some_and(|entry| !entry.retire_pending)
        {
            state.entries.remove(&ticket);
        }
        self.changed.notify_all();
    }
    fn retire(self: &Arc<Self>, ticket: Id, payload: Phase) {
        if let Some(entry) = self.state().entries.get_mut(&ticket) {
            entry.retire_pending = true;
        }
        let task = Retirement {
            scope: Arc::clone(self),
            ticket,
            payload,
        };
        let sender = self
            .retire_sender
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(sender) = sender {
            if let Err(error) = sender.send(task) {
                retire_now(error.0);
            }
        } else {
            retire_now(task);
        }
    }
    fn release(&self, ticket: Id) {
        self.state().entries.remove(&ticket);
        self.changed.notify_all();
    }
    pub fn source(
        self: &Arc<Self>,
        store: &Store,
        access: &Access,
        block: Id,
        region: Option<Id>,
    ) -> Result<RecognitionSource> {
        {
            let state = self.state();
            if state.closed {
                return Err(Error::WorkerFailed);
            }
            if !state.capture.is_empty() || state.entries.len() >= self.capacity {
                return Err(Error::ResourceLimit);
            }
        }
        let snapshot = if let Some(region) = region {
            store.region_snapshot(block, region, access)?
        } else {
            store.ink_snapshot(block, access)?
        };
        let bytes = charge(&snapshot)?;
        self.ensure_retirement_lane()?;
        let mut state = self.state();
        if state.closed {
            return Err(Error::WorkerFailed);
        }
        if !state.capture.is_empty()
            || state.entries.len() >= self.capacity
            || bytes
                > MAX_SOURCE_BYTES
                    .saturating_sub(state.entries.values().map(|entry| entry.bytes).sum())
        {
            return Err(Error::ResourceLimit);
        }
        let ticket = Id::new();
        let revision = snapshot.revision();
        let slot = Arc::new(Slot {
            authority: WorkAuthority {
                owner: self.owner,
                ticket,
                valid: Arc::clone(&state.valid),
                until: access.expires_at(),
            },
            phase: Mutex::new(Phase::Source(Some(snapshot))),
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
        Ok(RecognitionSource {
            slot,
            block,
            revision,
        })
    }
    pub fn check_result(&self, completed: &CompletedRecognition) -> Result<()> {
        if completed
            .authority
            .as_ref()
            .is_none_or(|authority| authority.owner != self.owner)
        {
            return Err(Error::Denied);
        }
        completed.check_delivery()
    }
}
/// Move-only, non-deserialisable source. A ticket exists before inference so an
/// ambiguous publication can be discovered/retried without duplicate reviews.
pub struct RecognitionSource {
    slot: Arc<Slot>,
    block: Id,
    revision: u64,
}
impl RecognitionSource {
    pub fn ticket(&self) -> Id {
        self.slot.authority.ticket
    }
    pub fn block(&self) -> Id {
        self.block
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn prepare(
        self,
        manager: &Manager,
        task: Task,
        override_id: Option<&str>,
    ) -> Result<HostRecognitionWork> {
        self.slot.authority.check()?;
        {
            let mut phase = self.slot.phase.lock().map_err(|_| Error::WorkerFailed)?;
            let Phase::Source(snapshot) = &mut *phase else {
                return Err(Error::Cancelled);
            };
            let work = manager
                .prepare_recognition(snapshot.take().ok_or(Error::Cancelled)?, task, override_id)?
                .bind_authority(self.slot.authority.clone());
            *phase = Phase::Prepared(Some(Box::new(work)));
        }
        Ok(HostRecognitionWork { slot: self.slot })
    }
}
pub struct HostRecognitionWork {
    slot: Arc<Slot>,
}
impl Drop for HostRecognitionWork {
    fn drop(&mut self) {
        self.slot.cancel_idle();
    }
}
struct RunningGuard {
    slot: Arc<Slot>,
}
impl Drop for RunningGuard {
    fn drop(&mut self) {
        *self.slot.phase.lock().unwrap_or_else(|p| p.into_inner()) = Phase::Finished;
        self.slot.lease.scope.release(self.slot.authority.ticket);
    }
}
impl HostRecognitionWork {
    pub fn ticket(&self) -> Id {
        self.slot.authority.ticket
    }
    pub fn run(self, control: &crate::processing::JobControl) -> Result<CompletedRecognition> {
        let work = {
            let mut state = self.slot.lease.scope.state();
            self.slot.authority.check()?;
            if state.closed {
                return Err(Error::Cancelled);
            }
            let mut phase = self.slot.phase.lock().map_err(|_| Error::WorkerFailed)?;
            let Phase::Prepared(work) = &mut *phase else {
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
        let mut completed = (*work).run(control)?;
        // No longer retain a second original-point copy in a computed result.
        // The core rechecks authoritative source/fingerprint when publishing.
        completed.snapshot.strokes = Vec::new();
        charge(&(
            &completed.snapshot,
            &completed.descriptor,
            &completed.output,
        ))?;
        Ok(completed)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    #[test]
    fn expired_processing_authority_cannot_be_refreshed_by_a_later_job_budget() {
        let authority = WorkAuthority {
            owner: Id::new(),
            ticket: Id::new(),
            valid: Arc::new(AtomicBool::new(true)),
            until: Some(Instant::now()),
        };
        assert!(matches!(authority.check(), Err(Error::Denied)));
    }
    #[test]
    fn oversized_processing_frames_fail_without_allocating_an_encoded_copy() {
        assert!(matches!(
            charge(&"x".repeat(MAX_FRAME)),
            Err(Error::ResourceLimit)
        ));
    }
    #[test]
    fn optional_retirement_worker_failure_does_not_disable_authoritative_writing() {
        let mut scope = RecognitionScope::new(8);
        scope.spawn_retirement = |_| Err(io::Error::other("synthetic worker startup failure"));
        let scope = Arc::new(scope);
        let mut store = Store::memory().unwrap();
        let access = Access::public();
        let note = store
            .create(crate::store::Kind::Note, None, "worker failure", &access)
            .unwrap();
        let stroke = crate::ink::Stroke {
            id: Id::new(),
            capture_session: Id::new(),
            sequence: 0,
            pointer_id: 1,
            coordinate_space: "fixture".into(),
            tool: crate::ink::Tool::Pen,
            style: None,
            end: Default::default(),
            points: vec![crate::ink::Point {
                x: 0.,
                y: 0.,
                time_us: 0,
                pressure: None,
                tilt_rad: None,
                orientation_rad: None,
                buttons: None,
            }],
        };
        let block = store
            .append_ink(note.id, vec![stroke.clone()], &access)
            .unwrap();
        assert!(scope.source(&store, &access, block.id, None).is_err());
        assert!(scope.activation_source(&store, &access).is_err());
        assert!(scope.detachment_source(&store, &access).is_err());
        assert_eq!(scope.status().reserved, 0);
        assert!(!scope.status().closed);
        assert!(store
            .append_ink(
                note.id,
                vec![crate::ink::Stroke {
                    id: Id::new(),
                    ..stroke
                }],
                &access
            )
            .is_ok());
    }
    #[test]
    fn closed_scope_never_issues_a_live_epoch_after_invalidation() {
        let scope = RecognitionScope::new(8);
        let old = Arc::clone(&scope.state().valid);
        scope.close();
        scope.invalidate();
        assert!(!old.load(Ordering::Acquire));
        assert!(!scope.state().valid.load(Ordering::Acquire));
    }
}
