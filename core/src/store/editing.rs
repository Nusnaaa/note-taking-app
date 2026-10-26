//! Durable, transactional authoring undo. Revisions always advance; ink observations,
//! locks, external grants and model state are not reverted by an editing action.
use super::*;
use crate::ink::Transform;
use rusqlite::params;
use serde_json::Value;

fn row(json: Option<&str>) -> Result<Option<Vec<Value>>> {
    json.map(serde_json::from_str)
        .transpose()
        .map_err(Into::into)
}
fn string(row: &[Value], index: usize) -> Result<&str> {
    row.get(index)
        .and_then(Value::as_str)
        .ok_or(Error::Integrity)
}
fn payload(row: &[Value]) -> Result<Value> {
    serde_json::from_str(string(
        row,
        row.len().checked_sub(1).ok_or(Error::Integrity)?,
    )?)
    .map_err(Into::into)
}
fn comparable(mut value: Value) -> Value {
    if let Some(o) = value.as_object_mut() {
        o.remove("revision");
        o.remove("locked");
    }
    value
}

impl Store {
    /// Backup rows are untrusted even after envelope authentication. Check both
    /// SQL identity columns and the embedded historical payload before replay.
    pub(crate) fn validate_edit_journal(
        &self,
        control: Option<&crate::processing::JobControl>,
    ) -> Result<()> {
        self.validate_edit_links(control)?;
        let mut q = self.db.prepare("SELECT o.entity,c.table_name,c.item,c.before_row,c.after_row FROM edit_changes c JOIN edit_operations o ON o.id=c.operation")?;
        for record in q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })? {
            if let Some(control) = control {
                control.check()?;
            }
            let (entity, table, item, before, after) = record?;
            let owner = Id::parse(&entity)?;
            let id = Id::parse(&item)?;
            if before.is_none() && after.is_none() {
                return Err(Error::Integrity);
            }
            for value in [before, after].into_iter().flatten() {
                let fields: Vec<Value> = serde_json::from_str(&value)?;
                let width = match table.as_str() {
                    "blocks" | "embeds" => 4,
                    "object_groups" => 3,
                    "layouts" | "canvas_layers" => 2,
                    _ => return Err(Error::Integrity),
                };
                if fields.len() != width || string(&fields, 0)? != item {
                    return Err(Error::Integrity);
                }
                match table.as_str() {
                    "blocks" => {
                        let b: Block = serde_json::from_value(payload(&fields)?)?;
                        b.validate()?;
                        if b.id != id
                            || b.entity != owner
                            || string(&fields, 1)? != entity
                            || fields[2].as_i64().is_none_or(|n| n < 0)
                        {
                            return Err(Error::Integrity);
                        }
                    }
                    "object_groups" => {
                        let g: ObjectGroup = serde_json::from_value(payload(&fields)?)?;
                        if g.id != id
                            || g.entity != owner
                            || string(&fields, 1)? != entity
                            || g.members.is_empty()
                            || g.members.len() > 4096
                            || g.members
                                .iter()
                                .collect::<std::collections::BTreeSet<_>>()
                                .len()
                                != g.members.len()
                        {
                            return Err(Error::Integrity);
                        }
                    }
                    "embeds" => {
                        let e: Embed = serde_json::from_value(payload(&fields)?)?;
                        e.validate_layers()?;
                        e.camera.validate()?;
                        e.frame.validate()?;
                        if e.id != id
                            || e.host != owner
                            || string(&fields, 1)? != entity
                            || string(&fields, 2)? != e.source.to_string()
                        {
                            return Err(Error::Integrity);
                        }
                    }
                    "canvas_layers" => {
                        let state: CanvasLayers = serde_json::from_value(payload(&fields)?)?;
                        state.validate()?;
                        if state.canvas != owner || state.canvas != id {
                            return Err(Error::Integrity);
                        }
                    }
                    "layouts" => {
                        let layout: Layout = serde_json::from_value(payload(&fields)?)?;
                        layout.bounds.validate()?;
                    }
                    _ => return Err(Error::Integrity),
                }
            }
        }
        Ok(())
    }
    pub fn undo(&mut self, entity: Id, access: &Access) -> Result<i64> {
        self.replay_edit(entity, false, access)
    }
    pub fn redo(&mut self, entity: Id, access: &Access) -> Result<i64> {
        self.replay_edit(entity, true, access)
    }
    pub fn edit_history_depth(&self, entity: Id, access: &Access) -> Result<(u64, u64)> {
        Self::authorise(&self.db, entity, access, false)?;
        Ok(self.db.query_row("SELECT COALESCE(SUM(undone=0 AND id>COALESCE((SELECT MAX(o.id) FROM edit_operations o JOIN edit_links l ON l.operation=o.id WHERE o.entity=?1 AND o.undone=0 AND l.retired=1),0)),0),COALESCE(SUM(undone=1 AND id<COALESCE((SELECT MIN(o.id) FROM edit_operations o JOIN edit_links l ON l.operation=o.id WHERE o.entity=?1 AND o.undone=1 AND l.retired=1),9223372036854775807)),0) FROM edit_operations WHERE entity=?1", [entity.to_string()], |r| Ok((r.get(0)?,r.get(1)?)))?)
    }
    fn replay_edit(&mut self, entity: Id, redo: bool, access: &Access) -> Result<i64> {
        self.write(|db| {
            Self::authorise(db, entity, access, false)?;
            let sql = if redo {
                "SELECT MIN(id) FROM edit_operations WHERE entity=?1 AND undone=1"
            } else {
                "SELECT MAX(id) FROM edit_operations WHERE entity=?1 AND undone=0"
            };
            let operation: Option<i64> = db.query_row(sql, [entity.to_string()], |r| r.get(0))?;
            let operation = operation.ok_or(Error::Unavailable)?;
            let mut operations = vec![(operation, entity)];
            let link: Option<(i64, bool)> = db
                .query_row(
                    "SELECT peer,retired FROM edit_links WHERE operation=?1",
                    [operation],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((peer, retired)) = link {
                if retired {
                    return Err(Error::Unavailable);
                }
                let (owner, undone, tick): (String, bool, i64) = db.query_row(
                    "SELECT entity,undone,active_ms FROM edit_operations WHERE id=?1",
                    [peer],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
                let own_tick: i64 = db.query_row(
                    "SELECT active_ms FROM edit_operations WHERE id=?1",
                    [operation],
                    |r| r.get(0),
                )?;
                if own_tick != tick
                    || !db
                        .prepare("SELECT 1 FROM edit_changes WHERE operation=?1")?
                        .exists([operation])?
                    || !db
                        .prepare("SELECT 1 FROM edit_changes WHERE operation=?1")?
                        .exists([peer])?
                {
                    return Err(Error::Integrity);
                }
                let owner = Id::parse(&owner)?;
                Self::authorise(db, owner, access, false)?;
                let reciprocal: Option<(i64, bool)> = db
                    .query_row(
                        "SELECT peer,retired FROM edit_links WHERE operation=?1",
                        [peer],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                let head: Option<i64> = db.query_row(sql, [owner.to_string()], |r| r.get(0))?;
                if reciprocal != Some((operation, false)) || owner == entity {
                    return Err(Error::Integrity);
                }
                if undone != redo || head != Some(peer) {
                    return Err(Error::Conflict);
                }
                operations.push((peer, owner));
            }
            // All mutations remain inside this single transaction. Each replay
            // rechecks current blocks and inherited protection before replacing.
            for (id, owner) in operations {
                Self::replay_operation(db, owner, id, redo, access)?;
            }
            Ok(operation)
        })
    }

    fn replay_operation(
        db: &Connection,
        entity: Id,
        operation: i64,
        redo: bool,
        access: &Access,
    ) -> Result<()> {
        let changes = {
            let mut q=db.prepare("SELECT table_name,item,before_row,after_row FROM edit_changes WHERE operation=?1 ORDER BY CASE table_name WHEN 'blocks' THEN 0 ELSE 1 END,item")?;
            let rows = q
                .query_map([operation], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        for (table, key, before, after) in changes {
            let before = row(before.as_deref())?;
            let after = row(after.as_deref())?;
            let (target, expected) = if redo {
                (&after, &before)
            } else {
                (&before, &after)
            };
            let id = Id::parse(&key)?;
            match table.as_str() {
                "blocks" => {
                    let current = Self::read_block(db, id, access)?;
                    if current.entity != entity {
                        return Err(Error::Integrity);
                    }
                    if let Some(expected) = expected {
                        let expected_block: Block = serde_json::from_value(payload(expected)?)?;
                        if !current.source_ids.starts_with(&expected_block.source_ids) {
                            return Err(Error::Conflict);
                        }
                        let mut compared = current.clone();
                        // Undo retains observations appended by the operation. Redo
                        // compares presentation against its old state, without
                        // demanding deletion of that append-only source suffix.
                        compared.source_ids = expected_block.source_ids.clone();
                        if comparable(serde_json::to_value(&compared)?)
                            != comparable(serde_json::to_value(&expected_block)?)
                        {
                            return Err(Error::Conflict);
                        }
                    } else if current.visible {
                        return Err(Error::Conflict);
                    }
                    let mut desired: Block = if let Some(target) = target {
                        serde_json::from_value(payload(target)?)?
                    } else {
                        let mut b = current.clone();
                        b.visible = false;
                        b
                    };
                    if desired.entity != entity || desired.id != id {
                        return Err(Error::Integrity);
                    }
                    desired.locked = current.locked;
                    desired.source_ids = current.source_ids;
                    desired.revision = current
                        .revision
                        .checked_add(1)
                        .ok_or(Error::ResourceLimit)?;
                    if let Some(target) = target {
                        let ordinal = target
                            .get(2)
                            .and_then(Value::as_i64)
                            .ok_or(Error::Integrity)?;
                        db.execute(
                            "UPDATE blocks SET ordinal=?1 WHERE id=?2",
                            params![ordinal, key],
                        )?;
                    }
                    Self::persist_block(db, &desired)?;
                }
                "canvas_layers" => {
                    if id != entity {
                        return Err(Error::Integrity);
                    }
                    Self::check_canvas(db, entity, access)?;
                    Self::require_content_access(db, entity, access)?;
                    let current = Self::raw_canvas_layers(db, entity)?;
                    let expected_state: CanvasLayers = if let Some(expected) = expected {
                        serde_json::from_value(payload(expected)?)?
                    } else {
                        CanvasLayers::implicit(entity)
                    };
                    if comparable(serde_json::to_value(&current)?)
                        != comparable(serde_json::to_value(expected_state)?)
                    {
                        return Err(Error::Conflict);
                    }
                    let mut desired: CanvasLayers = if let Some(target) = target {
                        serde_json::from_value(payload(target)?)?
                    } else {
                        CanvasLayers::implicit(entity)
                    };
                    if desired.canvas != entity {
                        return Err(Error::Integrity);
                    }
                    Self::check_layer_references(db, &current, access)?;
                    Self::check_layer_references(db, &desired, access)?;
                    desired.revision = current
                        .revision
                        .checked_add(1)
                        .filter(|r| *r <= i64::MAX as u64)
                        .ok_or(Error::ResourceLimit)?;
                    db.execute("INSERT INTO canvas_layers VALUES(?1,?2) ON CONFLICT(canvas) DO UPDATE SET payload=excluded.payload",params![entity.to_string(),serde_json::to_string(&desired)?])?;
                    Self::touch_master(db, entity)?;
                }
                "layouts" | "object_groups" | "embeds" => {
                    let key_column = if table == "layouts" { "block" } else { "id" };
                    let current: Option<String> = db
                        .query_row(
                            &format!("SELECT payload FROM {table} WHERE {key_column}=?1"),
                            [&key],
                            |r| r.get(0),
                        )
                        .optional()?;
                    if let Some(expected) = expected {
                        if current
                            .as_deref()
                            .map(serde_json::from_str::<Value>)
                            .transpose()?
                            .map(comparable)
                            != Some(comparable(payload(expected)?))
                        {
                            return Err(Error::Conflict);
                        }
                    } else if current.is_some() {
                        return Err(Error::Conflict);
                    }
                    // Check every current and historical referent, including newly locked members.
                    for json in [
                        current
                            .as_deref()
                            .map(serde_json::from_str::<Value>)
                            .transpose()?,
                        target.as_ref().map(|r| payload(r)).transpose()?,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        match table.as_str() {
                            "layouts" => {
                                if Self::read_block(db, id, access)?.entity != entity {
                                    return Err(Error::Integrity);
                                }
                            }
                            "object_groups" => {
                                let g: ObjectGroup = serde_json::from_value(json)?;
                                if g.entity != entity {
                                    return Err(Error::Integrity);
                                }
                                for b in g.members {
                                    Self::read_block(db, b, access)?;
                                }
                            }
                            _ => {
                                let e: Embed = serde_json::from_value(json)?;
                                if e.host != entity {
                                    return Err(Error::Integrity);
                                }
                                Self::authorise(db, e.source, access, false)?;
                            }
                        }
                    }
                    if let Some(target) = target {
                        let mut value = payload(target)?;
                        let revision = current
                            .as_deref()
                            .map(serde_json::from_str::<Value>)
                            .transpose()?
                            .and_then(|v| v["revision"].as_u64())
                            .unwrap_or(0)
                            + 1;
                        value["revision"] = revision.into();
                        let mut values = target.clone();
                        let last = values.len() - 1;
                        values[last] = serde_json::to_string(&value)?.into();
                        let values = values
                            .into_iter()
                            .map(|v| v.as_str().map(|s| s.to_owned()).ok_or(Error::Integrity))
                            .collect::<Result<Vec<_>>>()?;
                        let placeholders = (1..=values.len())
                            .map(|i| format!("?{i}"))
                            .collect::<Vec<_>>()
                            .join(",");
                        db.execute(&format!("INSERT INTO {table} VALUES({placeholders}) ON CONFLICT({key_column}) DO UPDATE SET payload=excluded.payload"), rusqlite::params_from_iter(values))?;
                    } else {
                        db.execute(&format!("DELETE FROM {table} WHERE {key_column}=?1"), [key])?;
                    }
                }
                _ => return Err(Error::Integrity),
            }
        }
        db.execute(
            "UPDATE edit_operations SET undone=?1 WHERE id=?2",
            params![!redo, operation],
        )?;
        Ok(())
    }

    /// Atomic lasso/group edit with one undo action and revision checks for every item.
    pub fn transform_selection(
        &mut self,
        selected: &[(Id, u64)],
        transform: Transform,
        access: &Access,
    ) -> Result<Vec<Block>> {
        transform.validate()?;
        if selected.is_empty()
            || selected.len() > 4096
            || selected
                .iter()
                .map(|v| v.0)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != selected.len()
        {
            return Err(Error::Invalid("selection"));
        }
        let entity = self.block(selected[0].0, access)?.entity;
        self.write_for(entity, |db| {
            let mut result = Vec::new();
            for &(id, revision) in selected {
                let mut b = Self::read_block(db, id, access)?;
                if b.entity != entity || b.revision != revision {
                    return Err(Error::Conflict);
                }
                b.transform = transform;
                b.revision += 1;
                Self::persist_block(db, &b)?;
                result.push(b);
            }
            Ok(result)
        })
    }
}
