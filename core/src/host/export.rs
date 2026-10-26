//! One opaque native-export snapshot per host; encoding/authentication/encryption
//! execute on the caller worker. Release checks the live writer again.
use super::{Host, Message, RecognitionScope};
use crate::{
    portable::{ExportPlan, ExportProtection, ExportStamp, Exported},
    processing::{JobControl, WorkAuthority},
    security::{Access, Authenticator},
    store::Store,
    Error, Id, Result,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{mpsc, Arc, Condvar, Mutex, MutexGuard},
    thread::{self, ThreadId},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum NativeExportSelection {
    Entities(Vec<Id>),
    Exercise(Id),
}
#[derive(Clone, Serialize)]
pub struct NativeExportSummary {
    pub selected_entities: usize,
    pub selected_blocks: usize,
    pub includes_raw_ink: bool,
    pub omissions: Vec<String>,
}
struct State {
    closed: bool,
    ticket: Option<Id>,
    worker: Option<ThreadId>,
}
pub(super) struct ExportScope {
    state: Mutex<State>,
    changed: Condvar,
}
impl ExportScope {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                closed: false,
                ticket: None,
                worker: None,
            }),
            changed: Condvar::new(),
        })
    }
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn reserve(self: &Arc<Self>) -> Result<Arc<Lease>> {
        let mut state = self.state();
        if state.closed {
            return Err(Error::WorkerFailed);
        }
        if state.ticket.is_some() {
            return Err(Error::ResourceLimit);
        }
        let ticket = Id::new();
        state.ticket = Some(ticket);
        Ok(Arc::new(Lease {
            scope: Arc::clone(self),
            ticket,
        }))
    }
    pub fn close(&self) {
        self.state().closed = true;
        self.changed.notify_all();
    }
    pub fn is_current_worker(&self) -> bool {
        self.state().worker == Some(thread::current().id())
    }
    pub fn wait(&self) {
        let mut s = self.state();
        while s.worker.is_some() {
            s = self.changed.wait(s).unwrap_or_else(|p| p.into_inner());
        }
    }
}
pub(super) struct Lease {
    scope: Arc<ExportScope>,
    ticket: Id,
}
impl Lease {
    fn check(&self) -> Result<()> {
        let s = self.scope.state();
        if s.closed || s.ticket != Some(self.ticket) {
            return Err(Error::Cancelled);
        }
        Ok(())
    }
    fn begin(self: &Arc<Self>) -> Result<Worker> {
        let mut s = self.scope.state();
        if s.closed || s.ticket != Some(self.ticket) {
            return Err(Error::Cancelled);
        }
        if s.worker.is_some() {
            return Err(Error::Conflict);
        }
        s.worker = Some(thread::current().id());
        Ok(Worker(Arc::clone(self)))
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let mut s = self.scope.state();
        if s.ticket == Some(self.ticket) {
            s.ticket = None;
        }
        self.scope.changed.notify_all();
    }
}
struct Worker(Arc<Lease>);
impl Drop for Worker {
    fn drop(&mut self) {
        self.0.scope.state().worker = None;
        self.0.scope.changed.notify_all();
    }
}
pub struct NativeExportReview {
    lease: Arc<Lease>,
    authority: WorkAuthority,
    summary: NativeExportSummary,
    plan: ExportPlan,
}
impl NativeExportReview {
    pub fn summary(&self) -> &NativeExportSummary {
        &self.summary
    }
}
pub struct CompletedNativeExport {
    control: JobControl,
    lease: Arc<Lease>,
    authority: WorkAuthority,
    stamp: ExportStamp,
    output: Exported,
}
pub(super) enum Request {
    Prepare {
        selection: NativeExportSelection,
        include_ink: bool,
        lease: Arc<Lease>,
        reply: mpsc::Sender<Result<NativeExportReview>>,
    },
    Validate {
        stamp: ExportStamp,
        authority: WorkAuthority,
        reply: mpsc::Sender<Result<()>>,
    },
}
pub(super) fn handle(
    request: Request,
    store: &Store,
    access: &Access,
    recognition: &RecognitionScope,
    complete: impl FnOnce(bool),
) {
    match request {
        Request::Prepare {
            selection,
            include_ink,
            lease,
            reply,
        } => {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                lease.check()?;
                let authority = recognition.optional_authority(access, lease.ticket)?;
                let plan = match selection {
                    NativeExportSelection::Entities(ids) => {
                        store.prepare_native_export(&ids, include_ink, access)?
                    }
                    NativeExportSelection::Exercise(id) => {
                        store.prepare_exercise_export(id, include_ink, access)?
                    }
                };
                plan.admission_bytes()?;
                authority.check()?;
                lease.check()?;
                let summary = NativeExportSummary {
                    selected_entities: plan.selected_entities,
                    selected_blocks: plan.selected_blocks,
                    includes_raw_ink: plan.includes_raw_ink,
                    omissions: plan.omissions.clone(),
                };
                Ok(NativeExportReview {
                    lease,
                    authority,
                    summary,
                    plan,
                })
            }))
            .unwrap_or(Err(Error::WorkerFailed));
            complete(result.is_err());
            let _ = reply.send(result);
        }
        Request::Validate {
            stamp,
            authority,
            reply,
        } => {
            let result = recognition
                .check_optional_authority(&authority)
                .and_then(|()| store.validate_export_stamp(&stamp, Some(access)));
            complete(result.is_err());
            let _ = reply.send(result);
        }
    }
}
impl Host {
    pub fn prepare_native_export(
        &self,
        selection: NativeExportSelection,
        include_original_ink: bool,
    ) -> Result<NativeExportReview> {
        if let NativeExportSelection::Entities(ids) = &selection {
            if ids.is_empty() || ids.len() > 10000 {
                return Err(Error::Invalid("native export selection"));
            }
        }
        let lease = self.exports.reserve()?;
        let bytes = match &selection {
            NativeExportSelection::Entities(ids) => 128 + ids.len() * 16,
            NativeExportSelection::Exercise(_) => 144,
        };
        let (tx, rx) = mpsc::channel();
        self.mailbox.admit(
            Message::NativeExport(Box::new(Request::Prepare {
                selection,
                include_ink: include_original_ink,
                lease,
                reply: tx,
            })),
            false,
            bytes,
        )?;
        rx.recv().map_err(|_| Error::WorkerFailed)?
    }
    /// Run on a caller-owned worker. A completed artifact has no public byte accessor.
    pub fn encode_native_export(
        &self,
        review: NativeExportReview,
        protection: ExportProtection<'_>,
        auth: &mut dyn Authenticator,
        acknowledge_omissions: bool,
        control: &JobControl,
    ) -> Result<CompletedNativeExport> {
        if !Arc::ptr_eq(&self.exports, &review.lease.scope) {
            return Err(Error::Denied);
        }
        self.recognition
            .check_optional_authority(&review.authority)?;
        let worker = review.lease.begin()?;
        let stamp = review.plan.stamp();
        let control = control.linked_authority(&review.authority);
        let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            review
                .plan
                .encode(protection, auth, acknowledge_omissions, Some(&control))
        }))
        .unwrap_or(Err(Error::WorkerFailed))?;
        review.authority.check()?;
        review.lease.check()?;
        drop(worker);
        Ok(CompletedNativeExport {
            control,
            lease: review.lease,
            authority: review.authority,
            stamp,
            output,
        })
    }
    pub fn finish_native_export(&self, completed: CompletedNativeExport) -> Result<Exported> {
        if !Arc::ptr_eq(&self.exports, &completed.lease.scope) {
            return Err(Error::Denied);
        }
        completed.control.check()?;
        completed.lease.check()?;
        completed.authority.check()?;
        let (tx, rx) = mpsc::channel();
        let bytes = super::recognition::charge(&completed.stamp)?;
        self.mailbox.admit(
            Message::NativeExport(Box::new(Request::Validate {
                stamp: completed.stamp,
                authority: completed.authority.clone(),
                reply: tx,
            })),
            false,
            bytes,
        )?;
        rx.recv().map_err(|_| Error::WorkerFailed)??;
        completed.control.check()?;
        completed.lease.check()?;
        completed.authority.check()?;
        Ok(completed.output)
    }
}
