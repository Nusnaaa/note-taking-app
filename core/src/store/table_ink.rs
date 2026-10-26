//! Explicit table-cell capture. Cell IDs are local targets of global ink regions;
//! original observations remain owned by the table independently of undo.
use super::*;
use crate::{content::Content, digest, ink::Stroke};
use std::collections::{BTreeMap, BTreeSet};

impl Store {
    /// Idempotent, revision-bound capture into one existing cell. Coordinates
    /// remain the observed table source coordinates; no clipping or grid inference.
    pub fn append_cell_capture(
        &mut self,
        ticket: Id,
        block: Id,
        revision: u64,
        cell: Id,
        strokes: Vec<Stroke>,
        access: &Access,
    ) -> Result<Block> {
        self.append_cell_capture_bound(None, ticket, block, revision, cell, strokes, access)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn append_cell_capture_for(
        &mut self,
        entity: Id,
        ticket: Id,
        block: Id,
        revision: u64,
        cell: Id,
        strokes: Vec<Stroke>,
        access: &Access,
    ) -> Result<Block> {
        self.append_cell_capture_bound(Some(entity), ticket, block, revision, cell, strokes, access)
    }
    #[allow(clippy::too_many_arguments)]
    fn append_cell_capture_bound(
        &mut self,
        entity: Option<Id>,
        ticket: Id,
        block: Id,
        revision: u64,
        cell: Id,
        strokes: Vec<Stroke>,
        access: &Access,
    ) -> Result<Block> {
        if strokes.is_empty() || strokes.len() > 1000 {
            return Err(Error::Invalid("cell capture batch"));
        }
        let mut ids = BTreeSet::new();
        let mut sequence = BTreeMap::new();
        for stroke in &strokes {
            stroke.validate_persisted()?;
            if !ids.insert(stroke.id)
                || sequence
                    .insert(stroke.capture_session, stroke.sequence)
                    .is_some_and(|previous| stroke.sequence <= previous)
            {
                return Err(Error::Invalid("cell capture order"));
            }
        }
        let owner = Self::read_block(&self.db, block, access)?.entity;
        let fingerprint = digest(&serde_json::to_vec(&(block, revision, cell, &strokes))?);
        self.write_for(owner, |db| {
            let mut current = Self::read_block(db, block, access)?;
            if entity.is_some_and(|entity| current.entity != entity) {
                return Err(Error::Conflict);
            }
            let old: Option<(String, String)> = db
                .query_row(
                    "SELECT block,fingerprint FROM capture_receipts WHERE ticket=?1",
                    [ticket.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if let Some((old_block, old_hash)) = old {
                // Do not confirm another protected receipt through this target.
                let old = Self::read_block(db, Id::parse(&old_block)?, access)?;
                if old.id != block || old_hash != fingerprint {
                    return Err(Error::Conflict);
                }
                return Ok(current);
            }
            if current.revision != revision {
                return Err(Error::Conflict);
            }
            let Content::Table(table) = &current.content else {
                return Err(Error::Invalid("cell capture table"));
            };
            if !table.rows.iter().flatten().any(|entry| entry.id == cell) {
                return Err(Error::Unavailable);
            }
            if !current.visible {
                return Err(Error::Unavailable);
            }
            let mut previous = BTreeMap::new();
            for original in Self::original_strokes(db, &current)? {
                previous
                    .entry(original.capture_session)
                    .and_modify(|sequence: &mut u64| *sequence = (*sequence).max(original.sequence))
                    .or_insert(original.sequence);
            }
            for stroke in &strokes {
                if previous
                    .insert(stroke.capture_session, stroke.sequence)
                    .is_some_and(|sequence| stroke.sequence <= sequence)
                {
                    return Err(Error::Invalid("cell capture session order"));
                }
            }
            if current.source_ids.len().saturating_add(strokes.len()) > 4096 {
                return Err(Error::ResourceLimit);
            }
            if let Some(layout) = &table.layout {
                let bounds = layout.cell_bounds(table, cell)?;
                if strokes.iter().any(|s| {
                    s.coordinate_space != layout.coordinate_space
                        || s.points
                            .iter()
                            .any(|p| !(p.x - bounds.x).is_finite() || !(p.y - bounds.y).is_finite())
                }) {
                    return Err(Error::Invalid("table capture coordinate space"));
                }
                let origins = strokes
                    .iter()
                    .map(|s| crate::objects::CellInkOrigin {
                        cell,
                        stroke: s.id,
                        origin: crate::objects::Position {
                            x: bounds.x,
                            y: bounds.y,
                        },
                    })
                    .collect::<Vec<_>>();
                if let Content::Table(table) = &mut current.content {
                    table
                        .layout
                        .as_mut()
                        .ok_or(Error::Integrity)?
                        .ink_origins
                        .extend(origins);
                }
            }
            let new_ids: Vec<_> = strokes.iter().map(|stroke| stroke.id).collect();
            for stroke in &strokes {
                let payload = serde_json::to_string(stroke)?;
                db.execute(
                    "INSERT INTO strokes VALUES(?1,?2,?3,?4)",
                    rusqlite::params![
                        stroke.id.to_string(),
                        block.to_string(),
                        payload,
                        digest(payload.as_bytes())
                    ],
                )?;
            }
            current.source_ids.extend(&new_ids);
            current.visible_strokes.extend(&new_ids);
            if let Some(region) = current
                .regions
                .iter_mut()
                .find(|region| region.target() == cell)
            {
                region.strokes.extend(new_ids);
                region.content = None; // new ink awaits an independent review; prior cell text stays retained
            } else {
                current.regions.push(Region {
                    id: Id::new(),
                    cell: Some(cell),
                    strokes: new_ids,
                    content: None,
                });
            }
            current.revision += 1;
            Self::persist_block(db, &current)?;
            db.execute(
                "INSERT INTO capture_receipts VALUES(?1,?2,?3)",
                rusqlite::params![ticket.to_string(), block.to_string(), fingerprint],
            )?;
            Ok(current)
        })
    }
}

/// A derived vector placement paired with the unchanged original stroke.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellInkPlacement {
    pub cell: Id,
    pub stroke: Stroke,
    pub source_to_surface: crate::ink::Transform,
}
impl Store {
    /// Visible table ink in source order, including explicit cell reflow and the
    /// block transform. No rasterization, clipping or source rewriting.
    pub fn table_ink_scene(&self, block: Id, access: &Access) -> Result<Vec<CellInkPlacement>> {
        let block = Self::read_block(&self.db, block, access)?;
        let Content::Table(table) = &block.content else {
            return Err(Error::Invalid("table block"));
        };
        let layout = table.layout.as_ref().ok_or(Error::ReviewRequired)?;
        if !block.visible {
            return Err(Error::Unavailable);
        }
        block.validate()?;
        let _ = layout;
        let mut scene = Vec::new();
        let mut bytes = 1024usize;
        const LIMIT: usize = 8 * 1024 * 1024;
        for id in &block.source_ids {
            if !block.visible_strokes.contains(id) {
                continue;
            }
            let encoded: usize = self.db.query_row(
                "SELECT length(CAST(payload AS BLOB)) FROM strokes WHERE id=?1 AND block=?2",
                rusqlite::params![id.to_string(), block.id.to_string()],
                |r| r.get(0),
            )?;
            if encoded > LIMIT - bytes {
                return Err(Error::ResourceLimit);
            }
            let placement =
                Self::place_table_stroke(&block, Self::scene_stroke(&self.db, block.id, *id)?)?;
            let encoded = serde_json::to_vec(&placement)?.len() + 2;
            if encoded > LIMIT - bytes {
                return Err(Error::ResourceLimit);
            }
            bytes += encoded;
            scene.push(placement);
        }
        Self::authorise_block_metadata(&self.db, block.id, access)?;
        Ok(scene)
    }
}
