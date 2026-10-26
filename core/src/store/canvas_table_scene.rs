//! Canvas-visible table fragments bound to layer, source-owner and optional
//! single-embed revisions. Parent screen/view transforms remain explicit.
use super::*;
use crate::{ink::Transform, objects::Rect};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasEmbedRevision {
    pub id: Id,
    pub revision: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasTableInkScope {
    pub canvas: Id,
    pub entity_revision: u64,
    pub layer_revision: u64,
    pub embedding: Option<CanvasEmbedRevision>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasTableInkPage {
    pub scope: CanvasTableInkScope,
    /// In the embedding host's logical coordinates. Clipping never edits ink.
    pub clip: Option<Rect>,
    pub ink: TableInkScenePage,
}
fn compose(outer: Transform, inner: Transform) -> Result<Transform> {
    let value = Transform {
        a: outer.a * inner.a + outer.c * inner.b,
        b: outer.b * inner.a + outer.d * inner.b,
        c: outer.a * inner.c + outer.c * inner.d,
        d: outer.b * inner.c + outer.d * inner.d,
        tx: outer.a * inner.tx + outer.c * inner.ty + outer.tx,
        ty: outer.b * inner.tx + outer.d * inner.ty + outer.ty,
    };
    value.validate()?;
    Ok(value)
}
impl Store {
    fn canvas_table_scope(
        db: &Connection,
        scope: &CanvasTableInkScope,
        block: Id,
        access: &Access,
    ) -> Result<(Transform, Option<Rect>)> {
        Self::check_canvas(db, scope.canvas, access)?;
        let entity = Self::authorise(db, scope.canvas, access, false)?;
        let layers = Self::raw_canvas_layers(db, scope.canvas)?;
        if entity.revision != scope.entity_revision || layers.revision != scope.layer_revision {
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
        let (layer, default) = layers
            .layers
            .iter()
            .find(|layer| layer.blocks.contains(&block))
            .map(|layer| (LayerRef::Custom(layer.id), layer.visible))
            .unwrap_or((LayerRef::Base, layers.base_visible));
        let mut transform = Transform::default();
        let mut clip = None;
        let mut shown = default;
        if let Some(expected) = &scope.embedding {
            // Authorise SQL identities before decoding the embedding payload.
            let (host, source): (String, String) = db
                .query_row(
                    "SELECT host,source FROM embeds WHERE id=?1",
                    [expected.id.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or(Error::Unavailable)?;
            let host = Id::parse(&host)?;
            let source = Id::parse(&source)?;
            Self::authorise(db, host, access, false)?;
            Self::authorise(db, source, access, false)?;
            let embedding = Self::read_embed(db, expected.id, access)?;
            if embedding.id != expected.id || embedding.host != host || embedding.source != source {
                return Err(Error::Integrity);
            }
            if source != scope.canvas {
                return Err(Error::Invalid("scene embed source"));
            }
            if embedding.revision != expected.revision {
                return Err(Error::Conflict);
            }
            embedding.camera.validate()?;
            embedding.frame.validate()?;
            if let Some(visible) = &embedding.visible_layers {
                shown = visible.contains(&layer);
            }
            let camera = embedding.camera;
            let frame = embedding.frame;
            let (sin, cos) = camera.rotation_rad.sin_cos();
            transform = Transform {
                a: camera.zoom * cos,
                b: camera.zoom * sin,
                c: -camera.zoom * sin,
                d: camera.zoom * cos,
                tx: 0.,
                ty: 0.,
            };
            transform.tx = frame.x + frame.width / 2.
                - transform.a * camera.centre_x
                - transform.c * camera.centre_y;
            transform.ty = frame.y + frame.height / 2.
                - transform.b * camera.centre_x
                - transform.d * camera.centre_y;
            transform.validate()?;
            clip = Some(frame);
        }
        if !shown {
            return Err(Error::Unavailable);
        }
        Ok((transform, clip))
    }
    pub fn canvas_table_ink_scene_page(
        &self,
        scope: CanvasTableInkScope,
        block: Id,
        revision: u64,
        cursor: Option<TableInkCursor>,
        max_points: u32,
        access: &Access,
    ) -> Result<CanvasTableInkPage> {
        let (outer, clip) = Self::canvas_table_scope(&self.db, &scope, block, access)?;
        let mut ink = self.table_ink_scene_page(block, revision, cursor, max_points, access)?;
        for fragment in &mut ink.fragments {
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
                return Err(Error::Invalid("embedded ink coordinates"));
            }
        }
        if Self::canvas_table_scope(&self.db, &scope, block, access)? != (outer, clip) {
            return Err(Error::Conflict);
        }
        let page = CanvasTableInkPage { scope, clip, ink };
        if serde_json::to_vec(&page)?.len() > 8 * 1024 * 1024 {
            return Err(Error::ResourceLimit);
        }
        Ok(page)
    }
}
