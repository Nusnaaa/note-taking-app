//! Private, bounded host capabilities. Neither a read lease nor a revision review
//! can be manufactured by deserialising an ordinary request.
use crate::{
    security::Access,
    sources::{Availability, PdfView, RevisionReview},
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
            Self::Source { .. }
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
struct Review {
    plan: RevisionReview,
    created: Instant,
}
#[derive(Default)]
pub(super) struct PdfState {
    reads: BTreeMap<Id, ReadLease>,
    reviews: BTreeMap<Id, Review>,
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
        self.reviews.clear();
    }
    fn retire_expired(&mut self) {
        self.reads
            .retain(|_, plan| plan.created.elapsed() < LIFETIME);
        self.reviews
            .retain(|_, plan| plan.created.elapsed() < LIFETIME);
    }
    pub fn execute(&mut self, s: &mut Store, a: &Access, command: PdfCommand) -> Result<Value> {
        self.retire_expired();
        Ok(match command {
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
                s.replace_pdf_revision(review.plan, confirmed, a)?;
                self.reviews.clear();
                Value::Null
            }
            PdfCommand::PrepareRead { source } => {
                if self.reads.len() >= CAPACITY {
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
                Value::Null
            }
        })
    }
}
