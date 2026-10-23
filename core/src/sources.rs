//! Source links do not fetch data or grant access. Only the trusted platform host
//! supplies bytes after a deliberate document-provider grant. Authorised snapshots
//! are distinct from disposable live-source caches (owner Q383).
use crate::{digest, now_ms, security::Access, store::Store, Error, Id, Result};

use rusqlite::{Connection, OptionalExtension};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Availability {
    Available,
    Missing,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Acquisition {
    ExternalLink,
    ManagedCopy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfView {
    pub page: u32,
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
    pub divider: f64,
}

impl Default for PdfView {
    fn default() -> Self {
        Self {
            page: 0,
            zoom: 1.,
            pan_x: 0.,
            pan_y: 0.,
            divider: 0.5,
        }
    }
}

impl PdfView {
    pub(crate) fn validate(&self) -> Result<()> {
        if !self.zoom.is_finite()
            || self.zoom <= 0.
            || !self.pan_x.is_finite()
            || !self.pan_y.is_finite()
            || !self.divider.is_finite()
            || self.divider <= 0.
            || self.divider >= 1.
        {
            return Err(Error::Invalid("PDF view"));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: Id,
    pub context: Id,
    pub locator: String,
    pub revision: String,
    pub availability: Availability,
    pub acquisition: Acquisition,
    pub previous_revisions: Vec<String>,
    pub view: PdfView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetClass {
    LiveCache,
    ManagedCopy,
    HistoricalSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retention {
    pub source_revision: String,
    pub captured_ms: i64,
    /// Independent continuing authorisation, not an inference from past access.
    pub authority: String,
    pub export_allowed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceBytes {
    pub bytes: Vec<u8>,
    pub historical: bool,
    pub availability: Availability,
    pub source_revision: String,
}

/// Bounded metadata; bytes are read independently through checked chunks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceAsset {
    pub id: Id,
    pub source: Id,
    pub class: AssetClass,
    pub retention: Option<Retention>,
    pub length: u64,
    pub digest: String,
    pub source_revision: String,
}

/// Explicit, reviewable retention decision supplied by the trusted host.
pub struct RetentionChoice<'a> {
    pub class: AssetClass,
    pub authority: Option<&'a str>,
    pub export_allowed: bool,
    pub consented: bool,
}

impl Store {
    pub(crate) fn raw_source(db: &Connection, id: Id) -> Result<Source> {
        let json: String = db
            .query_row(
                "SELECT payload FROM sources WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;

        Ok(serde_json::from_str(&json)?)
    }

    pub(crate) fn read_source(db: &Connection, id: Id, access: &Access) -> Result<Source> {
        let s = Self::raw_source(db, id)?;

        Self::authorise(db, s.context, access, false)?;

        Ok(s)
    }

    pub(crate) fn save_source(db: &Connection, s: &Source) -> Result<()> {
        db.execute(
            "UPDATE sources SET payload=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_string(s)?, s.id.to_string()],
        )?;

        Ok(())
    }

    pub fn source(&self, id: Id, access: &Access) -> Result<Source> {
        Self::read_source(&self.db, id, access)
    }

    pub fn link_pdf(
        &mut self,
        context: Id,
        locator: &str,
        revision: &str,
        access: &Access,
    ) -> Result<Source> {
        if locator.is_empty()
            || locator.len() > 4096
            || (!locator.starts_with("content://") && !locator.starts_with("file://"))
        {
            return Err(Error::Invalid("local PDF locator"));
        }

        if revision.len() != 64 || !revision.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::Invalid("source digest"));
        }

        let s = Source {
            id: Id::new(),
            context,
            locator: locator.into(),
            revision: revision.to_ascii_lowercase(),
            availability: Availability::Available,
            acquisition: Acquisition::ExternalLink,
            previous_revisions: vec![],
            view: PdfView::default(),
        };

        self.write(|db| {
            Self::authorise(db, context, access, false)?;

            db.execute(
                "INSERT INTO sources VALUES(?1,?2,?3)",
                rusqlite::params![
                    s.id.to_string(),
                    context.to_string(),
                    serde_json::to_string(&s)?
                ],
            )?;

            Ok(s.clone())
        })
    }

    pub fn set_pdf_view(&mut self, id: Id, view: PdfView, access: &Access) -> Result<()> {
        view.validate()?;

        self.write(|db| {
            let mut s = Self::read_source(db, id, access)?;

            s.view = view;

            Self::save_source(db, &s)
        })
    }

    pub fn mark_source_missing(&mut self, id: Id, access: &Access) -> Result<()> {
        self.write(|db| {
            let mut s = Self::read_source(db, id, access)?;

            if s.availability == Availability::Revoked {
                return Err(Error::Denied);
            }

            s.availability = Availability::Missing;

            Self::save_source(db, &s)
        })
    }

    pub fn relink_pdf(
        &mut self,
        id: Id,
        locator: &str,
        revision: &str,
        confirmed: bool,
        access: &Access,
    ) -> Result<()> {
        if !confirmed {
            return Err(Error::ReviewRequired);
        }

        if !locator.starts_with("content://") && !locator.starts_with("file://") {
            return Err(Error::Invalid("local PDF locator"));
        }

        self.write(|db| {
            let mut s = Self::read_source(db, id, access)?;

            if s.availability == Availability::Revoked {
                return Err(Error::Denied);
            }

            if revision != s.revision {
                return Err(Error::ReviewRequired);
            }

            s.locator = locator.into();

            s.availability = Availability::Available;

            Self::save_source(db, &s)
        })
    }

    pub fn retain_source_bytes(
        &mut self,
        id: Id,
        bytes: &[u8],
        choice: RetentionChoice<'_>,
        access: &Access,
    ) -> Result<Id> {
        let RetentionChoice {
            class,
            authority,
            export_allowed,
            consented,
        } = choice;

        if !consented {
            return Err(Error::Denied);
        }

        if bytes.len() > 64 * 1024 * 1024 {
            return Err(Error::ResourceLimit);
        }

        if class == AssetClass::HistoricalSnapshot && authority.is_none_or(str::is_empty) {
            return Err(Error::Denied);
        }

        if authority.is_some_and(|v| v.len() > 4096 || (!v.is_empty() && v.trim().is_empty())) {
            return Err(Error::Invalid("retention authority"));
        }

        self.write(|db| {
            let mut s = Self::read_source(db, id, access)?;

            if s.availability != Availability::Available {
                return Err(Error::Denied);
            }

            if class == AssetClass::ManagedCopy && digest(bytes) != s.revision {
                return Err(Error::Integrity);
            }

            if class == AssetClass::ManagedCopy && !bytes.starts_with(b"%PDF-") {
                return Err(Error::Invalid("PDF signature"));
            }

            let retention = authority.filter(|v| !v.is_empty()).map(|a| Retention {
                source_revision: s.revision.clone(),
                captured_ms: now_ms(),
                authority: a.into(),
                export_allowed,
            });

            if class == AssetClass::LiveCache && retention.is_some() {
                return Err(Error::Invalid("cache retention exception"));
            }

            let asset = Id::new();

            db.execute(
                "INSERT INTO source_assets VALUES(?1,?2,?3,?4,?5,?6)",
                rusqlite::params![
                    asset.to_string(),
                    id.to_string(),
                    serde_json::to_string(&class)?,
                    retention.as_ref().map(serde_json::to_string).transpose()?,
                    bytes,
                    digest(bytes)
                ],
            )?;

            if class == AssetClass::ManagedCopy {
                s.acquisition = Acquisition::ManagedCopy;

                Self::save_source(db, &s)?;
            }

            Ok(asset)
        })
    }

    /// Lists retained PDF assets only. Disposable previews are never a PDF read source.
    pub fn pdf_assets(&self, source: Id, access: &Access) -> Result<Vec<SourceAsset>> {
        Self::read_source(&self.db, source, access)?;
        let mut statement = self
            .db
            .prepare("SELECT id FROM source_assets WHERE source=?1 ORDER BY id LIMIT 257")?;
        let ids = statement
            .query_map([source.to_string()], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if ids.len() > 256 {
            return Err(Error::ResourceLimit);
        }
        let mut result = Vec::new();
        for id in ids {
            match self.pdf_asset(Id::parse(&id)?, access) {
                Ok(asset) => result.push(asset),
                Err(Error::Denied | Error::Unsupported) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(result)
    }
    pub fn pdf_asset(&self, asset: Id, access: &Access) -> Result<SourceAsset> {
        let (source, class, authority, length, hash, prefix): (String, String, Option<String>, i64, String, Vec<u8>) = self.db.query_row(
            "SELECT source,class,authority,length(bytes),digest,substr(bytes,1,5) FROM source_assets WHERE id=?1",
            [asset.to_string()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))
            .optional()?.ok_or(Error::Unavailable)?;
        let source = Self::read_source(&self.db, Id::parse(&source)?, access)?;
        let class: AssetClass = serde_json::from_str(&class)?;
        let retention: Option<Retention> =
            authority.as_deref().map(serde_json::from_str).transpose()?;
        if class == AssetClass::LiveCache {
            return Err(Error::Unsupported);
        }
        if source.availability == Availability::Revoked && retention.is_none() {
            return Err(Error::Denied);
        }
        if class == AssetClass::HistoricalSnapshot && retention.is_none() {
            return Err(Error::Denied);
        }
        if retention
            .as_ref()
            .is_some_and(|r| r.authority.trim().is_empty() || r.authority.len() > 4096)
        {
            return Err(Error::Denied);
        }
        if prefix != b"%PDF-" {
            return Err(Error::Unsupported);
        }
        if !(5..=64 * 1024 * 1024).contains(&length)
            || hash.len() != 64
            || !hash.bytes().all(|v| v.is_ascii_hexdigit())
        {
            return Err(Error::Integrity);
        }
        let revision = retention
            .as_ref()
            .map(|r| r.source_revision.clone())
            .unwrap_or_else(|| source.revision.clone());
        if retention.is_none() && !hash.eq_ignore_ascii_case(&source.revision) {
            return Err(Error::ReviewRequired);
        }
        Ok(SourceAsset {
            id: asset,
            source: source.id,
            class,
            retention,
            length: length as u64,
            digest: hash,
            source_revision: revision,
        })
    }
    pub fn pdf_asset_chunk(
        &self,
        asset: Id,
        offset: u64,
        length: u32,
        access: &Access,
    ) -> Result<Vec<u8>> {
        if length == 0 || length > 64 * 1024 {
            return Err(Error::ResourceLimit);
        }
        let info = self.pdf_asset(asset, access)?;
        if offset > info.length {
            return Err(Error::Invalid("PDF asset offset"));
        }
        let bytes = self.db.query_row(
            "SELECT substr(bytes,?2,?3) FROM source_assets WHERE id=?1",
            rusqlite::params![asset.to_string(), offset as i64 + 1, length],
            |r| r.get(0),
        )?;
        Ok(bytes)
    }

    pub fn revoke_source(&mut self, id: Id, access: &Access) -> Result<()> {
        self.write(|db| {
            let mut s = Self::read_source(db, id, access)?;

            s.availability = Availability::Revoked;

            Self::save_source(db, &s)?;

            db.execute(
                "DELETE FROM source_assets WHERE source=?1 AND authority IS NULL",
                [id.to_string()],
            )?;

            Ok(())
        })
    }

    pub fn revoke_retention(&mut self, asset: Id, access: &Access) -> Result<()> {
        self.write(|db| {
            let source: String = db
                .query_row(
                    "SELECT source FROM source_assets WHERE id=?1",
                    [asset.to_string()],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or(Error::Unavailable)?;

            Self::read_source(db, Id::parse(&source)?, access)?;

            db.execute("DELETE FROM source_assets WHERE id=?1", [asset.to_string()])?;

            Ok(())
        })
    }

    pub fn source_bytes(
        &self,
        asset: Id,
        for_export: bool,
        access: &Access,
    ) -> Result<SourceBytes> {
        let (source, authority, bytes, hash): (String, Option<String>, Vec<u8>, String) = self
            .db
            .query_row(
                "SELECT source,authority,bytes,digest FROM source_assets WHERE id=?1",
                [asset.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;

        let s = Self::read_source(&self.db, Id::parse(&source)?, access)?;

        let retention: Option<Retention> =
            authority.as_deref().map(serde_json::from_str).transpose()?;

        if s.availability == Availability::Revoked && retention.is_none() {
            return Err(Error::Denied);
        }

        if for_export && retention.as_ref().is_none_or(|r| !r.export_allowed) {
            return Err(Error::Denied);
        }

        if digest(&bytes) != hash {
            return Err(Error::Integrity);
        }

        Ok(SourceBytes {
            bytes,
            historical: retention.is_some(),
            availability: s.availability,
            source_revision: retention.map(|r| r.source_revision).unwrap_or(s.revision),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnchorHealth {
    Current,
    NeedsReview,
    Historical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfAnchor {
    pub id: Id,
    pub source: Id,
    pub block: Id,
    pub source_revision: String,
    pub page: u32,
    pub bounds: crate::objects::Rect,
    pub pinned: bool,
    pub health: AnchorHealth,
}

pub struct RevisionReview {
    generation: String,
    source: Id,
    old_revision: String,
    new_revision: String,
    locator: String,
    pub affected_anchors: usize,
}

impl Store {
    pub fn anchor_pdf(
        &mut self,
        source: Id,
        block: Id,
        page: u32,
        bounds: crate::objects::Rect,
        pinned: bool,
        access: &Access,
    ) -> Result<PdfAnchor> {
        bounds.validate()?;

        if bounds.x < 0.
            || bounds.y < 0.
            || bounds.x + bounds.width > 1.
            || bounds.y + bounds.height > 1.
        {
            return Err(Error::Invalid("normalised PDF anchor"));
        }

        let context = self.source(source, access)?.context;

        self.write_for(context, |db| {
            let s = Self::read_source(db, source, access)?;

            if s.availability != Availability::Available {
                return Err(Error::Denied);
            }

            if Self::read_block(db, block, access)?.entity != s.context {
                return Err(Error::Invalid("annotation context"));
            }

            let a = PdfAnchor {
                id: Id::new(),
                source,
                block,
                source_revision: s.revision,
                page,
                bounds,
                pinned,
                health: AnchorHealth::Current,
            };

            db.execute(
                "INSERT INTO pdf_anchors VALUES(?1,?2,?3,?4)",
                rusqlite::params![
                    a.id.to_string(),
                    source.to_string(),
                    block.to_string(),
                    serde_json::to_string(&a)?
                ],
            )?;

            Ok(a)
        })
    }

    pub fn pdf_anchor(&self, id: Id, access: &Access) -> Result<PdfAnchor> {
        let json: String = self
            .db
            .query_row(
                "SELECT payload FROM pdf_anchors WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;

        let a: PdfAnchor = serde_json::from_str(&json)?;

        Self::read_source(&self.db, a.source, access)?;

        Self::read_block(&self.db, a.block, access)?;

        Ok(a)
    }

    pub fn review_pdf_revision(
        &self,
        id: Id,
        locator: &str,
        new_revision: &str,
        access: &Access,
    ) -> Result<RevisionReview> {
        if (!locator.starts_with("content://") && !locator.starts_with("file://"))
            || locator.len() > 4096
            || new_revision.len() != 64
            || !new_revision.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(Error::Invalid("PDF revision"));
        }

        let generation = Self::setting(&self.db, "generation")?;

        let s = self.source(id, access)?;

        if s.availability == Availability::Revoked {
            return Err(Error::Denied);
        }

        if new_revision.eq_ignore_ascii_case(&s.revision) {
            return Err(Error::Invalid("unchanged revision"));
        }

        let count = self.db.query_row(
            "SELECT COUNT(*) FROM pdf_anchors WHERE source=?1",
            [id.to_string()],
            |r| r.get::<_, usize>(0),
        )?;

        if Self::setting(&self.db, "generation")? != generation {
            return Err(Error::Conflict);
        }

        Ok(RevisionReview {
            generation,
            source: id,
            old_revision: s.revision,
            new_revision: new_revision.to_ascii_lowercase(),
            locator: locator.into(),
            affected_anchors: count,
        })
    }

    pub fn replace_pdf_revision(
        &mut self,
        review: RevisionReview,
        confirmed: bool,
        access: &Access,
    ) -> Result<()> {
        if !confirmed {
            return Err(Error::ReviewRequired);
        }

        let context = self.source(review.source, access)?.context;

        self.write_for(context, |db| {
            if Self::setting(db, "generation")? != review.generation {
                return Err(Error::Conflict);
            }

            let mut s = Self::read_source(db, review.source, access)?;

            if s.revision != review.old_revision || s.availability == Availability::Revoked {
                return Err(Error::Conflict);
            }

            let mut q = db.prepare("SELECT payload FROM pdf_anchors WHERE source=?1")?;

            let rows = q
                .query_map([s.id.to_string()], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            for row in rows {
                let mut a: PdfAnchor = serde_json::from_str(&row)?;

                Self::read_block(db, a.block, access)?;

                a.health = if a.pinned {
                    AnchorHealth::Historical
                } else {
                    AnchorHealth::NeedsReview
                };

                db.execute(
                    "UPDATE pdf_anchors SET payload=?1 WHERE id=?2",
                    rusqlite::params![serde_json::to_string(&a)?, a.id.to_string()],
                )?;
            }

            s.previous_revisions.push(s.revision.clone());

            s.revision = review.new_revision;

            s.locator = review.locator;

            s.availability = Availability::Available;

            s.acquisition = Acquisition::ExternalLink;

            // Disposable prior-revision bytes cannot masquerade as the new source.
            db.execute(
                "DELETE FROM source_assets WHERE source=?1 AND authority IS NULL",
                [s.id.to_string()],
            )?;

            Self::save_source(db, &s)
        })
    }

    pub fn reconnect_pdf_anchor(
        &mut self,
        id: Id,
        page: u32,
        bounds: crate::objects::Rect,
        confirmed: bool,
        access: &Access,
    ) -> Result<PdfAnchor> {
        if !confirmed {
            return Err(Error::ReviewRequired);
        }

        bounds.validate()?;

        if bounds.x < 0.
            || bounds.y < 0.
            || bounds.x + bounds.width > 1.
            || bounds.y + bounds.height > 1.
        {
            return Err(Error::Invalid("normalised PDF anchor"));
        }

        let a = self.pdf_anchor(id, access)?;

        let context = self.source(a.source, access)?.context;

        self.write_for(context, |db| {
            let json: String = db.query_row(
                "SELECT payload FROM pdf_anchors WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )?;

            let mut a: PdfAnchor = serde_json::from_str(&json)?;

            let source = Self::read_source(db, a.source, access)?;

            Self::read_block(db, a.block, access)?;

            if source.availability != Availability::Available {
                return Err(Error::Denied);
            }

            a.source_revision = source.revision;

            a.page = page;

            a.bounds = bounds;

            a.health = AnchorHealth::Current;

            db.execute(
                "UPDATE pdf_anchors SET payload=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&a)?, id.to_string()],
            )?;

            Ok(a)
        })
    }

    pub fn reacquire_pdf(
        &mut self,
        id: Id,
        locator: &str,
        revision: &str,
        new_platform_grant: bool,
        access: &Access,
    ) -> Result<()> {
        if !new_platform_grant {
            return Err(Error::Denied);
        }

        if (!locator.starts_with("content://") && !locator.starts_with("file://"))
            || locator.len() > 4096
        {
            return Err(Error::Invalid("PDF locator"));
        }

        let context = self.source(id, access)?.context;

        self.write_for(context, |db| {
            let mut s = Self::read_source(db, id, access)?;

            if s.revision != revision {
                return Err(Error::ReviewRequired);
            }

            s.locator = locator.into();

            s.availability = Availability::Available;

            Self::save_source(db, &s)
        })
    }
}

/// Trusted host selection destination. Serialised metadata is not a grant: the
/// host separately verifies the chosen provider observation before either step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum PdfSelectionTarget {
    Link { context: Id },
    Relink { source: Id },
}

pub(crate) struct PdfSelectionPlan {
    library: String,
    generation: String,
    target: PdfSelectionTarget,
    before: Option<Source>,
    proposed: Source,
    pub affected_anchors: usize,
}
impl PdfSelectionPlan {
    pub fn target(&self) -> &PdfSelectionTarget {
        &self.target
    }
    pub fn proposed(&self) -> &Source {
        &self.proposed
    }
    pub fn changed_revision(&self) -> bool {
        self.before
            .as_ref()
            .is_some_and(|s| s.revision != self.proposed.revision)
    }
}
impl Store {
    pub(crate) fn prepare_pdf_selection(
        &self,
        target: PdfSelectionTarget,
        locator: &str,
        revision: &str,
        access: &Access,
    ) -> Result<PdfSelectionPlan> {
        if (!locator.starts_with("content://") && !locator.starts_with("file://"))
            || locator.len() > 4096
            || revision.len() != 64
            || !revision.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(Error::Invalid("PDF selection"));
        }
        let library = Self::setting(&self.db, "library_id")?;
        let generation = Self::setting(&self.db, "generation")?;
        let before = match target {
            PdfSelectionTarget::Link { context } => {
                Self::authorise(&self.db, context, access, false)?;
                None
            }
            PdfSelectionTarget::Relink { source } => Some(self.source(source, access)?),
        };
        let mut proposed = match &before {
            Some(source) => source.clone(),
            None => Source {
                id: Id::new(),
                context: match target {
                    PdfSelectionTarget::Link { context } => context,
                    _ => unreachable!(),
                },
                locator: locator.into(),
                revision: revision.to_ascii_lowercase(),
                availability: Availability::Available,
                acquisition: Acquisition::ExternalLink,
                previous_revisions: Vec::new(),
                view: PdfView::default(),
            },
        };
        let changed = before
            .as_ref()
            .is_some_and(|s| !s.revision.eq_ignore_ascii_case(revision));
        let mut affected_anchors = 0;
        if changed {
            let mut q = self
                .db
                .prepare("SELECT payload FROM pdf_anchors WHERE source=?1")?;
            for row in q.query_map([proposed.id.to_string()], |r| r.get::<_, String>(0))? {
                let anchor: PdfAnchor = serde_json::from_str(&row?)?;
                // No protected annotation count or health mutation without access.
                Self::read_block(&self.db, anchor.block, access)?;
                affected_anchors += 1;
            }
            proposed.previous_revisions.push(proposed.revision.clone());
        }
        proposed.locator = locator.into();
        proposed.revision = revision.to_ascii_lowercase();
        proposed.availability = Availability::Available;
        proposed.acquisition = Acquisition::ExternalLink;
        if Self::setting(&self.db, "generation")? != generation {
            return Err(Error::Conflict);
        }
        Ok(PdfSelectionPlan {
            library,
            generation,
            target,
            before,
            proposed,
            affected_anchors,
        })
    }
    pub(crate) fn check_pdf_selection(
        &self,
        plan: &PdfSelectionPlan,
        access: &Access,
    ) -> Result<()> {
        if Self::setting(&self.db, "library_id")? != plan.library
            || Self::setting(&self.db, "generation")? != plan.generation
        {
            return Err(Error::Conflict);
        }
        Self::authorise(&self.db, plan.proposed.context, access, false)?;
        if let Some(before) = &plan.before {
            if self.source(before.id, access)? != *before {
                return Err(Error::Conflict);
            }
        }
        Ok(())
    }
    pub(crate) fn finish_pdf_selection(
        &mut self,
        plan: PdfSelectionPlan,
        access: &Access,
    ) -> Result<Source> {
        self.check_pdf_selection(&plan, access)?;
        let context = plan.proposed.context;
        self.write_for(context, |db| {
            if Self::setting(db, "library_id")? != plan.library
                || Self::setting(db, "generation")? != plan.generation
            {
                return Err(Error::Conflict);
            }
            Self::authorise(db, context, access, false)?;
            if let Some(before) = &plan.before {
                if Self::read_source(db, before.id, access)? != *before {
                    return Err(Error::Conflict);
                }
                if plan.changed_revision() {
                    let mut q = db.prepare("SELECT payload FROM pdf_anchors WHERE source=?1")?;
                    for row in q.query_map([before.id.to_string()], |r| r.get::<_, String>(0))? {
                        let mut anchor: PdfAnchor = serde_json::from_str(&row?)?;
                        Self::read_block(db, anchor.block, access)?;
                        anchor.health = if anchor.pinned {
                            AnchorHealth::Historical
                        } else {
                            AnchorHealth::NeedsReview
                        };
                        db.execute(
                            "UPDATE pdf_anchors SET payload=?1 WHERE id=?2",
                            rusqlite::params![
                                serde_json::to_string(&anchor)?,
                                anchor.id.to_string()
                            ],
                        )?;
                    }
                    db.execute(
                        "DELETE FROM source_assets WHERE source=?1 AND authority IS NULL",
                        [before.id.to_string()],
                    )?;
                }
                Self::save_source(db, &plan.proposed)?;
            } else {
                db.execute(
                    "INSERT INTO sources VALUES(?1,?2,?3)",
                    rusqlite::params![
                        plan.proposed.id.to_string(),
                        context.to_string(),
                        serde_json::to_string(&plan.proposed)?
                    ],
                )?;
            }
            Ok(plan.proposed.clone())
        })
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    use crate::{
        content::Content,
        objects::Rect,
        store::{FailurePoint, Kind},
    };
    #[test]
    fn retained_pdf_commit_failure_does_not_publish_asset_or_change_acquisition() {
        let a = Access::public();
        let mut s = Store::memory().unwrap();
        let note = s.create(Kind::Note, None, "synthetic", &a).unwrap();
        let block = s
            .append_content(note.id, Content::Text("independent work".into()), &a)
            .unwrap();
        let bytes = b"%PDF-atomic managed copy";
        let source = s
            .link_pdf(note.id, "content://synthetic/pdf", &digest(bytes), &a)
            .unwrap();
        s.inject_failure(FailurePoint::BeforeCommit);
        assert!(s
            .retain_source_bytes(
                source.id,
                bytes,
                RetentionChoice {
                    class: AssetClass::ManagedCopy,
                    authority: None,
                    export_allowed: false,
                    consented: true
                },
                &a
            )
            .is_err());
        assert!(s.pdf_assets(source.id, &a).unwrap().is_empty());
        assert_eq!(s.source(source.id, &a).unwrap(), source);
        assert_eq!(s.block(block.id, &a).unwrap(), block);
        let asset = s
            .retain_source_bytes(
                source.id,
                bytes,
                RetentionChoice {
                    class: AssetClass::ManagedCopy,
                    authority: None,
                    export_allowed: false,
                    consented: true,
                },
                &a,
            )
            .unwrap();
        assert_eq!(s.pdf_asset_chunk(asset, 0, 65536, &a).unwrap(), bytes);
    }
    #[test]
    fn selected_revision_transaction_rolls_back_and_preserves_independent_evidence() {
        let a = Access::public();
        let mut s = Store::memory().unwrap();
        let n = s.create(Kind::Note, None, "synthetic", &a).unwrap();
        let b = s
            .append_content(n.id, Content::Text("authored observation".into()), &a)
            .unwrap();
        let old = b"%PDF-old";
        let source = s.link_pdf(n.id, "content://old", &digest(old), &a).unwrap();
        let anchor = s
            .anchor_pdf(
                source.id,
                b.id,
                2,
                Rect {
                    x: 0.,
                    y: 0.,
                    width: 0.5,
                    height: 0.5,
                },
                false,
                &a,
            )
            .unwrap();
        let evidence = s
            .retain_source_bytes(
                source.id,
                old,
                RetentionChoice {
                    class: AssetClass::HistoricalSnapshot,
                    authority: Some("independent owner authority"),
                    export_allowed: false,
                    consented: true,
                },
                &a,
            )
            .unwrap();
        s.revoke_source(source.id, &a).unwrap();
        let target = PdfSelectionTarget::Relink { source: source.id };
        let plan = s
            .prepare_pdf_selection(target.clone(), "content://new", &digest(b"%PDF-new"), &a)
            .unwrap();
        s.inject_failure(FailurePoint::BeforeCommit);
        assert!(s.finish_pdf_selection(plan, &a).is_err());
        assert_eq!(
            s.source(source.id, &a).unwrap().availability,
            Availability::Revoked
        );
        assert_eq!(
            s.pdf_anchor(anchor.id, &a).unwrap().health,
            AnchorHealth::Current
        );
        let plan = s
            .prepare_pdf_selection(target, "content://new", &digest(b"%PDF-new"), &a)
            .unwrap();
        s.finish_pdf_selection(plan, &a).unwrap();
        assert_eq!(
            s.pdf_anchor(anchor.id, &a).unwrap().health,
            AnchorHealth::NeedsReview
        );
        assert_eq!(s.source_bytes(evidence, false, &a).unwrap().bytes, old);
        assert_eq!(s.block(b.id, &a).unwrap(), b);
    }
}
