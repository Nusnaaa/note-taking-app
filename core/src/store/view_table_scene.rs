//! Explicit saved-view camera projection over direct or nested table sources.
use super::canvas_table_scene::compose;
use super::*;
use crate::{ink::Transform, objects::Rect};
const WIRE_BYTES: usize = 8 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum WritingViewTableSource {
    Direct { entity_revision: u64 },
    Nested(NestedCanvasTableInkScope),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WritingViewTableScope {
    pub view: Id,
    pub revision: u64,
    pub source: WritingViewTableSource,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WritingViewTableRequest {
    pub scope: WritingViewTableScope,
    pub block: Id,
    pub revision: u64,
    pub cursor: Option<TableInkCursor>,
    pub max_points: u32,
    /// Caller-defined logical surface units; never inferred device pixels/mm.
    pub viewport: Rect,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WritingViewTablePage {
    pub scope: WritingViewTableScope,
    pub viewport: Rect,
    pub clips: Vec<CanvasInkClip>,
    pub ink: TableInkScenePage,
}
fn check_clip(clip: &CanvasInkClip) -> Result<()> {
    clip.bounds.validate()?;
    clip.to_surface.validate()?;
    let r = clip.bounds;
    let t = clip.to_surface;
    for (x, y) in [
        (r.x, r.y),
        (r.x + r.width, r.y),
        (r.x, r.y + r.height),
        (r.x + r.width, r.y + r.height),
    ] {
        if !(t.a * x + t.c * y + t.tx).is_finite() || !(t.b * x + t.d * y + t.ty).is_finite() {
            return Err(Error::Invalid("view clip coordinates"));
        }
    }
    Ok(())
}
impl Store {
    fn writing_view_table_scope(
        db: &Connection,
        scope: &WritingViewTableScope,
        block: Id,
        viewport: Rect,
        access: &Access,
    ) -> Result<(Transform, Vec<CanvasInkClip>)> {
        let view = Self::read_view_db(db, scope.view, access)?.settings;
        if view.revision != scope.revision {
            return Err(Error::Conflict);
        }
        let camera = view.camera.to_frame(viewport)?;
        let (outer, mut clips) = match &scope.source {
            WritingViewTableSource::Direct { entity_revision } => {
                let entity = Self::authorise(db, view.entity, access, false)?;
                if entity.revision != *entity_revision {
                    return Err(Error::Conflict);
                }
                Self::authorise_block_metadata(db, block, access)?;
                let (owner, visible): (String, bool) = db.query_row(
                    "SELECT entity,json_extract(payload,'$.visible') FROM blocks WHERE id=?1",
                    [block.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                if Id::parse(&owner)? != view.entity {
                    return Err(Error::Invalid("writing view source"));
                }
                if !visible {
                    return Err(Error::Unavailable);
                }
                if entity.kind == Kind::Canvas {
                    let layers = Self::raw_canvas_layers(db, entity.id)?;
                    let shown = layers
                        .layers
                        .iter()
                        .find(|layer| layer.blocks.contains(&block))
                        .map_or(layers.base_visible, |layer| layer.visible);
                    if !shown {
                        return Err(Error::Unavailable);
                    }
                }
                (Transform::default(), vec![])
            }
            WritingViewTableSource::Nested(nested) => {
                let (matrix, clips, surface) = Self::nested_table_scope(db, nested, block, access)?;
                if surface != view.entity {
                    return Err(Error::Invalid("writing view surface"));
                }
                if view.exercise.is_some() && !nested.path.is_empty() {
                    return Err(Error::Unavailable);
                }
                (matrix, clips)
            }
        };
        if let Some(exercise) = view.exercise {
            let exercise = Self::read_exercise(db, exercise, access)?;
            if !exercise.visible || Self::block_exercise(db, block)? != Some(exercise.id) {
                return Err(Error::Unavailable);
            }
        }
        for clip in &mut clips {
            clip.to_surface = compose(camera, clip.to_surface)?;
            check_clip(clip)?;
        }
        clips.push(CanvasInkClip {
            bounds: viewport,
            to_surface: Transform::default(),
        });
        Ok((compose(camera, outer)?, clips))
    }
    pub fn writing_view_table_ink_scene_page(
        &self,
        request: WritingViewTableRequest,
        access: &Access,
    ) -> Result<WritingViewTablePage> {
        let (outer, clips) = Self::writing_view_table_scope(
            &self.db,
            &request.scope,
            request.block,
            request.viewport,
            access,
        )?;
        let mut page = WritingViewTablePage {
            scope: request.scope,
            viewport: request.viewport,
            clips,
            ink: TableInkScenePage {
                block: request.block,
                revision: request.revision,
                fragments: vec![],
                next: None,
            },
        };
        let overhead = serde_json::to_vec(&page)?.len();
        let budget = WIRE_BYTES
            .checked_sub(overhead)
            .filter(|b| *b > 1024)
            .ok_or(Error::ResourceLimit)?;
        page.ink = self.table_ink_scene_page_budget(
            request.block,
            request.revision,
            request.cursor,
            (request.max_points, budget),
            access,
        )?;
        for fragment in &mut page.ink.fragments {
            fragment.source_to_surface = compose(outer, fragment.source_to_surface)?;
            let t = fragment.source_to_surface;
            if fragment
                .points
                .iter()
                .chain(fragment.preceding.iter())
                .any(|p| {
                    !(t.a * p.x + t.c * p.y + t.tx).is_finite()
                        || !(t.b * p.x + t.d * p.y + t.ty).is_finite()
                })
            {
                return Err(Error::Invalid("view ink coordinates"));
            }
        }
        if Self::writing_view_table_scope(
            &self.db,
            &page.scope,
            request.block,
            page.viewport,
            access,
        )? != (outer, page.clips.clone())
        {
            return Err(Error::Conflict);
        }
        if serde_json::to_vec(&page)?.len() > WIRE_BYTES {
            return Err(Error::ResourceLimit);
        }
        Ok(page)
    }
}
