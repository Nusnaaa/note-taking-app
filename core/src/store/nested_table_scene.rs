//! Iterative nested-embed context validation and exact transformed clip paths.
use super::canvas_table_scene::compose;
use super::*;
use crate::{ink::Transform, objects::Rect};
use std::collections::BTreeSet;
const WIRE_BYTES: usize = 8 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasEmbedPathStep {
    pub embedding: CanvasEmbedRevision,
    pub host_revision: u64,
    pub host_layer_revision: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NestedCanvasTableInkScope {
    pub canvas: Id,
    pub entity_revision: u64,
    pub layer_revision: u64,
    /// Inner to outer; each next embedding must source the preceding host.
    pub path: Vec<CanvasEmbedPathStep>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasInkClip {
    pub bounds: Rect,
    pub to_surface: Transform,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NestedCanvasTableInkPage {
    pub scope: NestedCanvasTableInkScope,
    /// Intersect these transformed rectangles; an AABB would lose rotations.
    pub clips: Vec<CanvasInkClip>,
    pub ink: TableInkScenePage,
}
impl Store {
    pub(crate) fn nested_table_scope(
        db: &Connection,
        scope: &NestedCanvasTableInkScope,
        block: Id,
        access: &Access,
    ) -> Result<(Transform, Vec<CanvasInkClip>, Id)> {
        Self::check_canvas(db, scope.canvas, access)?;
        let root = Self::authorise(db, scope.canvas, access, false)?;
        let layers = Self::raw_canvas_layers(db, scope.canvas)?;
        if root.revision != scope.entity_revision || layers.revision != scope.layer_revision {
            return Err(Error::Conflict);
        }
        Self::authorise_block_metadata(db, block, access)?;
        let (owner, visible): (String, bool) = db.query_row(
            "SELECT entity,json_extract(payload,'$.visible') FROM blocks WHERE id=?1",
            [block.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if Id::parse(&owner)? != scope.canvas {
            return Err(Error::Invalid("canvas table member"));
        }
        if !visible {
            return Err(Error::Unavailable);
        }
        let (mut layer, mut shown) = layers
            .layers
            .iter()
            .find(|l| l.blocks.contains(&block))
            .map(|l| (LayerRef::Custom(l.id), l.visible))
            .unwrap_or((LayerRef::Base, layers.base_visible));
        let mut surface = scope.canvas;
        let mut visited = BTreeSet::from([surface]);
        let mut matrices = Vec::new();
        let mut frames = Vec::new();
        for (index, step) in scope.path.iter().enumerate() {
            let e = Self::read_embed(db, step.embedding.id, access)?;
            if e.source != surface {
                return Err(Error::Invalid("disconnected scene path"));
            }
            if e.revision != step.embedding.revision {
                return Err(Error::Conflict);
            }
            let visible = e
                .visible_layers
                .as_ref()
                .map_or(shown, |layers| layers.contains(&layer));
            if !visible {
                return Err(Error::Unavailable);
            }
            let host = Self::authorise(db, e.host, access, false)?;
            if host.revision != step.host_revision {
                return Err(Error::Conflict);
            }
            if !visited.insert(host.id) {
                return Err(Error::Invalid("scene path cycle"));
            }
            if host.kind == Kind::Canvas {
                let host_layers = Self::raw_canvas_layers(db, host.id)?;
                if Some(host_layers.revision) != step.host_layer_revision {
                    return Err(Error::Conflict);
                }
                shown = host_layers.base_visible;
            } else {
                if !host.kind.is_surface()
                    || step.host_layer_revision.is_some()
                    || index + 1 != scope.path.len()
                {
                    return Err(Error::Invalid("scene path host"));
                }
                shown = true;
            }
            let camera = e.camera;
            let frame = e.frame;
            let t = camera.to_frame(frame)?;
            matrices.push(t);
            frames.push(frame);
            surface = host.id;
            layer = LayerRef::Base;
        }
        if !shown {
            return Err(Error::Unavailable);
        }
        let mut suffix = Transform::default();
        let mut clips = Vec::with_capacity(frames.len());
        for (frame, matrix) in frames.into_iter().zip(matrices).rev() {
            for (x, y) in [
                (frame.x, frame.y),
                (frame.x + frame.width, frame.y),
                (frame.x, frame.y + frame.height),
                (frame.x + frame.width, frame.y + frame.height),
            ] {
                if !(suffix.a * x + suffix.c * y + suffix.tx).is_finite()
                    || !(suffix.b * x + suffix.d * y + suffix.ty).is_finite()
                {
                    return Err(Error::Invalid("scene clip coordinates"));
                }
            }
            clips.push(CanvasInkClip {
                bounds: frame,
                to_surface: suffix,
            });
            suffix = compose(suffix, matrix)?;
        }
        clips.reverse();
        Ok((suffix, clips, surface))
    }
    pub fn nested_canvas_table_ink_scene_page(
        &self,
        scope: NestedCanvasTableInkScope,
        block: Id,
        revision: u64,
        cursor: Option<TableInkCursor>,
        max_points: u32,
        access: &Access,
    ) -> Result<NestedCanvasTableInkPage> {
        let (outer, clips, surface) = Self::nested_table_scope(&self.db, &scope, block, access)?;
        let empty = TableInkScenePage {
            block,
            revision,
            fragments: vec![],
            next: None,
        };
        let mut page = NestedCanvasTableInkPage {
            scope,
            clips,
            ink: empty,
        };
        let overhead = serde_json::to_vec(&page)?.len();
        let budget = WIRE_BYTES
            .checked_sub(overhead)
            .filter(|b| *b > 1024)
            .ok_or(Error::ResourceLimit)?;
        page.ink = self.table_ink_scene_page_budget(
            block,
            revision,
            cursor,
            (max_points, budget),
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
                return Err(Error::Invalid("nested ink coordinates"));
            }
        }
        if Self::nested_table_scope(&self.db, &page.scope, block, access)?
            != (outer, page.clips.clone(), surface)
        {
            return Err(Error::Conflict);
        }
        if serde_json::to_vec(&page)?.len() > WIRE_BYTES {
            return Err(Error::ResourceLimit);
        }
        Ok(page)
    }
}
