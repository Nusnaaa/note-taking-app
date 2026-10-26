//! Whole-library semantic validation. Work indexes live inside the private stage
//! under a savepoint and are rolled back on both success and failure. Rust keeps
//! only individual model records; this is not a total SQLite or record-size quota.
use super::*;
use crate::{content::Content, ink::Stroke, processing::JobControl, store::Kind};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeSet;

#[cfg(test)]
thread_local! {
    static CANCEL_AFTER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

// Empty work tables are populated row by row. CREATE INDEX over the populated
// source graph could invoke SQLite's external sorter outside the private stage.
const WORK_EDGES: &str = "CREATE TABLE restore_validation_edges(
    host TEXT NOT NULL, edge INTEGER NOT NULL, source TEXT NOT NULL,
    PRIMARY KEY(host,edge)) STRICT, WITHOUT ROWID";
const EMBED_HOSTS: &str = "SELECT DISTINCT host FROM restore_validation_edges";
const FIRST_EDGE: &str =
    "SELECT edge,source FROM restore_validation_edges WHERE host=?1 ORDER BY edge LIMIT 1";
const NEXT_EDGE: &str = "SELECT edge,source FROM restore_validation_edges WHERE host=?1 AND edge>?2 ORDER BY edge LIMIT 1";

pub(super) fn check(control: Option<&JobControl>) -> Result<()> {
    #[cfg(test)]
    CANCEL_AFTER.with(|remaining| {
        if let (Some(count), Some(control)) = (remaining.get(), control) {
            if count == 0 {
                control.cancel();
            } else {
                remaining.set(Some(count - 1));
            }
        }
    });
    control.map_or(Ok(()), JobControl::check)
}

fn each(
    db: &Connection,
    sql: &str,
    control: Option<&JobControl>,
    mut visit: impl FnMut(&rusqlite::Row<'_>) -> Result<()>,
) -> Result<()> {
    let mut statement = db.prepare(sql)?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        check(control)?;
        visit(row)?;
    }
    Ok(())
}

fn identity(db: &Connection, id: Id, kind: &str) -> Result<()> {
    if db.execute(
        "INSERT OR IGNORE INTO restore_validation_ids(id,kind) VALUES(?1,?2)",
        params![id.to_string(), kind],
    )? != 1
    {
        return Err(Error::Invalid("restore duplicate identity"));
    }
    Ok(())
}

fn entity(db: &Connection, id: Id) -> Result<Entity> {
    Store::raw_entity(db, id)
}

fn block(db: &Connection, id: Id) -> Result<Block> {
    Store::raw_block(db, id)
}

fn block_content(
    db: &Connection,
    b: &Block,
    version: u32,
    control: Option<&JobControl>,
) -> Result<()> {
    b.validate()?;
    if version < 12 && b.regions.iter().any(|region| region.cell.is_some()) {
        return Err(Error::Unsupported);
    }
    if version < 11 && b.has_table_ink() {
        return Err(Error::Unsupported);
    }
    if b.revision > i64::MAX as u64 {
        return Err(Error::Integrity);
    }
    if version < 4 && !b.regions.is_empty() {
        return Err(Error::Unsupported);
    }
    for region in &b.regions {
        check(control)?;
        db.execute(
            "INSERT OR IGNORE INTO restore_validation_regions(id,owner) VALUES(?1,?2)",
            params![region.id.to_string(), b.id.to_string()],
        )?;
        let owner: String = db.query_row(
            "SELECT owner FROM restore_validation_regions WHERE id=?1",
            [region.id.to_string()],
            |r| r.get(0),
        )?;
        if owner != b.id.to_string() {
            return Err(Error::Invalid("restore region owner"));
        }
    }
    if let Content::Image(image) = &b.content {
        let owner: Option<String> = db
            .query_row(
                "SELECT entity FROM image_assets WHERE id=?1",
                [image.asset.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if owner.as_deref() != Some(b.entity.to_string().as_str()) {
            return Err(Error::Invalid("restore image reference"));
        }
    }
    Ok(())
}

impl Store {
    #[cfg(test)]
    pub(super) fn validate_backup_state(&self, version: u32) -> Result<()> {
        self.validate_backup_state_controlled(version, None)
    }

    pub(super) fn validate_backup_state_controlled(
        &self,
        version: u32,
        control: Option<&JobControl>,
    ) -> Result<()> {
        check(control)?;
        if !matches!(version, 1..=12) {
            return Err(Error::Unsupported);
        }
        self.validate_backup_prelude(version, control)?;
        let db = &self.db;
        db.execute_batch("SAVEPOINT restore_semantic_validation")?;
        let result = (|| {
            db.execute_batch(
                "CREATE TABLE restore_validation_ids(id TEXT PRIMARY KEY,kind TEXT NOT NULL) STRICT;
                 CREATE TABLE restore_validation_regions(id TEXT PRIMARY KEY,owner TEXT NOT NULL) STRICT;
                 CREATE TABLE restore_validation_headings(id TEXT PRIMARY KEY) STRICT;
                 CREATE TABLE restore_validation_colours(id TEXT PRIMARY KEY,colour INTEGER NOT NULL) STRICT;
                 CREATE TABLE restore_validation_stack(depth INTEGER PRIMARY KEY,id TEXT NOT NULL,last_edge INTEGER) STRICT;",
            )?;
            db.execute_batch(WORK_EDGES)?;
            self.validate_restore_graph(version, control)?;
            self.validate_backup_aux(control)?;
            check(control)
        })();
        // Even successful validation publishes none of its private work tables.
        // Do not skip cleanup when cancellation/validation fails.
        let cleanup = db.execute_batch(
            "ROLLBACK TO restore_semantic_validation; RELEASE restore_semantic_validation",
        );
        result?;
        cleanup?;
        Ok(())
    }

    fn validate_restore_graph(&self, version: u32, control: Option<&JobControl>) -> Result<()> {
        let db = &self.db;
        each(db, "SELECT id,parent,payload FROM entities", control, |r| {
            let id: String = r.get(0)?;
            let parent: Option<String> = r.get(1)?;
            let e: Entity = serde_json::from_str(&r.get::<_, String>(2)?)?;
            if e.id.to_string() != id
                || e.parent.map(|p| p.to_string()) != parent
                || e.title.len() > 4096
                || e.revision > i64::MAX as u64
            {
                return Err(Error::Integrity);
            }
            // Trash and purge flags are retained in whole-library backups.
            identity(db, e.id, "entity")?;
            let mut child = e;
            let mut seen = BTreeSet::from([child.id]);
            let mut modules = usize::from(child.kind == Kind::Module);
            while let Some(parent) = child.parent {
                check(control)?;
                let ancestor = entity(db, parent)?;
                modules += usize::from(ancestor.kind == Kind::Module);
                if !seen.insert(parent) || !ancestor.kind.accepts(child.kind) || modules > 2 {
                    return Err(Error::Invalid("restore containment"));
                }
                child = ancestor;
            }
            Ok(())
        })?;

        each(db, "SELECT id,entity,payload FROM blocks", control, |r| {
            let id: String = r.get(0)?;
            let owner: String = r.get(1)?;
            let b: Block = serde_json::from_str(&r.get::<_, String>(2)?)?;
            if b.id.to_string() != id
                || b.entity.to_string() != owner
                || !entity(db, b.entity)?.kind.is_surface()
            {
                return Err(Error::Integrity);
            }
            identity(db, b.id, "block")?;
            block_content(db, &b, version, control)?;
            let source_ids: BTreeSet<_> = b.source_ids.iter().copied().collect();
            let mut stroke_query =
                db.prepare("SELECT payload,digest FROM strokes WHERE id=?1 AND block=?2")?;
            for id in &b.source_ids {
                check(control)?;
                let (json, hash): (String, String) = stroke_query
                    .query_row(params![id.to_string(), b.id.to_string()], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })?;
                let stroke: Stroke = serde_json::from_str(&json)?;
                if crate::digest(json.as_bytes()) != hash || stroke.id != *id {
                    return Err(Error::Integrity);
                }
                stroke.validate_persisted()?;
                if version < 10
                    && stroke
                        .style
                        .is_some_and(|style| style.pressure_response.is_some())
                {
                    return Err(Error::Unsupported);
                }
                if (version < 7 && stroke.style.is_some())
                    || (version < 5 && !stroke.end.is_completed())
                {
                    return Err(Error::Unsupported);
                }
                identity(db, stroke.id, "stroke")?;
            }
            let mut query = db.prepare(
                "SELECT revision,active_ms,payload FROM history WHERE block=?1 ORDER BY revision",
            )?;
            let mut rows = query.query([&id])?;
            while let Some(row) = rows.next()? {
                check(control)?;
                let revision: u64 = row.get(0)?;
                let active_ms: u64 = row.get(1)?;
                let historical: Block = serde_json::from_str(&row.get::<_, String>(2)?)?;
                block_content(db, &historical, version, control)?;
                if historical.id != b.id
                    || historical.entity != b.entity
                    || historical.revision != revision
                    || revision > b.revision
                    || active_ms > i64::MAX as u64
                    || historical
                        .source_ids
                        .iter()
                        .any(|id| !source_ids.contains(id))
                {
                    return Err(Error::Invalid("restore history"));
                }
            }
            Ok(())
        })?;
        // Every stored raw stroke must be accounted for, including invisible ink.
        // The old subset reconstruction silently omitted unreferenced rows.
        let orphan: bool = db.query_row(
            "SELECT (SELECT count(*) FROM strokes) != (SELECT count(*) FROM restore_validation_ids WHERE kind='stroke')",
            [], |r| r.get(0),
        )?;
        if orphan {
            return Err(Error::Invalid("restore orphan stroke"));
        }

        each(
            db,
            "SELECT id,master,heading,payload FROM exercises",
            control,
            |r| {
                if version == 1 {
                    return Err(Error::Unsupported);
                }
                let e: Exercise = serde_json::from_str(&r.get::<_, String>(3)?)?;
                if e.id.to_string() != r.get::<_, String>(0)?
                    || e.master.to_string() != r.get::<_, String>(1)?
                    || e.heading.map(|h| h.to_string()) != r.get::<_, Option<String>>(2)?
                    || entity(db, e.master)?.kind != Kind::Note
                    || e.title.is_empty()
                    || e.title.len() > 4096
                    || e.revision > i64::MAX as u64
                {
                    return Err(Error::Invalid("restore exercise"));
                }
                identity(db, e.id, "exercise")?;
                if let Some(heading) = e.heading {
                    if block(db, heading)?.entity != e.master
                        || db.execute(
                            "INSERT OR IGNORE INTO restore_validation_headings VALUES(?1)",
                            [heading.to_string()],
                        )? != 1
                    {
                        return Err(Error::Invalid("restore exercise heading"));
                    }
                }
                Ok(())
            },
        )?;
        each(db,
            "SELECT m.block,m.exercise,v.revision FROM exercise_members m LEFT JOIN membership_versions v ON m.block=v.block",
            control, |r| {
                if version == 1 { return Err(Error::Unsupported); }
                let b = Id::parse(&r.get::<_, String>(0)?)?;
                let e = Id::parse(&r.get::<_, String>(1)?)?;
                let revision: Option<u64> = r.get(2)?;
                if !revision.is_some_and(|v| v > 0 && v <= i64::MAX as u64)
                    || block(db, b)?.entity != Self::raw_exercise(db, e)?.master
                { return Err(Error::Invalid("restore membership")); }
                Ok(())
            })?;
        each(db, "SELECT block FROM confirmed_headings", control, |r| {
            if version == 1 {
                return Err(Error::Unsupported);
            }
            block(db, Id::parse(&r.get::<_, String>(0)?)?)?;
            Ok(())
        })?;
        each(
            db,
            "SELECT master,keyword FROM exercise_rules",
            control,
            |r| {
                if version == 1 {
                    return Err(Error::Unsupported);
                }
                entity(db, Id::parse(&r.get::<_, String>(0)?)?)?;
                let keyword: String = r.get(1)?;
                if keyword.trim().is_empty() || keyword.len() > 256 {
                    return Err(Error::Integrity);
                }
                Ok(())
            },
        )?;
        each(db, "SELECT entity,mode FROM modes", control, |r| {
            if version == 1 {
                return Err(Error::Unsupported);
            }
            entity(db, Id::parse(&r.get::<_, String>(0)?)?)?;
            let _: crate::store::WritingMode = serde_json::from_str(&r.get::<_, String>(1)?)?;
            Ok(())
        })?;
        each(
            db,
            "SELECT id,origin,target,anchor FROM links",
            control,
            |r| {
                let id = Id::parse(&r.get::<_, String>(0)?)?;
                let origin = Id::parse(&r.get::<_, String>(1)?)?;
                let target = Id::parse(&r.get::<_, String>(2)?)?;
                entity(db, origin)?;
                entity(db, target)?;
                identity(db, id, "link")?;
                if let Some(anchor) = r.get::<_, Option<String>>(3)? {
                    if block(db, Id::parse(&anchor)?)?.entity != origin {
                        return Err(Error::Integrity);
                    }
                }
                Ok(())
            },
        )?;
        self.validate_restore_attachments(version, control)?;
        let region_alias: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM restore_validation_regions r JOIN restore_validation_ids i ON i.id=r.id)",
            [], |r| r.get(0),
        )?;
        if region_alias {
            return Err(Error::Invalid("restore region identity"));
        }
        Ok(())
    }

    fn validate_restore_attachments(
        &self,
        version: u32,
        control: Option<&JobControl>,
    ) -> Result<()> {
        let db = &self.db;
        if version < 3 {
            let any: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM image_assets UNION ALL SELECT 1 FROM layouts UNION ALL SELECT 1 FROM object_groups UNION ALL SELECT 1 FROM embeds UNION ALL SELECT 1 FROM view_settings UNION ALL SELECT 1 FROM canvas_layers)",
                [], |r| r.get(0),
            )?;
            if any {
                return Err(Error::Unsupported);
            }
        }
        each(
            db,
            "SELECT canvas,payload FROM canvas_layers",
            control,
            |r| {
                if version < 8 {
                    return Err(Error::Unsupported);
                }
                let state: crate::store::CanvasLayers =
                    serde_json::from_str(&r.get::<_, String>(1)?)?;
                state.validate()?;
                if state.canvas.to_string() != r.get::<_, String>(0)?
                    || entity(db, state.canvas)?.kind != Kind::Canvas
                {
                    return Err(Error::Invalid("restore canvas layers"));
                }
                for layer in &state.layers {
                    check(control)?;
                    identity(db, layer.id, "layer")?;
                    for member in &layer.blocks {
                        check(control)?;
                        if block(db, *member)?.entity != state.canvas {
                            return Err(Error::Integrity);
                        }
                    }
                }
                Ok(())
            },
        )?;
        each(
            db,
            "SELECT rowid,id,entity,mime,digest FROM image_assets",
            control,
            |r| {
                let rowid: i64 = r.get(0)?;
                let id = Id::parse(&r.get::<_, String>(1)?)?;
                let owner = Id::parse(&r.get::<_, String>(2)?)?;
                let mime: String = r.get(3)?;
                let expected: String = r.get(4)?;
                identity(db, id, "image")?;
                if !entity(db, owner)?.kind.is_surface()
                    || super::stored_asset_digest(db, "image_assets", rowid, control)? != expected
                {
                    return Err(Error::Invalid("restore image asset"));
                }
                let blob = db.blob_open(
                    rusqlite::DatabaseName::Main,
                    "image_assets",
                    "bytes",
                    rowid,
                    true,
                )?;
                let signature: &[u8] = match mime.as_str() {
                    "image/png" => b"\x89PNG\r\n\x1a\n",
                    "image/jpeg" => b"\xff\xd8\xff",
                    _ => return Err(Error::Invalid("restore image MIME")),
                };
                let mut head = [0_u8; 8];
                if blob.len() < signature.len() {
                    return Err(Error::Integrity);
                }
                blob.read_at_exact(&mut head[..signature.len()], 0)?;
                if &head[..signature.len()] != signature {
                    return Err(Error::Integrity);
                }
                Ok(())
            },
        )?;
        each(db, "SELECT block,payload FROM layouts", control, |r| {
            block(db, Id::parse(&r.get::<_, String>(0)?)?)?;
            let layout: crate::store::Layout = serde_json::from_str(&r.get::<_, String>(1)?)?;
            layout.bounds.validate()?;
            if layout.revision > i64::MAX as u64 {
                return Err(Error::Integrity);
            }
            Ok(())
        })?;
        each(
            db,
            "SELECT id,entity,payload FROM object_groups",
            control,
            |r| {
                let group: ObjectGroup = serde_json::from_str(&r.get::<_, String>(2)?)?;
                if group.id.to_string() != r.get::<_, String>(0)?
                    || group.entity.to_string() != r.get::<_, String>(1)?
                    || group.members.is_empty()
                    || group.members.len() > 4096
                    || group.revision > i64::MAX as u64
                    || group.members.iter().collect::<BTreeSet<_>>().len() != group.members.len()
                {
                    return Err(Error::Invalid("restore group"));
                }
                identity(db, group.id, "group")?;
                for member in &group.members {
                    check(control)?;
                    if block(db, *member)?.entity != group.entity {
                        return Err(Error::Integrity);
                    }
                }
                Ok(())
            },
        )?;
        each(
            db,
            "SELECT rowid,id,host,source,payload FROM embeds",
            control,
            |r| {
                let embed: Embed = serde_json::from_str(&r.get::<_, String>(4)?)?;
                embed.validate_layers()?;
                embed.camera.validate()?;
                embed.frame.validate()?;
                if version < 8 && embed.visible_layers.is_some() {
                    return Err(Error::Unsupported);
                }
                if embed.id.to_string() != r.get::<_, String>(1)?
                    || embed.host.to_string() != r.get::<_, String>(2)?
                    || embed.source.to_string() != r.get::<_, String>(3)?
                    || embed.revision > i64::MAX as u64
                    || !entity(db, embed.host)?.kind.is_surface()
                    || entity(db, embed.source)?.kind != Kind::Canvas
                {
                    return Err(Error::Invalid("restore embed"));
                }
                identity(db, embed.id, "embed")?;
                db.execute(
                    "INSERT INTO restore_validation_edges VALUES(?1,?2,?3)",
                    params![
                        embed.host.to_string(),
                        r.get::<_, i64>(0)?,
                        embed.source.to_string()
                    ],
                )?;
                Ok(())
            },
        )?;
        validate_embed_cycles(db, control)?;
        each(
            db,
            "SELECT id,entity,payload FROM view_settings",
            control,
            |r| {
                let view: ViewSettings = serde_json::from_str(&r.get::<_, String>(2)?)?;
                view.validate()?;
                if version < 10 && view.authoring.as_ref().is_some_and(|a| a.requires_v10()) {
                    return Err(Error::Unsupported);
                }
                if version < 9 && view.authoring.is_some() || version < 6 && view.bookmark.is_some()
                {
                    return Err(Error::Unsupported);
                }
                if view.id.to_string() != r.get::<_, String>(0)?
                    || view.entity.to_string() != r.get::<_, String>(1)?
                {
                    return Err(Error::Integrity);
                }
                entity(db, view.entity)?;
                identity(db, view.id, "view")?;
                if let Some(exercise) = view.exercise {
                    if Self::raw_exercise(db, exercise)?.master != view.entity {
                        return Err(Error::Integrity);
                    }
                }
                if let Some(anchor) = view.anchor {
                    if block(db, anchor)?.entity != view.entity {
                        return Err(Error::Integrity);
                    }
                }
                if let Some(bookmark) = &view.bookmark {
                    for point in bookmark.points() {
                        let b = block(db, point.block)?;
                        if b.entity != view.entity {
                            return Err(Error::Integrity);
                        }
                        point.validate_block(&b, |stroke| {
                            let json: String = db.query_row(
                                "SELECT payload FROM strokes WHERE id=?1",
                                [stroke.to_string()],
                                |r| r.get(0),
                            )?;
                            Ok(serde_json::from_str::<Stroke>(&json)?.points.len())
                        })?;
                    }
                }
                if let (Some(exercise), Some(anchor)) = (view.exercise, view.anchor) {
                    let member: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM exercise_members WHERE block=?1 AND exercise=?2)",
                    params![anchor.to_string(), exercise.to_string()], |r| r.get(0))?;
                    if !member {
                        return Err(Error::Invalid("restore view membership"));
                    }
                }
                Ok(())
            },
        )?;
        Ok(())
    }
}

// Iterative depth-first search with SQLite-owned colours and stack. Long embed
// chains never recurse on the native stack or accumulate a whole graph in Rust.
fn validate_embed_cycles(db: &Connection, control: Option<&JobControl>) -> Result<()> {
    each(db, EMBED_HOSTS, control, |r| {
        let root: String = r.get(0)?;
        let done: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM restore_validation_colours WHERE id=?1 AND colour=2)",
            [&root],
            |r| r.get(0),
        )?;
        if done {
            return Ok(());
        }
        db.execute(
            "INSERT OR REPLACE INTO restore_validation_colours VALUES(?1,1)",
            [&root],
        )?;
        db.execute(
            "INSERT INTO restore_validation_stack VALUES(0,?1,NULL)",
            [&root],
        )?;
        let mut depth = 0_i64;
        loop {
            check(control)?;
            let (id, last): (String, Option<i64>) = db.query_row(
                "SELECT id,last_edge FROM restore_validation_stack WHERE depth=?1",
                [depth],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let next: Option<(i64, String)> = if let Some(last) = last {
                db.query_row(NEXT_EDGE, params![id, last], |r| Ok((r.get(0)?, r.get(1)?)))
                    .optional()?
            } else {
                db.query_row(FIRST_EDGE, [&id], |r| Ok((r.get(0)?, r.get(1)?)))
                    .optional()?
            };
            if let Some((edge, target)) = next {
                db.execute(
                    "UPDATE restore_validation_stack SET last_edge=?1 WHERE depth=?2",
                    params![edge, depth],
                )?;
                let colour: Option<i64> = db
                    .query_row(
                        "SELECT colour FROM restore_validation_colours WHERE id=?1",
                        [&target],
                        |r| r.get(0),
                    )
                    .optional()?;
                match colour {
                    Some(1) => return Err(Error::Invalid("restore embed cycle")),
                    Some(2) => continue,
                    _ => {
                        depth = depth.checked_add(1).ok_or(Error::ResourceLimit)?;
                        db.execute(
                            "INSERT INTO restore_validation_colours VALUES(?1,1)",
                            [&target],
                        )?;
                        db.execute(
                            "INSERT INTO restore_validation_stack VALUES(?1,?2,NULL)",
                            params![depth, target],
                        )?;
                    }
                }
            } else {
                db.execute(
                    "UPDATE restore_validation_colours SET colour=2 WHERE id=?1",
                    [&id],
                )?;
                db.execute(
                    "DELETE FROM restore_validation_stack WHERE depth=?1",
                    [depth],
                )?;
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ink::{Point, Tool},
        objects::{Image, Rect},
        store::{Camera, CanvasLayer, CanvasLayers, CaretPoint, CaretTarget, SelectionBookmark},
    };
    use std::time::Duration;

    fn ink() -> Stroke {
        Stroke {
            id: Id::new(),
            capture_session: Id::new(),
            sequence: 0,
            pointer_id: 1,
            coordinate_space: "synthetic restore validation".into(),
            tool: Tool::Pen,
            style: None,
            end: Default::default(),
            points: vec![Point {
                x: 1.,
                y: 2.,
                time_us: 1,
                pressure: None,
                tilt_rad: None,
                orientation_rad: None,
                buttons: None,
            }],
        }
    }

    fn fixture() -> Store {
        let mut s = Store::memory().unwrap();
        let a = Access::public();
        let n = s.create(Kind::Note, None, "note", &a).unwrap();
        let c = s.create(Kind::Canvas, None, "canvas", &a).unwrap();
        let other = s.create(Kind::Canvas, None, "other canvas", &a).unwrap();
        let mut first = None;
        for _ in 0..2 {
            let b = s.append_ink(n.id, vec![ink()], &a).unwrap();
            let b = s
                .partition_regions(b.id, b.revision, vec![b.source_ids.clone()], true, &a)
                .unwrap();
            let b = s
                .edit_region(
                    b.id,
                    b.revision,
                    b.regions[0].id,
                    Content::Text("keep authored 1+1=3".into()),
                    &a,
                )
                .unwrap();
            first.get_or_insert(b);
        }
        let b = first.unwrap();
        let exercise = s.create_exercise(n.id, "exercise", &[b.id], &a).unwrap();
        let canvas_block = s
            .append_content(c.id, Content::Text("1+1=3".into()), &a)
            .unwrap();
        s.edit_content(
            canvas_block.id,
            canvas_block.revision,
            Content::Text("still 1+1=3".into()),
            &a,
        )
        .unwrap();
        s.create_group(c.id, &[canvas_block.id], &a).unwrap();
        let bounds = Rect {
            x: 0.,
            y: 0.,
            width: 10.,
            height: 10.,
        };
        s.set_layout(canvas_block.id, None, bounds, 0, &a).unwrap();
        s.create_embed(n.id, c.id, bounds, &a).unwrap();
        s.create_embed(c.id, other.id, bounds, &a).unwrap();
        s.set_canvas_layers(
            CanvasLayers {
                canvas: c.id,
                revision: 0,
                base_visible: true,
                layers: vec![CanvasLayer {
                    id: Id::new(),
                    name: "layer".into(),
                    visible: true,
                    blocks: vec![canvas_block.id],
                }],
            },
            0,
            &a,
        )
        .unwrap();
        let asset = s
            .import_image(n.id, "image/png", b"\x89PNG\r\n\x1a\nsynthetic", true, &a)
            .unwrap();
        s.append_content(
            n.id,
            Content::Image(Image {
                asset,
                caption: "retained image".into(),
                crop: None,
            }),
            &a,
        )
        .unwrap();
        let point = CaretPoint {
            block: b.id,
            revision: b.revision,
            region: None,
            target: CaretTarget::Ink {
                stroke: b.source_ids[0],
                sample: 0,
            },
        };
        s.save_view(
            ViewSettings {
                id: Id::new(),
                entity: n.id,
                exercise: Some(exercise.id),
                camera: Camera::default(),
                pen_width_mm: 1.,
                colour_rgba: [0, 0, 0, 255],
                finger_ink: false,
                anchor: Some(b.id),
                bookmark: Some(SelectionBookmark {
                    anchor: point.clone(),
                    focus: point,
                }),
                authoring: None,
                revision: 0,
            },
            None,
            &a,
        )
        .unwrap();
        s.add_link(n.id, c.id, Some(b.id), &a).unwrap();
        s.set_mode(n.id, crate::store::WritingMode::Assisted, &a)
            .unwrap();
        s.db.execute(
            "INSERT INTO exercise_rules VALUES(?1,'Exercise')",
            [n.id.to_string()],
        )
        .unwrap();
        s
    }

    fn no_work_tables(s: &Store) {
        let count: usize =
            s.db.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name GLOB 'restore_validation_*'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn sql_and_aggregate_validators_agree_on_rich_graph_and_corruptions() {
        let s = fixture();
        s.validate_backup_state_aggregate(9).unwrap();
        s.validate_backup_state(9).unwrap();
        no_work_tables(&s);
        // Each mutation preserves SQL syntax/schema, targeting semantic invariants.
        let cases = [
            ("entity identity", "UPDATE entities SET payload=json_set(payload,'$.id','00000000-0000-0000-0000-000000000000')"),
            ("entity title", "UPDATE entities SET payload=json_set(payload,'$.title',replace(hex(zeroblob(4097)),'00','x'))"),
            ("surface parent", "UPDATE entities SET parent=id,payload=json_set(payload,'$.parent',id) WHERE json_extract(payload,'$.kind')='Note'"),
            ("entity revision", "UPDATE entities SET payload=json_set(payload,'$.revision',-1)"),
            ("block identity", "UPDATE blocks SET payload=json_set(payload,'$.id','00000000-0000-0000-0000-000000000000')"),
            ("block owner", "UPDATE blocks SET payload=json_set(payload,'$.entity','00000000-0000-0000-0000-000000000000')"),
            ("block transform", "UPDATE blocks SET payload=json_set(payload,'$.transform.a',0)"),
            ("stroke digest", "UPDATE strokes SET digest='not a digest'"),
            ("stroke identity", "UPDATE strokes SET payload=json_set(payload,'$.id','00000000-0000-0000-0000-000000000000'),digest='bad'"),
            ("stroke membership", "UPDATE blocks SET payload=json_set(payload,'$.source_ids[0]','00000000-0000-0000-0000-000000000000') WHERE json_array_length(payload,'$.source_ids')>0"),
            ("history revision", "UPDATE history SET payload=json_set(payload,'$.revision',999999)"),
            ("history owner", "UPDATE history SET payload=json_set(payload,'$.entity','00000000-0000-0000-0000-000000000000')"),
            ("history active time", "UPDATE history SET active_ms=-1"),
            ("region alias", "UPDATE blocks SET payload=json_set(payload,'$.regions[0].id',id) WHERE json_array_length(payload,'$.regions')>0"),
            ("region owner", "UPDATE blocks SET payload=json_set(payload,'$.regions[0].id','00000000-0000-0000-0000-000000000001') WHERE json_array_length(payload,'$.regions')>0"),
            ("exercise title", "UPDATE exercises SET payload=json_set(payload,'$.title','')"),
            ("exercise owner", "UPDATE exercises SET payload=json_set(payload,'$.master','00000000-0000-0000-0000-000000000000')"),
            ("membership revision", "UPDATE membership_versions SET revision=0"),
            ("membership absent revision", "DELETE FROM membership_versions"),
            ("exercise keyword", "UPDATE exercise_rules SET keyword='   '"),
            ("mode", "UPDATE modes SET mode='\"unknown\"'"),
            ("link alias", "UPDATE links SET id=(SELECT id FROM blocks LIMIT 1)"),
            ("link anchor", "UPDATE links SET anchor=(SELECT id FROM blocks WHERE entity=links.target LIMIT 1)"),
            ("image digest", "UPDATE image_assets SET digest='bad'"),
            ("image MIME", "UPDATE image_assets SET mime='image/gif'"),
            ("image signature", "UPDATE image_assets SET bytes=zeroblob(16)"),
            ("layout bounds", "UPDATE layouts SET payload=json_set(payload,'$.bounds.width',-1)"),
            ("empty group", "UPDATE object_groups SET payload=json_set(payload,'$.members',json('[]'))"),
            ("group member", "UPDATE object_groups SET payload=json_set(payload,'$.members[0]','00000000-0000-0000-0000-000000000000')"),
            ("embed identity", "UPDATE embeds SET payload=json_set(payload,'$.id','00000000-0000-0000-0000-000000000000')"),
            ("embed self cycle", "UPDATE embeds SET source=host,payload=json_set(payload,'$.source',host) WHERE host IN (SELECT id FROM entities WHERE json_extract(payload,'$.kind')='Canvas')"),
            ("embed camera", "UPDATE embeds SET payload=json_set(payload,'$.camera.zoom',0)"),
            ("view width", "UPDATE view_settings SET payload=json_set(payload,'$.pen_width_mm',0)"),
            ("view anchor", "UPDATE view_settings SET payload=json_set(payload,'$.anchor','00000000-0000-0000-0000-000000000000')"),
            ("bookmark sample", "UPDATE view_settings SET payload=json_set(payload,'$.bookmark.anchor.target.Ink.sample',99)"),
            ("layer owner", "UPDATE canvas_layers SET payload=json_set(payload,'$.canvas','00000000-0000-0000-0000-000000000000')"),
            ("layer alias", "UPDATE canvas_layers SET payload=json_set(payload,'$.layers[0].id',canvas)"),
            ("layer member", "UPDATE canvas_layers SET payload=json_set(payload,'$.layers[0].blocks[0]','00000000-0000-0000-0000-000000000000')"),
        ];
        for (label, sql) in cases {
            s.db.execute_batch("SAVEPOINT corruption").unwrap();
            s.db.execute_batch(sql).unwrap();
            assert!(
                s.validate_backup_state_aggregate(9).is_err(),
                "oracle accepted {label}"
            );
            assert!(s.validate_backup_state(9).is_err(), "SQL accepted {label}");
            no_work_tables(&s);
            s.db.execute_batch("ROLLBACK TO corruption; RELEASE corruption")
                .unwrap();
        }
        s.validate_backup_state(9).unwrap();
    }

    #[test]
    fn cancellation_inside_semantic_validation_removes_work_indexes() {
        let s = fixture();
        let control = JobControl::new(Duration::from_secs(300)).unwrap();
        CANCEL_AFTER.with(|n| n.set(Some(15)));
        let result = s.validate_backup_state_controlled(9, Some(&control));
        CANCEL_AFTER.with(|n| n.set(None));
        assert!(matches!(result, Err(Error::Cancelled)));
        no_work_tables(&s);
        s.validate_backup_state(9).unwrap();
    }

    #[test]
    fn checkpoint_checks_busy_readers_and_keeps_sqlite_in_charge_of_stage_io() {
        assert!(matches!(
            super::super::streaming::checkpoint_stage(&Connection::open_in_memory().unwrap()),
            Err(Error::Conflict)
        ));
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("private-stage.db");
        let mut stage = Store::open(&path).unwrap();
        let public = Access::public();
        let first = stage
            .create(Kind::Note, None, "before checkpoint", &public)
            .unwrap();
        let mut reader = Connection::open(&path).unwrap();
        let snapshot = reader.transaction().unwrap();
        let count: i64 = snapshot
            .query_row("SELECT count(*) FROM entities", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        stage
            .create(Kind::Note, None, "after reader snapshot", &public)
            .unwrap();
        stage.db.busy_timeout(Duration::ZERO).unwrap();
        assert!(matches!(
            super::super::streaming::checkpoint_stage(&stage.db),
            Err(Error::Conflict)
        ));
        snapshot.rollback().unwrap();
        drop(reader);
        super::super::streaming::checkpoint_stage(&stage.db).unwrap();
        assert_eq!(
            stage.entity(first.id, &public).unwrap().title,
            "before checkpoint"
        );
        stage
            .create(Kind::Note, None, "writing still works", &public)
            .unwrap();
        stage.check_integrity().unwrap();
    }

    #[test]
    fn source_grant_retirement_is_incremental_and_cancellation_rolls_back() {
        let mut s = Store::memory().unwrap();
        let public = Access::public();
        let note = s
            .create(Kind::Note, None, "source relationships", &public)
            .unwrap();
        for index in 0..130 {
            let source = s
                .link_pdf(
                    note.id,
                    &format!("file://synthetic/{index}"),
                    &"0".repeat(64),
                    &public,
                )
                .unwrap();
            if index % 2 == 0 {
                s.revoke_source(source.id, &public).unwrap();
            }
        }
        let snapshot = || -> Vec<(String, String)> {
            s.db.prepare("SELECT id,payload FROM sources ORDER BY rowid")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap()
        };
        let before = snapshot();
        let library = Store::setting(&s.db, "library_id").unwrap();
        let control = JobControl::new(Duration::from_secs(300)).unwrap();
        CANCEL_AFTER.with(|n| n.set(Some(7)));
        let result = s.retire_restored_runtime_state(Some(&control));
        CANCEL_AFTER.with(|n| n.set(None));
        assert!(matches!(result, Err(Error::Cancelled)));
        let after: Vec<(String, String)> =
            s.db.prepare("SELECT id,payload FROM sources ORDER BY rowid")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap();
        assert_eq!(after, before);
        assert!(s.db.is_autocommit());
        assert_eq!(Store::setting(&s.db, "library_id").unwrap(), library);
        s.retire_restored_runtime_state(None).unwrap();
        assert_ne!(Store::setting(&s.db, "library_id").unwrap(), library);
        assert_eq!(Store::setting(&s.db, "restore_origin").unwrap(), library);
        let mut q =
            s.db.prepare("SELECT payload FROM sources ORDER BY rowid")
                .unwrap();
        let rows = q.query_map([], |r| r.get::<_, String>(0)).unwrap();
        for ((_, old), new) in before.into_iter().zip(rows) {
            let mut expected: Source = serde_json::from_str(&old).unwrap();
            if expected.availability != Availability::Revoked {
                expected.availability = Availability::Missing;
            }
            assert_eq!(
                serde_json::from_str::<Source>(&new.unwrap()).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn legacy_restore_worker_cancellation_reaches_semantic_validation() {
        struct Auth;
        impl crate::security::Authenticator for Auth {
            fn verify(&mut self, _: Id, _: &BTreeSet<Scope>) -> bool {
                true
            }
        }
        let source = fixture();
        let public = Access::public();
        let plan = source
            .prepare_backup(
                BackupOptions {
                    include_original_ink: true,
                    include_authorised_source_assets: false,
                },
                &public,
            )
            .unwrap();
        let archive = source
            .finish_backup(
                plan,
                "Synthetic deliberate legacy backup password",
                &mut Auth,
                true,
            )
            .unwrap();
        let stage = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(stage.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let control = JobControl::new(Duration::from_secs(300)).unwrap();
        CANCEL_AFTER.with(|n| n.set(Some(15)));
        let result = Store::restore_any_backup(
            archive.bytes.as_slice(),
            crypto::Credential::Recovery(&archive.recovery_key),
            64 * 1024 * 1024,
            stage.path(),
            &control,
        );
        CANCEL_AFTER.with(|n| n.set(None));
        assert!(matches!(result, Err(Error::Cancelled)));
        assert_eq!(std::fs::read_dir(stage.path()).unwrap().count(), 0);
        no_work_tables(&source);
        source.validate_backup_state(9).unwrap();
    }

    #[test]
    fn unreferenced_raw_ink_is_rejected_instead_of_silently_omitted() {
        let s = fixture();
        let stroke = ink();
        let json = serde_json::to_string(&stroke).unwrap();
        s.db.execute(
            "INSERT INTO strokes SELECT ?1,block,?2,?3 FROM strokes LIMIT 1",
            params![stroke.id.to_string(), json, crate::digest(json.as_bytes())],
        )
        .unwrap();
        // Explicitly stronger than the old aggregate reconstruction.
        s.validate_backup_state_aggregate(9).unwrap();
        assert!(matches!(
            s.validate_backup_state(9),
            Err(Error::Invalid("restore orphan stroke"))
        ));
        no_work_tables(&s);
    }

    #[test]
    fn schema_feature_gates_match_aggregate_oracle() {
        let s = fixture();
        for version in 1..=12 {
            assert_eq!(
                s.validate_backup_state(version).is_ok(),
                s.validate_backup_state_aggregate(version).is_ok(),
                "version {version}"
            );
            no_work_tables(&s);
        }
    }

    #[test]
    fn containment_depth_cycles_and_retired_entities_keep_the_same_rules() {
        let mut s = Store::memory().unwrap();
        let a = Access::public();
        let outer = s.create(Kind::Module, None, "outer", &a).unwrap();
        let inner = s.create(Kind::Module, Some(outer.id), "inner", &a).unwrap();
        let project = s
            .create(Kind::Project, Some(inner.id), "project", &a)
            .unwrap();
        let notebook = s
            .create(Kind::Notebook, Some(project.id), "notebook", &a)
            .unwrap();
        let note = s.create(Kind::Note, Some(notebook.id), "note", &a).unwrap();
        s.append_content(note.id, Content::Text("literal 1+1=3".into()), &a)
            .unwrap();
        // Whole backups retain tombstones and Trash without importing the subset's
        // requirement that every entity be live.
        s.db.execute("UPDATE entities SET payload=json_set(payload,'$.trashed',json('true'),'$.purged',json('true')) WHERE id=?1", [note.id.to_string()]).unwrap();
        s.validate_backup_state_aggregate(9).unwrap();
        s.validate_backup_state(9).unwrap();
        for sql in [
            "UPDATE entities SET parent=id,payload=json_set(payload,'$.parent',id) WHERE json_extract(payload,'$.title')='outer'",
            "UPDATE entities SET payload=json_set(payload,'$.kind','Module') WHERE json_extract(payload,'$.title')='project'",
            "UPDATE entities SET payload=json_set(payload,'$.kind','Note') WHERE json_extract(payload,'$.title')='notebook'",
        ] {
            s.db.execute_batch("SAVEPOINT ancestry").unwrap();
            s.db.execute_batch(sql).unwrap();
            assert!(s.validate_backup_state_aggregate(9).is_err());
            assert!(s.validate_backup_state(9).is_err());
            no_work_tables(&s);
            s.db.execute_batch("ROLLBACK TO ancestry; RELEASE ancestry").unwrap();
        }
    }

    #[test]
    fn whole_library_stream_restore_exceeds_subset_entity_block_and_exercise_caps() {
        use crate::security::Authenticator;
        struct Auth;
        impl Authenticator for Auth {
            fn verify(&mut self, _: Id, _: &BTreeSet<Scope>) -> bool {
                true
            }
        }
        let root = tempfile::tempdir().unwrap();
        let mut s = Store::open(root.path().join("large-source.db")).unwrap();
        let a = Access::public();
        let seed = s
            .create(Kind::Note, None, "synthetic scale case", &a)
            .unwrap();
        let template = s
            .append_content(seed.id, Content::Text("1+1=3".into()), &a)
            .unwrap();
        {
            let tx = s.db.transaction().unwrap();
            let mut entities = tx
                .prepare("INSERT INTO entities VALUES(?1,NULL,?2)")
                .unwrap();
            for _ in 0..10_000 {
                let mut e = seed.clone();
                e.id = Id::new();
                entities
                    .execute(params![
                        e.id.to_string(),
                        serde_json::to_string(&e).unwrap()
                    ])
                    .unwrap();
            }
            let mut blocks = tx
                .prepare("INSERT INTO blocks VALUES(?1,?2,?3,?4)")
                .unwrap();
            for ordinal in 1..=100_000 {
                let mut b = template.clone();
                b.id = Id::new();
                blocks
                    .execute(params![
                        b.id.to_string(),
                        seed.id.to_string(),
                        ordinal,
                        serde_json::to_string(&b).unwrap()
                    ])
                    .unwrap();
            }
            let mut exercises = tx
                .prepare("INSERT INTO exercises VALUES(?1,?2,NULL,?3)")
                .unwrap();
            for _ in 0..10_001 {
                let e = Exercise {
                    id: Id::new(),
                    master: seed.id,
                    heading: None,
                    title: "synthetic exercise".into(),
                    revision: 0,
                    locked: false,
                    visible: true,
                    created_ms: 0,
                };
                exercises
                    .execute(params![
                        e.id.to_string(),
                        seed.id.to_string(),
                        serde_json::to_string(&e).unwrap()
                    ])
                    .unwrap();
            }
            drop((entities, blocks, exercises));
            tx.commit().unwrap();
        }
        let job = JobControl::new(Duration::from_secs(600)).unwrap();
        let plan = s
            .prepare_stream_backup(
                BackupOptions {
                    include_original_ink: true,
                    include_authorised_source_assets: false,
                },
                &a,
            )
            .unwrap();
        let archive = std::fs::File::create(root.path().join("encrypted.backup")).unwrap();
        let (archive, key) = s
            .finish_stream_backup(
                plan,
                "Synthetic deliberate scale test backup password",
                &mut Auth,
                true,
                archive,
                &job,
            )
            .unwrap();
        drop(archive);
        let stage = tempfile::Builder::new()
            .prefix("private-scale-restore")
            .tempdir()
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(stage.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let restored = Store::restore_stream_backup(
            std::fs::File::open(root.path().join("encrypted.backup")).unwrap(),
            crypto::Credential::Recovery(&key),
            256 * 1024 * 1024,
            stage.path(),
            &job,
        )
        .unwrap();
        let store = restored.store();
        for (table, expected) in [
            ("entities", 10_001),
            ("blocks", 100_001),
            ("exercises", 10_001),
        ] {
            let count: i64 = store
                .db
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, expected, "{table}");
        }
        assert_eq!(
            store.block(template.id, &a).unwrap().content,
            Content::Text("1+1=3".into())
        );
        no_work_tables(store);
        drop(restored);
        assert_eq!(std::fs::read_dir(stage.path()).unwrap().count(), 0);
    }

    #[test]
    fn whole_library_images_exceed_aggregate_subset_limit_without_collecting_bytes() {
        use sha2::{Digest, Sha256};
        let root = tempfile::tempdir().unwrap();
        let mut s = Store::open(root.path().join("images.db")).unwrap();
        let a = Access::public();
        let owner = s
            .create(Kind::Canvas, None, "large synthetic image assets", &a)
            .unwrap();
        // Three individually permitted image sizes exceed the 64 MiB aggregate
        // subset cap. Only signatures are meaningful; this is no decode benchmark.
        let size = 24 * 1024 * 1024;
        let signature = b"\x89PNG\r\n\x1a\n";
        let zeros = [0_u8; 65536];
        let mut h = Sha256::new();
        h.update(signature);
        let mut remaining = size - signature.len();
        while remaining > 0 {
            let n = remaining.min(zeros.len());
            h.update(&zeros[..n]);
            remaining -= n;
        }
        let digest = format!("{:x}", h.finalize());
        for _ in 0..3 {
            let id = Id::new();
            s.db.execute(
                "INSERT INTO image_assets VALUES(?1,?2,'image/png',zeroblob(?3),?4)",
                params![id.to_string(), owner.id.to_string(), size, digest],
            )
            .unwrap();
            let rowid = s.db.last_insert_rowid();
            let mut blob =
                s.db.blob_open(
                    rusqlite::DatabaseName::Main,
                    "image_assets",
                    "bytes",
                    rowid,
                    false,
                )
                .unwrap();
            blob.write_at(signature, 0).unwrap();
            drop(blob);
            s.append_content(
                owner.id,
                Content::Image(Image {
                    asset: id,
                    caption: "synthetic".into(),
                    crop: None,
                }),
                &a,
            )
            .unwrap();
        }
        s.validate_backup_state(9).unwrap();
        no_work_tables(&s);
        s.db.execute("UPDATE image_assets SET digest='wrong' WHERE rowid=(SELECT max(rowid) FROM image_assets)", []).unwrap();
        assert!(s.validate_backup_state(9).is_err());
        no_work_tables(&s);
    }

    #[test]
    fn embed_work_queries_use_covering_primary_key_without_temporary_sorting() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(WORK_EDGES).unwrap();
        for host in ["a", "b"] {
            for edge in [-3_i64, 0, 7, 29] {
                db.execute(
                    "INSERT INTO restore_validation_edges VALUES(?1,?2,'target')",
                    params![host, edge],
                )
                .unwrap();
            }
        }
        for (sql, bindings) in [
            (EMBED_HOSTS, vec![]),
            (FIRST_EDGE, vec![rusqlite::types::Value::Text("a".into())]),
            (
                NEXT_EDGE,
                vec![
                    rusqlite::types::Value::Text("a".into()),
                    rusqlite::types::Value::Integer(0),
                ],
            ),
        ] {
            let mut q = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
            let plan = q
                .query_map(rusqlite::params_from_iter(bindings), |r| {
                    r.get::<_, String>(3)
                })
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
                .join("; ");
            assert!(!plan.contains("TEMP B-TREE"), "{sql}: {plan}");
            if sql != EMBED_HOSTS {
                assert!(
                    plan.contains("SEARCH restore_validation_edges USING PRIMARY KEY"),
                    "{plan}"
                );
            }
            if sql == NEXT_EDGE {
                assert!(plan.contains("edge>?"), "{plan}");
            }
        }
        let first: i64 = db.query_row(FIRST_EDGE, ["a"], |r| r.get(0)).unwrap();
        let next: i64 = db
            .query_row(NEXT_EDGE, params!["a", first], |r| r.get(0))
            .unwrap();
        assert_eq!((first, next), (-3, 0));
    }

    #[test]
    fn deep_embed_graph_uses_sqlite_stack_and_detects_back_edges() {
        let mut s = Store::memory().unwrap();
        let a = Access::public();
        let mut previous = s.create(Kind::Canvas, None, "root", &a).unwrap();
        let root = previous.id;
        {
            let tx = s.db.transaction().unwrap();
            let mut entities = tx
                .prepare("INSERT INTO entities VALUES(?1,NULL,?2)")
                .unwrap();
            let mut edges = tx
                .prepare("INSERT INTO embeds VALUES(?1,?2,?3,?4)")
                .unwrap();
            for _ in 0..2048 {
                let mut next = previous.clone();
                next.id = Id::new();
                entities
                    .execute(params![
                        next.id.to_string(),
                        serde_json::to_string(&next).unwrap()
                    ])
                    .unwrap();
                let edge = Embed {
                    id: Id::new(),
                    host: previous.id,
                    source: next.id,
                    camera: Camera::default(),
                    frame: Rect {
                        x: 0.,
                        y: 0.,
                        width: 1.,
                        height: 1.,
                    },
                    pinned: false,
                    visible_layers: None,
                    revision: 0,
                };
                edges
                    .execute(params![
                        edge.id.to_string(),
                        edge.host.to_string(),
                        edge.source.to_string(),
                        serde_json::to_string(&edge).unwrap()
                    ])
                    .unwrap();
                previous = next;
            }
            drop((entities, edges));
            tx.commit().unwrap();
        }
        s.validate_backup_state(9).unwrap();
        no_work_tables(&s);
        let edge = Embed {
            id: Id::new(),
            host: previous.id,
            source: root,
            camera: Camera::default(),
            frame: Rect {
                x: 0.,
                y: 0.,
                width: 1.,
                height: 1.,
            },
            pinned: false,
            visible_layers: None,
            revision: 0,
        };
        s.db.execute(
            "INSERT INTO embeds VALUES(?1,?2,?3,?4)",
            params![
                edge.id.to_string(),
                edge.host.to_string(),
                edge.source.to_string(),
                serde_json::to_string(&edge).unwrap()
            ],
        )
        .unwrap();
        assert!(matches!(
            s.validate_backup_state(9),
            Err(Error::Invalid("restore embed cycle"))
        ));
        no_work_tables(&s);
    }
}
