//! Worker-decoded/remapped native copies, followed by guarded insert-only SQL.
//! The durable operation receipt is committed with every imported object.
use super::{export::Lease, Host, Message, RecognitionScope};
use crate::{
    crypto::Credential,
    portable::{
        prepare_native_import_bytes, ExportStamp, NativeImportReceipt, NativeImportSummary,
        PreparedNativeImport,
    },
    processing::{JobControl, WorkAuthority},
    security::{Access, Authenticator, Scope},
    store::Store,
    Error, Id, Result,
};
use std::sync::{mpsc, Arc};
pub struct NativeImportSource {
    lease: Arc<Lease>,
    authority: WorkAuthority,
    stamp: ExportStamp,
}
impl NativeImportSource {
    /// Identity only, not a permission grant. Retain it before publication.
    pub fn operation(&self) -> Id {
        self.lease.ticket
    }
}
pub struct NativeImportReview {
    lease: Arc<Lease>,
    authority: WorkAuthority,
    stamp: ExportStamp,
    control: JobControl,
    digest: String,
    prepared: PreparedNativeImport,
}
impl NativeImportReview {
    pub fn operation(&self) -> Id {
        self.lease.ticket
    }
    pub fn summary(&self) -> Result<&NativeImportSummary> {
        self.lease.check()?;
        self.authority.check()?;
        self.control.check()?;
        Ok(&self.prepared.summary)
    }
}
pub(super) enum Request {
    Begin {
        lease: Arc<Lease>,
        reply: mpsc::Sender<Result<NativeImportSource>>,
    },
    Publish {
        prepared: Box<PreparedNativeImport>,
        operation: Id,
        digest: String,
        authority: WorkAuthority,
        stamp: Box<ExportStamp>,
        control: JobControl,
        reply: mpsc::Sender<Result<NativeImportReceipt>>,
    },
    Receipt {
        operation: Id,
        reply: mpsc::Sender<Result<Option<NativeImportReceipt>>>,
    },
}
pub(super) fn handle(
    request: Request,
    store: &mut Store,
    access: &Access,
    recognition: &RecognitionScope,
    complete: impl FnOnce(bool),
) {
    match request {
        Request::Begin { lease, reply } => {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                lease.check()?;
                Store::require_app(&store.db, access)?;
                let authority = recognition.optional_authority(access, lease.ticket)?;
                let scopes = if Store::setting(&store.db, "app_locked")? == "true" {
                    [Scope::App].into_iter().collect()
                } else {
                    Default::default()
                };
                let stamp = ExportStamp::new(
                    Store::setting(&store.db, "library_id")?,
                    Store::setting(&store.db, "generation")?,
                    scopes,
                );
                authority.check()?;
                Ok(NativeImportSource {
                    lease,
                    authority,
                    stamp,
                })
            }))
            .unwrap_or(Err(Error::WorkerFailed));
            complete(result.is_err());
            let _ = reply.send(result);
        }
        Request::Publish {
            prepared,
            operation,
            digest,
            authority,
            stamp,
            control,
            reply,
        } => {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                recognition.check_optional_authority(&authority)?;
                control.check()?;
                // A known commit is an acknowledgement, not another write.
                if let Some(receipt) = store.native_import_receipt(operation, access)? {
                    if receipt.artifact_digest != digest {
                        return Err(Error::Conflict);
                    }
                    return Ok(receipt);
                }
                store.validate_export_stamp(&stamp, Some(access))?;
                let result = store.publish_prepared_native(
                    *prepared,
                    access,
                    Some(stamp.guard()),
                    Some((operation, &digest)),
                    Some(&control),
                )?;
                // Never turn a committed transaction into a cancellation error.
                result.receipt.ok_or(Error::Integrity)
            }))
            .unwrap_or(Err(Error::WorkerFailed));
            complete(result.is_err());
            let _ = reply.send(result);
        }
        Request::Receipt { operation, reply } => {
            let result = store.native_import_receipt(operation, access);
            complete(result.is_err());
            let _ = reply.send(result);
        }
    }
}
impl Host {
    pub fn native_import_source(&self) -> Result<NativeImportSource> {
        let lease = self.exports.reserve()?;
        let (tx, rx) = mpsc::channel();
        self.mailbox.admit(
            Message::NativeImport(Box::new(Request::Begin { lease, reply: tx })),
            false,
            128,
        )?;
        rx.recv().map_err(|_| Error::WorkerFailed)?
    }
    /// Caller-worker decode/decrypt/validation, remapping and row serialization.
    pub fn prepare_native_import(
        &self,
        source: NativeImportSource,
        bytes: &[u8],
        credential: Option<Credential<'_>>,
        control: &JobControl,
    ) -> Result<NativeImportReview> {
        if !Arc::ptr_eq(&self.exports, &source.lease.scope) {
            return Err(Error::Denied);
        }
        self.recognition
            .check_optional_authority(&source.authority)?;
        let worker = source.lease.begin()?;
        let control = control.linked_authority(&source.authority);
        let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prepare_native_import_bytes(bytes, credential, &control)
        }))
        .unwrap_or(Err(Error::WorkerFailed))?;
        super::recognition::charge(&prepared.summary)?;
        control.check()?;
        source.lease.check()?;
        let digest = crate::digest(bytes);
        control.check()?;
        drop(worker);
        Ok(NativeImportReview {
            lease: source.lease,
            authority: source.authority,
            stamp: source.stamp,
            control,
            digest,
            prepared,
        })
    }
    /// Once SQL commits, return its receipt even if cancellation/close follows.
    /// On a lost transport response reconcile the operation before retrying.
    pub fn publish_native_import(
        &self,
        review: NativeImportReview,
        auth: &mut dyn Authenticator,
        confirmed: bool,
        acknowledge_omissions: bool,
    ) -> Result<NativeImportReceipt> {
        if !Arc::ptr_eq(&self.exports, &review.lease.scope) {
            return Err(Error::Denied);
        }
        review.summary()?;
        let worker = review.lease.begin()?;
        if !confirmed
            || (!review.prepared.summary.source_omissions.is_empty() && !acknowledge_omissions)
        {
            return Err(Error::ReviewRequired);
        }
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| review.stamp.verify(auth)))
            .unwrap_or(Err(Error::WorkerFailed))?;
        review.control.check()?;
        review.lease.check()?;
        let (tx, rx) = mpsc::channel();
        let operation = review.lease.ticket;
        // The separately bounded, exclusively reserved transfer slot owns the
        // row payload; the mailbox envelope carries its ownership, not a copy.
        self.mailbox.admit(
            Message::NativeImport(Box::new(Request::Publish {
                prepared: Box::new(review.prepared),
                operation,
                digest: review.digest,
                authority: review.authority,
                stamp: Box::new(review.stamp),
                control: review.control,
                reply: tx,
            })),
            true,
            256,
        )?;
        let result = rx.recv().map_err(|_| Error::WorkerFailed)?;
        drop(worker);
        result
    }
    /// None means no committed receipt observed; retire pending work before retry.
    pub fn native_import_receipt(&self, operation: Id) -> Result<Option<NativeImportReceipt>> {
        let (tx, rx) = mpsc::channel();
        self.mailbox.admit(
            Message::NativeImport(Box::new(Request::Receipt {
                operation,
                reply: tx,
            })),
            false,
            128,
        )?;
        rx.recv().map_err(|_| Error::WorkerFailed)?
    }
}
