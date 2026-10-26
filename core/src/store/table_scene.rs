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
        self.table_ink_scene_page_budget(block, revision, cursor, (max_points, PAGE_BYTES), access)
    }
    pub(crate) fn table_ink_scene_page_budget(
        &self,
        block: Id,
        revision: u64,
        cursor: Option<TableInkCursor>,
        budget: (u32, usize),
        access: &Access,
    ) -> Result<TableInkScenePage> {
        let (max_points, byte_budget) = budget;
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
        if byte_budget <= 1024 || byte_budget > PAGE_BYTES {
            return Err(Error::ResourceLimit);
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
            let mut fragment = CellInkFragment {
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
            let mut encoded = serde_json::to_vec(&fragment)?.len() + 2;
            if encoded > byte_budget - bytes && !fragments.is_empty() {
                break;
            }
            while encoded > byte_budget - bytes && fragment.points.len() > 1 {
                fragment.points.truncate(fragment.points.len().div_ceil(2));
                encoded = serde_json::to_vec(&fragment)?.len() + 2;
            }
            if encoded > byte_budget - bytes {
                return Err(Error::ResourceLimit);
            }
            let taken = fragment.points.len();
            bytes += encoded;
            remaining -= taken;
            offset += taken;
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

#[cfg(test)]
mod budget_tests {
    use super::*;
    #[test]
    fn reserved_envelope_budget_shrinks_fragments_without_skipping_observations() {
        use crate::{
            ink::Tool,
            objects::{Cell, CellValue, Position, Table, TableLayout},
        };
        let mut store = Store::memory().unwrap();
        let access = Access::public();
        let cell = Id::new();
        let note = store.create(Kind::Note, None, "budget", &access).unwrap();
        let block = store
            .append_content(
                note.id,
                Content::Table(Table {
                    rows: vec![vec![Cell {
                        id: cell,
                        value: CellValue::Text(String::new()),
                    }]],
                    header_rows: 0,
                    layout: Some(TableLayout {
                        coordinate_space: "local".into(),
                        origin: Position { x: 0., y: 0. },
                        row_heights: vec![20.],
                        column_widths: vec![40.],
                        ink_origins: vec![],
                    }),
                }),
                &access,
            )
            .unwrap();
        let points: Vec<_> = (0..100)
            .map(|i| Point {
                x: i as f64,
                y: 0.,
                time_us: i,
                pressure: None,
                tilt_rad: None,
                orientation_rad: None,
                buttons: None,
            })
            .collect();
        let stroke = Stroke {
            id: Id::new(),
            capture_session: Id::new(),
            sequence: 0,
            pointer_id: 1,
            coordinate_space: "local".into(),
            tool: Tool::Pen,
            style: None,
            end: StrokeEnd::Completed,
            points: points.clone(),
        };
        let block = store
            .append_cell_capture(
                Id::new(),
                block.id,
                block.revision,
                cell,
                vec![stroke],
                &access,
            )
            .unwrap();
        let mut cursor = None;
        let mut reconstructed = Vec::new();
        let mut pages = 0;
        loop {
            let page = store
                .table_ink_scene_page_budget(block.id, block.revision, cursor, (100, 2048), &access)
                .unwrap();
            assert!(serde_json::to_vec(&page).unwrap().len() <= 2048);
            assert!(!page.fragments.is_empty());
            for fragment in page.fragments {
                assert_eq!(fragment.point_offset as usize, reconstructed.len());
                assert_eq!(fragment.preceding, reconstructed.last().cloned());
                reconstructed.extend(fragment.points);
            }
            pages += 1;
            match page.next {
                Some(next) => {
                    assert_ne!(Some(next), cursor);
                    cursor = Some(next);
                }
                None => break,
            }
        }
        assert!(pages > 1);
        assert_eq!(reconstructed, points);
        assert_eq!(
            store.source_ink(block.id, &access).unwrap()[0].points,
            points
        );
    }
}
