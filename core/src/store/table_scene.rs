//! Revision-bound vector fragments. Cursors identify original samples; fragments
//! are rendering excerpts, never new captured strokes or raster recognition input.
use super::*;
use crate::{
    content::Content,
    ink::{Point, Stroke, StrokeEnd, StrokeStyle, Tool, Transform},
};
const PAGE_BYTES: usize = 8 * 1024 * 1024; // existing canvas-scene wire budget
const RECORD_BYTES: usize = 64 * 1024 * 1024; // existing logical-backup cell budget

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableInkCursor {
    pub source_index: u32,
    pub point_offset: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellInkFragment {
    pub cell: Id,
    pub source_id: Id,
    pub capture_session: Id,
    pub sequence: u64,
    pub pointer_id: u32,
    pub coordinate_space: String,
    pub tool: Tool,
    pub style: Option<StrokeStyle>,
    pub source_end: StrokeEnd,
    pub source_points: u32,
    pub point_offset: u32,
    /// Actual preceding observation for independent segment rendering. This
    /// context sample is not part of `points` when reconstructing the source.
    pub preceding: Option<Point>,
    pub points: Vec<Point>,
    pub source_to_surface: Transform,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableInkScenePage {
    pub block: Id,
    pub revision: u64,
    pub fragments: Vec<CellInkFragment>,
    pub next: Option<TableInkCursor>,
}
impl Store {
    pub(crate) fn scene_stroke(db: &Connection, block: Id, id: Id) -> Result<Stroke> {
        let bytes: usize = db.query_row(
            "SELECT length(CAST(payload AS BLOB)) FROM strokes WHERE id=?1 AND block=?2",
            rusqlite::params![id.to_string(), block.to_string()],
            |r| r.get(0),
        )?;
        if bytes > RECORD_BYTES {
            return Err(Error::ResourceLimit);
        }
        Self::original_stroke(db, block, id)
    }
    pub(crate) fn place_table_stroke(block: &Block, stroke: Stroke) -> Result<CellInkPlacement> {
        let Content::Table(table) = &block.content else {
            return Err(Error::Invalid("table block"));
        };
        let layout = table.layout.as_ref().ok_or(Error::ReviewRequired)?;
        layout.validate_source(table, &stroke)?;
        if stroke.coordinate_space != layout.coordinate_space {
            return Err(Error::Invalid("table ink coordinate space"));
        }
        let shift = layout.stroke_transform(table, stroke.id)?;
        let mut transform = block.transform;
        transform.tx += transform.a * shift.tx + transform.c * shift.ty;
        transform.ty += transform.b * shift.tx + transform.d * shift.ty;
        transform.validate()?;
        if stroke.points.iter().any(|p| {
            !(transform.a * p.x + transform.c * p.y + transform.tx).is_finite()
                || !(transform.b * p.x + transform.d * p.y + transform.ty).is_finite()
        }) {
            return Err(Error::Invalid("table ink scene coordinates"));
        }
        let cell = layout
            .ink_origins
            .iter()
            .find(|ink| ink.stroke == stroke.id)
            .ok_or(Error::Integrity)?
            .cell;
        Ok(CellInkPlacement {
            cell,
            stroke,
            source_to_surface: transform,
        })
    }
    pub fn table_ink_scene_page(
        &self,
        block: Id,
        revision: u64,
        cursor: Option<TableInkCursor>,
        max_points: u32,
        access: &Access,
    ) -> Result<TableInkScenePage> {
        let b = Self::read_block(&self.db, block, access)?;
        if b.revision != revision {
            return Err(Error::Conflict);
        }
        b.validate()?;
        let Content::Table(table) = &b.content else {
            return Err(Error::Invalid("table block"));
        };
        if table.layout.is_none() {
            return Err(Error::ReviewRequired);
        }
        if !b.visible {
            return Err(Error::Unavailable);
        }
        if max_points == 0 || max_points > 4096 {
            return Err(Error::Invalid("table scene point budget"));
        }
        let cursor = cursor.unwrap_or(TableInkCursor {
            source_index: 0,
            point_offset: 0,
        });
        let mut index = cursor.source_index as usize;
        let mut offset = cursor.point_offset as usize;
        if index > b.source_ids.len() || (index == b.source_ids.len() && offset != 0) {
            return Err(Error::Invalid("table scene cursor"));
        }
        let mut fragments = Vec::new();
        let mut remaining = max_points as usize;
        let mut bytes = 1024usize;
        while index < b.source_ids.len() && remaining > 0 {
            let id = b.source_ids[index];
            if !b.visible_strokes.contains(&id) {
                if offset != 0 {
                    return Err(Error::Invalid("table scene cursor"));
                }
                index += 1;
                continue;
            }
            let placement = Self::place_table_stroke(&b, Self::scene_stroke(&self.db, block, id)?)?;
            let stroke = placement.stroke;
            let count = stroke.points.len();
            if offset >= count {
                return Err(Error::Invalid("table scene cursor"));
            }
            let take = (count - offset).min(remaining);
            let preceding = offset
                .checked_sub(1)
                .and_then(|at| stroke.points.get(at))
                .cloned();
            let fragment = CellInkFragment {
                cell: placement.cell,
                source_id: stroke.id,
                capture_session: stroke.capture_session,
                sequence: stroke.sequence,
                pointer_id: stroke.pointer_id,
                coordinate_space: stroke.coordinate_space,
                tool: stroke.tool,
                style: stroke.style,
                source_end: stroke.end,
                source_points: u32::try_from(count).map_err(|_| Error::ResourceLimit)?,
                point_offset: u32::try_from(offset).map_err(|_| Error::ResourceLimit)?,
                preceding,
                points: stroke.points.into_iter().skip(offset).take(take).collect(),
                source_to_surface: placement.source_to_surface,
            };
            let encoded = serde_json::to_vec(&fragment)?.len() + 2;
            if encoded > PAGE_BYTES - bytes {
                if fragments.is_empty() {
                    return Err(Error::ResourceLimit);
                }
                break;
            }
            bytes += encoded;
            remaining -= take;
            offset += take;
            fragments.push(fragment);
            if offset == count {
                index += 1;
                offset = 0;
            }
        }
        // Skip hidden records without loading their observed point arrays.
        while index < b.source_ids.len()
            && offset == 0
            && !b.visible_strokes.contains(&b.source_ids[index])
        {
            index += 1;
        }
        let next = if index < b.source_ids.len() {
            Some(TableInkCursor {
                source_index: u32::try_from(index).map_err(|_| Error::ResourceLimit)?,
                point_offset: u32::try_from(offset).map_err(|_| Error::ResourceLimit)?,
            })
        } else {
            None
        };
        Self::authorise_block_metadata(&self.db, block, access)?;
        Ok(TableInkScenePage {
            block,
            revision,
            fragments,
            next,
        })
    }
}
