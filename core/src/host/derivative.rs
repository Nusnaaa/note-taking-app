//! Authorized derivative snapshots and off-writer render/review/release.
use super::{
    export::{Lease, Request as ExportRequest},
    Host, Message, RecognitionScope,
};
use crate::{
    export::{DerivativeSource, ExportBundle, Page, Selection},
    portable::ExportStamp,
    processing::{JobControl, WorkAuthority},
    render::PdfOutput,
    security::{Access, Authenticator},
    store::Store,
    Error, Result,
};
use serde::Serialize;
use std::sync::{mpsc, Arc};
pub enum DerivativeFormat<'a> {
    Markdown,
    Pdf { font: &'a [u8], page: Page },
}
pub enum DerivativeOutput {
    Markdown(ExportBundle),
    Pdf(PdfOutput),
}
#[derive(Clone, Serialize)]
pub struct DerivativeSummary {
    pub selected_blocks: usize,
    pub includes_original_ink: bool,
    pub warnings: Vec<String>,
    pub assets: usize,
    pub bytes: usize,
    pub pages: Option<usize>,
}
pub struct HostDerivativeSource {
    lease: Arc<Lease>,
    authority: WorkAuthority,
    source: DerivativeSource,
}
pub struct DerivativeReview {
    lease: Arc<Lease>,
    authority: WorkAuthority,
    control: JobControl,
    stamp: ExportStamp,
    summary: DerivativeSummary,
    output: DerivativeOutput,
}
impl DerivativeReview {
    pub fn summary(&self) -> Result<&DerivativeSummary> {
        self.lease.check()?;
        self.authority.check()?;
        self.control.check()?;
        Ok(&self.summary)
    }
}
pub(super) struct Request {
    selection: Selection,
    include_ink: bool,
    lease: Arc<Lease>,
    reply: mpsc::Sender<Result<HostDerivativeSource>>,
}
pub(super) fn handle(
    request: Request,
    store: &Store,
    access: &Access,
    recognition: &RecognitionScope,
    complete: impl FnOnce(bool),
) {
    let Request {
        selection,
        include_ink,
        lease,
        reply,
    } = request;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        lease.check()?;
        let authority = recognition.optional_authority(access, lease.ticket)?;
        let source =
            store.derivative_source_authorised(selection, include_ink, access, Some(&authority))?;
        lease.check()?;
        authority.check()?;
        Ok(HostDerivativeSource {
            lease,
            authority,
            source,
        })
    }))
    .unwrap_or(Err(Error::WorkerFailed));
    complete(result.is_err());
    let _ = reply.send(result);
}
impl Host {
    pub fn derivative_source(
        &self,
        selection: Selection,
        include_original_ink: bool,
    ) -> Result<HostDerivativeSource> {
        let bytes = super::recognition::charge(&selection)?;
        let lease = self.exports.reserve()?;
        let (tx, rx) = mpsc::channel();
        self.mailbox.admit(
            Message::DerivativeExport(Box::new(Request {
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
    pub fn render_derivative(
        &self,
        source: HostDerivativeSource,
        format: DerivativeFormat<'_>,
        control: &JobControl,
    ) -> Result<DerivativeReview> {
        if !Arc::ptr_eq(&self.exports, &source.lease.scope) {
            return Err(Error::Denied);
        }
        self.recognition
            .check_optional_authority(&source.authority)?;
        let worker = source.lease.begin()?;
        let control = control.linked_authority(&source.authority);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            control.check()?;
            let plan = source.source.render(&control)?;
            let stamp = plan.stamp();
            let selected_blocks = plan.selected_blocks;
            let (output, summary) = match format {
                DerivativeFormat::Markdown => {
                    let bundle = plan.into_bundle();
                    let bytes =
                        bundle.markdown.len() + bundle.assets.values().map(Vec::len).sum::<usize>();
                    let summary = DerivativeSummary {
                        selected_blocks,
                        includes_original_ink: bundle.includes_original_ink,
                        warnings: bundle.warnings.clone(),
                        assets: bundle.assets.len(),
                        bytes,
                        pages: None,
                    };
                    (DerivativeOutput::Markdown(bundle), summary)
                }
                DerivativeFormat::Pdf { font, page } => {
                    let pdf = plan.render_pdf(font, page, Some(&control))?;
                    let summary = DerivativeSummary {
                        selected_blocks,
                        includes_original_ink: false,
                        warnings: pdf.warnings.clone(),
                        assets: 0,
                        bytes: pdf.bytes.len(),
                        pages: Some(pdf.pages),
                    };
                    (DerivativeOutput::Pdf(pdf), summary)
                }
            };
            control.check()?;
            source.lease.check()?;
            Ok((stamp, output, summary))
        }))
        .unwrap_or(Err(Error::WorkerFailed))?;
        drop(worker);
        Ok(DerivativeReview {
            lease: source.lease,
            authority: source.authority,
            control,
            stamp: result.0,
            output: result.1,
            summary: result.2,
        })
    }
    /// Worker-only fresh verification; no callback or rendering runs on the writer.
    pub fn finish_derivative(
        &self,
        review: DerivativeReview,
        auth: &mut dyn Authenticator,
        confirmed: bool,
    ) -> Result<DerivativeOutput> {
        if !Arc::ptr_eq(&self.exports, &review.lease.scope) {
            return Err(Error::Denied);
        }
        review.summary()?;
        let worker = review.lease.begin()?;
        if !confirmed {
            return Err(Error::ReviewRequired);
        }
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| review.stamp.verify(auth)))
            .unwrap_or(Err(Error::WorkerFailed))?;
        review.control.check()?;
        review.lease.check()?;
        let bytes = super::recognition::charge(&review.stamp)?;
        let (tx, rx) = mpsc::channel();
        self.mailbox.admit(
            Message::NativeExport(Box::new(ExportRequest::Validate {
                stamp: review.stamp,
                authority: review.authority.clone(),
                reply: tx,
            })),
            false,
            bytes,
        )?;
        rx.recv().map_err(|_| Error::WorkerFailed)??;
        review.control.check()?;
        review.authority.check()?;
        review.lease.check()?;
        drop(worker);
        Ok(review.output)
    }
}
