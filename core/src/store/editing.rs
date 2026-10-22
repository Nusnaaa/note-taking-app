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
    pub fn undo(&mut self, entity: Id, access: &Access) -> Result<i64> {
        self.replay_edit(entity, false, access)
    }
    pub fn redo(&mut self, entity: Id, access: &Access) -> Result<i64> {
        self.replay_edit(entity, true, access)
    }
    pub fn edit_history_depth(&self, entity: Id, access: &Access) -> Result<(u64, u64)> {
        Self::authorise(&self.db, entity, access, false)?;
        Ok(self.db.query_row("SELECT COALESCE(SUM(undone=0),0),COALESCE(SUM(undone=1),0) FROM edit_operations WHERE entity=?1", [entity.to_string()], |r| Ok((r.get(0)?,r.get(1)?)))?)
    }
    fn replay_edit(&mut self, entity: Id, redo: bool, access: &Access) -> Result<i64> {
        self.write(|db| {
            Self::authorise(db, entity, access, false)?;
            let sql = if redo {"SELECT MIN(id) FROM edit_operations WHERE entity=?1 AND undone=1"} else {"SELECT MAX(id) FROM edit_operations WHERE entity=?1 AND undone=0"};
            let operation: Option<i64> = db.query_row(sql,[entity.to_string()],|r|r.get(0))?;
            let operation = operation.ok_or(Error::Unavailable)?;
            let changes = {
                let mut q=db.prepare("SELECT table_name,item,before_row,after_row FROM edit_changes WHERE operation=?1 ORDER BY CASE table_name WHEN 'blocks' THEN 0 ELSE 1 END,item")?;
                let rows=q.query_map([operation],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?)))?.collect::<std::result::Result<Vec<_>,_>>()?; rows
            };
            for (table,key,before,after) in changes {
                let before=row(before.as_deref())?; let after=row(after.as_deref())?;
                let (target,expected)=if redo {(&after,&before)} else {(&before,&after)};
                let id=Id::parse(&key)?;
                match table.as_str() {
                    "blocks" => {
                        let current=Self::read_block(db,id,access)?;
                        if current.entity!=entity {return Err(Error::Integrity)}
                        if let Some(expected)=expected {
                            if comparable(serde_json::to_value(&current)?)!=comparable(payload(expected)?) {return Err(Error::Conflict)}
                        } else if current.visible { return Err(Error::Conflict) }
                        let mut desired: Block=if let Some(target)=target {serde_json::from_value(payload(target)?)?} else {let mut b=current.clone(); b.visible=false; b};
                        if desired.entity!=entity || desired.id!=id {return Err(Error::Integrity)}
                        desired.locked=current.locked;
                        desired.source_ids=current.source_ids;
                        desired.revision=current.revision.checked_add(1).ok_or(Error::ResourceLimit)?;
                        if let Some(target)=target {
                            let ordinal=target.get(2).and_then(Value::as_i64).ok_or(Error::Integrity)?;
                            db.execute("UPDATE blocks SET ordinal=?1 WHERE id=?2",params![ordinal,key])?;
                        }
                        Self::persist_block(db,&desired)?;
                    }
                    "layouts" | "object_groups" | "embeds" => {
                        let key_column=if table=="layouts" {"block"} else {"id"};
                        let current:Option<String>=db.query_row(&format!("SELECT payload FROM {table} WHERE {key_column}=?1"),[&key],|r|r.get(0)).optional()?;
                        if let Some(expected)=expected {
                            if current.as_deref().map(serde_json::from_str::<Value>).transpose()?.map(comparable)!=Some(comparable(payload(expected)?)) {return Err(Error::Conflict)}
                        } else if current.is_some() {return Err(Error::Conflict)}
                        // Check every current and historical referent, including newly locked members.
                        for json in [current.as_deref().map(serde_json::from_str::<Value>).transpose()?,target.as_ref().map(|r|payload(r)).transpose()?].into_iter().flatten() {
                            match table.as_str() {
                                "layouts" => {if Self::read_block(db,id,access)?.entity!=entity {return Err(Error::Integrity)}},
                                "object_groups" => {let g:ObjectGroup=serde_json::from_value(json)?; if g.entity!=entity {return Err(Error::Integrity)} for b in g.members {Self::read_block(db,b,access)?;}},
                                _ => {let e:Embed=serde_json::from_value(json)?; if e.host!=entity {return Err(Error::Integrity)} Self::authorise(db,e.source,access,false)?;}
                            }
                        }
                        if let Some(target)=target {
                            let mut value=payload(target)?;
                            let revision=current.as_deref().map(serde_json::from_str::<Value>).transpose()?.and_then(|v|v["revision"].as_u64()).unwrap_or(0)+1;
                            value["revision"]=revision.into();
                            let mut values=target.clone(); let last=values.len()-1; values[last]=serde_json::to_string(&value)?.into();
                            let values=values.into_iter().map(|v|v.as_str().map(|s|s.to_owned()).ok_or(Error::Integrity)).collect::<Result<Vec<_>>>()?;
                            let placeholders=(1..=values.len()).map(|i|format!("?{i}")).collect::<Vec<_>>().join(",");
                            db.execute(&format!("INSERT INTO {table} VALUES({placeholders}) ON CONFLICT({key_column}) DO UPDATE SET payload=excluded.payload"), rusqlite::params_from_iter(values))?;
                        } else {db.execute(&format!("DELETE FROM {table} WHERE {key_column}=?1"),[key])?;}
                    }
                    _=>return Err(Error::Integrity)
                }
            }
            db.execute("UPDATE edit_operations SET undone=?1 WHERE id=?2",params![!redo,operation])?;
            Ok(operation)
        })
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
