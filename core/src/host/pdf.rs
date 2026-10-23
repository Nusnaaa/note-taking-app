//! Private, bounded host capabilities. Neither a read lease nor a revision review
//! can be manufactured by deserialising an ordinary request.
use crate::{
    security::Access,
    sources::{
        AssetClass, Availability, PdfSelectionPlan, PdfSelectionTarget, PdfView, RetentionChoice,
        RevisionReview, Source, SourceAsset,
    },
    store::Store,
    Error, Id, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum PdfCommand {
    Assets {
        source: Id,
    },
    BeginRetention {
        source: Id,
        class: AssetClass,
        authority: Option<String>,
        export_allowed: bool,
        consented: bool,
        length: u64,
    },
    AppendRetention {
        transfer: Id,
        offset: u64,
        bytes: Vec<u8>,
    },
    FinishRetention {
        transfer: Id,
        confirmed: bool,
    },
    ReleaseRetention {
        transfer: Id,
    },
    RevokeRetention {
        asset: Id,
    },
    PrepareAssetRead {
        asset: Id,
    },
    ReadAssetChunk {
        lease: Id,
        offset: u64,
        length: u32,
    },
    ValidateAssetRead {
        lease: Id,
        observed_digest: String,
    },
    ReleaseSelection {
        review: Id,
    },
    Source {
        source: Id,
    },
    Link {
        context: Id,
        locator: String,
        revision: String,
    },
    SetView {
        source: Id,
        view: PdfView,
    },
    MarkMissing {
        source: Id,
    },
    Revoke {
        source: Id,
    },
    Anchor {
        anchor: Id,
    },
    CreateAnchor {
        source: Id,
        block: Id,
        page: u32,
        bounds: crate::objects::Rect,
        pinned: bool,
    },
    ReviewRevision {
        source: Id,
        locator: String,
        revision: String,
    },
    ReplaceRevision {
        review: Id,
        confirmed: bool,
    },
    ReconnectAnchor {
        anchor: Id,
        page: u32,
        bounds: crate::objects::Rect,
        confirmed: bool,
    },
    PrepareRead {
        source: Id,
    },
    ValidateRead {
        lease: Id,
        observed_revision: String,
    },
    ReleaseRead {
        lease: Id,
    },
}
impl PdfCommand {
    pub(super) fn is_query(&self) -> bool {
        matches!(
            self,
            Self::Assets { .. }
                | Self::PrepareAssetRead { .. }
                | Self::ReadAssetChunk { .. }
                | Self::ValidateAssetRead { .. }
                | Self::Source { .. }
                | Self::Anchor { .. }
                | Self::PrepareRead { .. }
                | Self::ValidateRead { .. }
                | Self::ReviewRevision { .. }
        )
    }
}
struct ReadLease {
    source: Id,
    context: Id,
    locator: String,
    revision: String,
    created: Instant,
}
struct AssetReadLease {
    info: SourceAsset,
    created: Instant,
}
struct RetentionTransfer {
    source: Source,
    class: AssetClass,
    authority: Option<String>,
    export_allowed: bool,
    length: u64,
    bytes: Vec<u8>,
    created: Instant,
}
impl RetentionTransfer {
    fn check(&self, s: &Store, a: &Access) -> Result<()> {
        let now = s.source(self.source.id, a)?;
        if now.context != self.source.context
            || now.locator != self.source.locator
            || now.revision != self.source.revision
            || now.availability != Availability::Available
        {
            return Err(Error::Conflict);
        }
        Ok(())
    }
}
pub trait PdfGrantVerifier: Send {
    /// Trusted application code verifies a fresh, immutable provider observation.
    /// This callback must be bounded host bookkeeping, never provider I/O.
    fn verify(
        &mut self,
        challenge: Id,
        target: &PdfSelectionTarget,
        locator: &str,
        revision: &str,
    ) -> bool;
}
struct Selection {
    plan: PdfSelectionPlan,
    created: Instant,
}
struct Review {
    plan: RevisionReview,
    created: Instant,
}
#[derive(Default)]
pub(super) struct PdfState {
    reads: BTreeMap<Id, ReadLease>,
    asset_reads: BTreeMap<Id, AssetReadLease>,
    transfers: BTreeMap<Id, RetentionTransfer>,
    reviews: BTreeMap<Id, Review>,
    selections: BTreeMap<Id, Selection>,
}
const CAPACITY: usize = 32;
const LIFETIME: Duration = Duration::from_secs(300);
fn validity_ms(created: Instant, access: &Access) -> u64 {
    let remaining = LIFETIME.saturating_sub(created.elapsed());
    // Never turn an authentication expiry racing the source check into a fresh
    // public lifetime. The host normalises already-expired access before requests.
    let remaining = access.remaining().map_or(remaining, |d| remaining.min(d));
    remaining.as_millis().min(u64::MAX as u128) as u64
}
impl PdfState {
    pub fn clear(&mut self) {
        self.reads.clear();
        self.asset_reads.clear();
        self.transfers.clear();
        self.reviews.clear();
        self.selections.clear();
    }
    fn retire_expired(&mut self) {
        self.asset_reads
            .retain(|_, v| v.created.elapsed() < LIFETIME);
        self.transfers.retain(|_, v| v.created.elapsed() < LIFETIME);
        self.selections
            .retain(|_, plan| plan.created.elapsed() < LIFETIME);
        self.reads
            .retain(|_, plan| plan.created.elapsed() < LIFETIME);
        self.reviews
            .retain(|_, plan| plan.created.elapsed() < LIFETIME);
    }
    pub fn prepare_selection(
        &mut self,
        s: &Store,
        a: &Access,
        target: PdfSelectionTarget,
        locator: &str,
        revision: &str,
        verifier: &mut dyn PdfGrantVerifier,
    ) -> Result<Value> {
        self.retire_expired();
        if self.selections.len() >= CAPACITY {
            return Err(Error::ResourceLimit);
        }
        let plan = s.prepare_pdf_selection(target, locator, revision, a)?;
        if !verifier.verify(
            Id::new(),
            plan.target(),
            &plan.proposed().locator,
            &plan.proposed().revision,
        ) {
            return Err(Error::Denied);
        }
        s.check_pdf_selection(&plan, a)?;
        let token = Id::new();
        let result = json!({"review":token,"source":plan.proposed().id,"changed_revision":plan.changed_revision(),"affected_anchors":plan.affected_anchors});
        self.selections.insert(
            token,
            Selection {
                plan,
                created: Instant::now(),
            },
        );
        Ok(result)
    }
    pub fn finish_selection(
        &mut self,
        s: &mut Store,
        a: &Access,
        review: Id,
        confirmed: bool,
        verifier: &mut dyn PdfGrantVerifier,
    ) -> Result<Value> {
        self.retire_expired();
        let selection = self.selections.remove(&review).ok_or(Error::Unavailable)?;
        if !confirmed {
            return Err(Error::ReviewRequired);
        }
        s.check_pdf_selection(&selection.plan, a)?;
        if !verifier.verify(
            Id::new(),
            selection.plan.target(),
            &selection.plan.proposed().locator,
            &selection.plan.proposed().revision,
        ) {
            return Err(Error::Denied);
        }
        let source = s.finish_pdf_selection(selection.plan, a)?;
        self.clear();
        Ok(serde_json::to_value(source)?)
    }
    pub fn execute(&mut self, s: &mut Store, a: &Access, command: PdfCommand) -> Result<Value> {
        self.retire_expired();
        Ok(match command {
            PdfCommand::Assets { source } => serde_json::to_value(s.pdf_assets(source, a)?)?,
            PdfCommand::BeginRetention {
                source,
                class,
                authority,
                export_allowed,
                consented,
                length,
            } => {
                if !consented
                    || class == AssetClass::LiveCache
                    || authority
                        .as_ref()
                        .is_some_and(|v| v.trim().is_empty() || v.len() > 4096)
                    || (class == AssetClass::HistoricalSnapshot || export_allowed)
                        && authority.is_none()
                {
                    return Err(Error::Denied);
                }
                if !self.transfers.is_empty() || !(5..=64 * 1024 * 1024).contains(&length) {
                    return Err(Error::ResourceLimit);
                }
                let source = s.source(source, a)?;
                if source.availability != Availability::Available {
                    return Err(Error::Denied);
                }
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(length as usize)
                    .map_err(|_| Error::ResourceLimit)?;
                let token = Id::new();
                self.transfers.insert(
                    token,
                    RetentionTransfer {
                        source,
                        class,
                        authority,
                        export_allowed,
                        length,
                        bytes,
                        created: Instant::now(),
                    },
                );
                json!({"transfer":token,"maximum_chunk":65536})
            }
            PdfCommand::AppendRetention {
                transfer,
                offset,
                bytes,
            } => {
                let result = (|| -> Result<Value> {
                    let plan = self
                        .transfers
                        .get_mut(&transfer)
                        .ok_or(Error::Unavailable)?;
                    plan.check(s, a)?;
                    if bytes.is_empty()
                        || bytes.len() > 65536
                        || offset != plan.bytes.len() as u64
                        || offset + bytes.len() as u64 > plan.length
                    {
                        return Err(Error::Invalid("PDF transfer chunk"));
                    }
                    plan.bytes.extend_from_slice(&bytes);
                    Ok(json!({"offset":plan.bytes.len()}))
                })();
                if result.is_err() {
                    self.transfers.remove(&transfer);
                }
                result?
            }
            PdfCommand::FinishRetention {
                transfer,
                confirmed,
            } => {
                let plan = self.transfers.remove(&transfer).ok_or(Error::Unavailable)?;
                if !confirmed {
                    return Err(Error::ReviewRequired);
                }
                plan.check(s, a)?;
                if plan.bytes.len() as u64 != plan.length
                    || !plan.bytes.starts_with(b"%PDF-")
                    || !crate::digest(&plan.bytes).eq_ignore_ascii_case(&plan.source.revision)
                {
                    return Err(Error::Integrity);
                }
                let asset = s.retain_source_bytes(
                    plan.source.id,
                    &plan.bytes,
                    RetentionChoice {
                        class: plan.class,
                        authority: plan.authority.as_deref(),
                        export_allowed: plan.export_allowed,
                        consented: true,
                    },
                    a,
                )?;
                serde_json::to_value(s.pdf_asset(asset, a)?)?
            }
            PdfCommand::ReleaseRetention { transfer } => {
                self.transfers.remove(&transfer);
                Value::Null
            }
            PdfCommand::RevokeRetention { asset } => {
                self.clear();
                s.revoke_retention(asset, a)?;
                Value::Null
            }
            PdfCommand::PrepareAssetRead { asset } => {
                if self.reads.len() + self.asset_reads.len() >= CAPACITY {
                    return Err(Error::ResourceLimit);
                }
                let info = s.pdf_asset(asset, a)?;
                let lease = Id::new();
                let created = Instant::now();
                let value =
                    json!({"lease":lease,"asset":info,"valid_for_ms":validity_ms(created,a)});
                self.asset_reads
                    .insert(lease, AssetReadLease { info, created });
                value
            }
            PdfCommand::ReadAssetChunk {
                lease,
                offset,
                length,
            } => {
                let result = (|| -> Result<Value> {
                    let plan = self.asset_reads.get(&lease).ok_or(Error::Unavailable)?;
                    if s.pdf_asset(plan.info.id, a)? != plan.info {
                        return Err(Error::Conflict);
                    }
                    let bytes = s.pdf_asset_chunk(plan.info.id, offset, length, a)?;
                    Ok(json!({"offset":offset,"bytes":bytes}))
                })();
                if result.is_err() {
                    self.asset_reads.remove(&lease);
                }
                result?
            }
            PdfCommand::ValidateAssetRead {
                lease,
                observed_digest,
            } => {
                let result = (|| -> Result<Value> {
                    let plan = self.asset_reads.get(&lease).ok_or(Error::Unavailable)?;
                    if s.pdf_asset(plan.info.id, a)? != plan.info {
                        return Err(Error::Conflict);
                    }
                    if !observed_digest.eq_ignore_ascii_case(&plan.info.digest) {
                        return Err(Error::Integrity);
                    }
                    Ok(json!({"valid_for_ms":validity_ms(plan.created,a)}))
                })();
                if result.is_err() {
                    self.asset_reads.remove(&lease);
                }
                result?
            }
            PdfCommand::ReleaseSelection { review } => {
                self.selections.remove(&review);
                Value::Null
            }
            PdfCommand::Source { source } => serde_json::to_value(s.source(source, a)?)?,
            PdfCommand::Link {
                context,
                locator,
                revision,
            } => serde_json::to_value(s.link_pdf(context, &locator, &revision, a)?)?,
            PdfCommand::SetView { source, view } => {
                s.set_pdf_view(source, view, a)?;
                Value::Null
            }
            PdfCommand::MarkMissing { source } => {
                self.clear();
                s.mark_source_missing(source, a)?;
                Value::Null
            }
            PdfCommand::Revoke { source } => {
                self.clear();
                s.revoke_source(source, a)?;
                Value::Null
            }
            PdfCommand::Anchor { anchor } => serde_json::to_value(s.pdf_anchor(anchor, a)?)?,
            PdfCommand::CreateAnchor {
                source,
                block,
                page,
                bounds,
                pinned,
            } => serde_json::to_value(s.anchor_pdf(source, block, page, bounds, pinned, a)?)?,
            PdfCommand::ReconnectAnchor {
                anchor,
                page,
                bounds,
                confirmed,
            } => serde_json::to_value(s.reconnect_pdf_anchor(anchor, page, bounds, confirmed, a)?)?,
            PdfCommand::ReviewRevision {
                source,
                locator,
                revision,
            } => {
                if self.reviews.len() >= CAPACITY {
                    return Err(Error::ResourceLimit);
                }
                let plan = s.review_pdf_revision(source, &locator, &revision, a)?;
                let affected = plan.affected_anchors;
                let token = Id::new();
                self.reviews.insert(
                    token,
                    Review {
                        plan,
                        created: Instant::now(),
                    },
                );
                json!({"review":token,"affected_anchors":affected})
            }
            PdfCommand::ReplaceRevision { review, confirmed } => {
                // Even a failed/rejected acceptance consumes the prepared review.
                let review = self.reviews.remove(&review).ok_or(Error::Unavailable)?;
                self.reads.clear();
                self.asset_reads.clear();
                self.transfers.clear();
                s.replace_pdf_revision(review.plan, confirmed, a)?;
                self.reviews.clear();
                Value::Null
            }
            PdfCommand::PrepareRead { source } => {
                if self.reads.len() + self.asset_reads.len() >= CAPACITY {
                    return Err(Error::ResourceLimit);
                }
                let source = s.source(source, a)?;
                if source.availability != Availability::Available {
                    return Err(Error::Denied);
                }
                let lease = Id::new();
                let result = json!({"lease":lease,"source":source.id,"context":source.context,"locator":source.locator,"revision":source.revision,"valid_for_ms":validity_ms(Instant::now(), a)});
                self.reads.insert(
                    lease,
                    ReadLease {
                        source: source.id,
                        context: source.context,
                        locator: source.locator,
                        revision: source.revision,
                        created: Instant::now(),
                    },
                );
                result
            }
            PdfCommand::ValidateRead {
                lease,
                observed_revision,
            } => {
                let result = (|| -> Result<Value> {
                    let plan = self.reads.get(&lease).ok_or(Error::Unavailable)?;
                    let current = s.source(plan.source, a)?;
                    if current.availability != Availability::Available {
                        return Err(Error::Denied);
                    }
                    if current.context != plan.context
                        || current.locator != plan.locator
                        || current.revision != plan.revision
                    {
                        return Err(Error::Conflict);
                    }
                    if !observed_revision.eq_ignore_ascii_case(&plan.revision) {
                        return Err(Error::ReviewRequired);
                    }
                    Ok(json!({"valid_for_ms":validity_ms(plan.created, a)}))
                })();
                if result.is_err() {
                    self.reads.remove(&lease);
                }
                result?
            }
            PdfCommand::ReleaseRead { lease } => {
                self.reads.remove(&lease);
                self.asset_reads.remove(&lease);
                Value::Null
            }
        })
    }
}
