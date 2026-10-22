use super::*;
use crate::now_ms;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub id: Id,
    pub origin: Id,
    pub target: Id,
    pub anchor: Option<Id>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum LinkState {
    Available(Entity),
    Unavailable,
}
#[derive(Debug, Clone, Copy)]
pub enum Order {
    Name,
    Created,
    Modified,
}

/// Do not reveal the titles or IDs of restricted referencing documents.
#[derive(Debug)]
pub struct DeletionReview {
    target: Id,
    generation: String,
    pub has_dependencies: bool,
    pub visible_dependants: Vec<Id>,
}

impl Store {
    fn check_parent(
        db: &Connection,
        kind: Kind,
        id: Id,
        parent: Option<Id>,
        access: &Access,
    ) -> Result<()> {
        if let Some(p) = parent {
            let destination = Self::authorise(db, p, access, false)?;
            if !destination.kind.accepts(kind) {
                return Err(Error::Invalid("parent kind"));
            }
            let mut cursor = Some(p);
            let mut depth = usize::from(kind == Kind::Module);
            let mut visited = BTreeSet::new();
            while let Some(item) = cursor {
                if item == id || !visited.insert(item) {
                    return Err(Error::Invalid("containment cycle"));
                }
                let e = Self::raw_entity(db, item)?;
                depth += usize::from(e.kind == Kind::Module);
                cursor = e.parent;
            }
            // Moving a Module must also account for every existing Module descendant.
            if kind == Kind::Module {
                let descendants = Self::subtree(db, id).unwrap_or_default();
                let has_submodule = descendants.iter().any(|&d| {
                    d != id && Self::raw_entity(db, d).is_ok_and(|e| e.kind == Kind::Module)
                });
                depth += usize::from(has_submodule);
            }
            if depth > 2 {
                return Err(Error::Invalid("Module depth"));
            }
        }
        Ok(())
    }
    pub(crate) fn subtree(db: &Connection, id: Id) -> Result<Vec<Id>> {
        let mut stmt=db.prepare("WITH RECURSIVE tree(id) AS (SELECT id FROM entities WHERE id=?1 UNION SELECT e.id FROM entities e JOIN tree t ON e.parent=t.id) SELECT id FROM tree")?;
        let strings = stmt
            .query_map([id.to_string()], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        strings.iter().map(|s| Id::parse(s)).collect()
    }
    pub fn create(
        &mut self,
        kind: Kind,
        parent: Option<Id>,
        title: &str,
        access: &Access,
    ) -> Result<Entity> {
        if title.len() > 4096 {
            return Err(Error::ResourceLimit);
        }
        let e = Entity {
            id: Id::new(),
            kind,
            parent,
            title: title.into(),
            created_ms: now_ms(),
            modified_ms: now_ms(),
            revision: 0,
            locked: false,
            trashed: false,
            purged: false,
        };
        self.write(|db| {
            Self::require_app(db, access)?;
            Self::check_parent(db, kind, e.id, parent, access)?;
            Self::save_entity(db, &e)?;
            // Notebook defaults are copied at creation. Linked views use the master.
            if kind == Kind::Notebook {
                db.execute(
                    "INSERT INTO modes(entity,mode) VALUES(?1,?2)",
                    rusqlite::params![e.id.to_string(), Self::setting(db, "global_mode")?],
                )?;
            }
            Ok(e.clone())
        })
    }
    pub fn entity(&self, id: Id, access: &Access) -> Result<Entity> {
        Self::authorise(&self.db, id, access, false)
    }
    pub fn rename(
        &mut self,
        id: Id,
        revision: u64,
        title: &str,
        access: &Access,
    ) -> Result<Entity> {
        if title.len() > 4096 {
            return Err(Error::ResourceLimit);
        }
        self.write_for(id, |db| {
            let mut e = Self::authorise(db, id, access, false)?;
            if e.revision != revision {
                return Err(Error::Conflict);
            }
            e.title = title.into();
            e.revision += 1;
            e.modified_ms = now_ms();
            Self::save_entity(db, &e)?;
            Ok(e)
        })
    }
    pub fn move_entity(
        &mut self,
        id: Id,
        revision: u64,
        parent: Option<Id>,
        access: &Access,
    ) -> Result<Entity> {
        self.write_for(id, |db| {
            let mut e = Self::authorise(db, id, access, false)?;
            if e.revision != revision {
                return Err(Error::Conflict);
            }
            Self::check_parent(db, e.kind, id, parent, access)?;
            // A move out of a protected ancestor cannot silently declassify content.
            let mut old = e.parent;
            let mut inherited = false;
            while let Some(p) = old {
                let a = Self::raw_entity(db, p)?;
                inherited |= a.locked;
                old = a.parent;
            }
            e.locked |= inherited;
            e.parent = parent;
            e.revision += 1;
            e.modified_ms = now_ms();
            Self::save_entity(db, &e)?;
            Ok(e)
        })
    }
    pub fn set_lock(
        &mut self,
        id: Id,
        revision: u64,
        locked: bool,
        access: &Access,
    ) -> Result<Entity> {
        if !access.permits(&Scope::Entity(id)) {
            return Err(Error::Denied);
        }
        self.write_for(id, |db| {
            let mut e = Self::authorise(db, id, access, false)?;
            if e.revision != revision {
                return Err(Error::Conflict);
            }
            e.locked = locked;
            e.revision += 1;
            e.modified_ms = now_ms();
            Self::save_entity(db, &e)?;
            Ok(e)
        })
    }
    /// Bounded directory query. Restricted entities do not contribute titles/snippets.
    pub fn list(
        &self,
        parent: Option<Id>,
        order: Order,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        access: &Access,
    ) -> Result<Vec<Entity>> {
        Self::require_app(&self.db, access)?;
        if let Some(p) = parent {
            Self::authorise(&self.db, p, access, false)?;
        }
        let mut stmt = self
            .db
            .prepare("SELECT id FROM entities WHERE parent IS ?1")?;
        let ids = stmt
            .query_map([parent.map(|i| i.to_string())], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut out = Vec::new();
        for id in ids {
            match Self::authorise(&self.db, Id::parse(&id)?, access, false) {
                Ok(e)
                    if from_ms.is_none_or(|t| e.created_ms >= t)
                        && to_ms.is_none_or(|t| e.created_ms <= t) =>
                {
                    out.push(e)
                }
                Ok(_) | Err(Error::Denied | Error::Unavailable) => {}
                Err(e) => return Err(e),
            }
        }
        out.sort_by(|a, b| {
            match order {
                Order::Name => a.title.cmp(&b.title),
                Order::Created => a.created_ms.cmp(&b.created_ms),
                Order::Modified => a.modified_ms.cmp(&b.modified_ms),
            }
            .then(a.id.cmp(&b.id))
        });
        Ok(out)
    }
    pub fn unfiled(&self, access: &Access) -> Result<Vec<Entity>> {
        Ok(self
            .list(None, Order::Created, None, None, access)?
            .into_iter()
            .filter(|e| e.kind.is_surface())
            .collect())
    }
    pub fn add_link(
        &mut self,
        origin: Id,
        target: Id,
        anchor: Option<Id>,
        access: &Access,
    ) -> Result<Link> {
        let l = Link {
            id: Id::new(),
            origin,
            target,
            anchor,
        };
        self.write(|db| {
            Self::authorise(db, origin, access, false)?;
            let target_entity = Self::authorise(db, target, access, false)?;
            if let Some(a) = anchor {
                let b = Self::read_block(db, a, access)?;
                if b.entity != origin {
                    return Err(Error::Invalid("link anchor"));
                }
            }
            db.execute(
                "INSERT INTO links VALUES(?1,?2,?3,?4,?5)",
                rusqlite::params![
                    l.id.to_string(),
                    origin.to_string(),
                    target.to_string(),
                    anchor.map(|a| a.to_string()),
                    serde_json::to_string(&target_entity)?
                ],
            )?;
            Ok(l.clone())
        })
    }
    pub fn links(&self, id: Id, incoming: bool, access: &Access) -> Result<Vec<Link>> {
        Self::authorise(&self.db, id, access, false)?;
        let sql = if incoming {
            "SELECT id,origin,target,anchor FROM links WHERE target=?1"
        } else {
            "SELECT id,origin,target,anchor FROM links WHERE origin=?1"
        };
        let mut stmt = self.db.prepare(sql)?;
        let rows = stmt
            .query_map([id.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut links = Vec::new();
        for (l, o, t, a) in rows {
            let origin = Id::parse(&o)?;
            if incoming && Self::authorise(&self.db, origin, access, false).is_err() {
                continue;
            }
            if let Some(anchor) = &a {
                match Self::read_block(&self.db, Id::parse(anchor)?, access) {
                    Ok(_) => {}
                    Err(Error::Denied | Error::Unavailable) => continue,
                    Err(e) => return Err(e),
                }
            }
            links.push(Link {
                id: Id::parse(&l)?,
                origin,
                target: Id::parse(&t)?,
                anchor: a.as_deref().map(Id::parse).transpose()?,
            });
        }
        Ok(links)
    }
    pub fn follow_link(&self, link: Id, access: &Access) -> Result<LinkState> {
        let (origin, target, anchor): (String, String, Option<String>) = self
            .db
            .query_row(
                "SELECT origin,target,anchor FROM links WHERE id=?1",
                [link.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;
        Self::authorise(&self.db, Id::parse(&origin)?, access, false)?;
        if let Some(anchor) = anchor {
            Self::read_block(&self.db, Id::parse(&anchor)?, access)?;
        }
        match Self::authorise(&self.db, Id::parse(&target)?, access, false) {
            Ok(e) => Ok(LinkState::Available(e)),
            Err(Error::Denied | Error::Unavailable) => Ok(LinkState::Unavailable),
            Err(e) => Err(e),
        }
    }
    pub fn remove_link(&mut self, link: Id, access: &Access) -> Result<()> {
        self.write(|db| {
            let origin: String = db
                .query_row(
                    "SELECT origin FROM links WHERE id=?1",
                    [link.to_string()],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or(Error::Unavailable)?;
            Self::authorise(db, Id::parse(&origin)?, access, false)?;
            db.execute("DELETE FROM links WHERE id=?1", [link.to_string()])?;
            Ok(())
        })
    }
    pub fn review_deletion(&self, id: Id, access: &Access) -> Result<DeletionReview> {
        Self::authorise(&self.db, id, access, false)?;
        let mut has_dependencies = false;
        let mut visible = BTreeSet::new();
        for target in Self::subtree(&self.db, id)? {
            Self::authorise(&self.db, target, access, false)?;
            Self::require_content_access(&self.db, target, access)?;
            has_dependencies |= self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM exercises WHERE master=?1)",
                [target.to_string()],
                |r| r.get::<_, bool>(0),
            )?;
            let mut stmt = self
                .db
                .prepare("SELECT origin FROM links WHERE target=?1")?;
            for o in stmt.query_map([target.to_string()], |r| r.get::<_, String>(0))? {
                has_dependencies = true;
                let origin = Id::parse(&o?)?;
                if Self::authorise(&self.db, origin, access, false).is_ok() {
                    visible.insert(origin);
                }
            }
        }
        Ok(DeletionReview {
            target: id,
            generation: Self::setting(&self.db, "generation")?,
            has_dependencies,
            visible_dependants: visible.into_iter().collect(),
        })
    }
    pub fn trash(
        &mut self,
        review: DeletionReview,
        acknowledged: bool,
        access: &Access,
    ) -> Result<()> {
        self.write(|db| {
            if Self::setting(db, "generation")? != review.generation {
                return Err(Error::Conflict);
            }
            if review.has_dependencies && !acknowledged {
                return Err(Error::ReviewRequired);
            }
            // Authentication can expire after review without changing generation.
            for target in Self::subtree(db, review.target)? {
                Self::authorise(db, target, access, false)?;
                Self::require_content_access(db, target, access)?;
            }
            let mut e = Self::authorise(db, review.target, access, false)?;
            e.trashed = true;
            e.revision += 1;
            e.modified_ms = now_ms();
            Self::save_entity(db, &e)
        })
    }
    pub fn restore(&mut self, id: Id, access: &Access) -> Result<()> {
        self.write(|db| {
            let mut e = Self::authorise(db, id, access, true)?;
            if !e.trashed {
                return Err(Error::Invalid("Trash state"));
            }
            Self::check_parent(db, e.kind, id, e.parent, access)?;
            e.trashed = false;
            e.revision += 1;
            e.modified_ms = now_ms();
            Self::save_entity(db, &e)
        })
    }
    pub fn purge(&mut self, id: Id, access: &Access) -> Result<()> {
        self.write(|db|{
            let e=Self::authorise(db,id,access,true)?;
            if !e.trashed{return Err(Error::Invalid("permanent deletion requires Trash"));}
            for i in Self::subtree(db,id)? {
                let mut item=Self::authorise(db,i,access,true)?;
                Self::require_content_access(db,i,access)?;
                db.execute("DELETE FROM exercise_members WHERE exercise IN(SELECT id FROM exercises WHERE master=?1)",[i.to_string()])?;
                db.execute("DELETE FROM membership_versions WHERE block IN(SELECT id FROM blocks WHERE entity=?1)",[i.to_string()])?;
                db.execute("DELETE FROM confirmed_headings WHERE block IN(SELECT id FROM blocks WHERE entity=?1)",[i.to_string()])?;
                db.execute("DELETE FROM membership_operations WHERE master=?1",[i.to_string()])?;
                db.execute("DELETE FROM exercise_rules WHERE master=?1",[i.to_string()])?;
                db.execute("DELETE FROM exercises WHERE master=?1",[i.to_string()])?;
                // Keep tombstone identities for honest broken references; purge payloads.
                for table in ["strokes","history","proposals"] {
                    db.execute(&format!("DELETE FROM {table} WHERE block IN(SELECT id FROM blocks WHERE entity=?1)"),[i.to_string()])?;
                }
                db.execute("DELETE FROM source_assets WHERE source IN(SELECT id FROM sources WHERE context=?1)",[i.to_string()])?;
                db.execute("DELETE FROM sources WHERE context=?1",[i.to_string()])?;
                db.execute("DELETE FROM blocks WHERE entity=?1",[i.to_string()])?;
                db.execute("DELETE FROM links WHERE origin=?1",[i.to_string()])?;
                // Frozen link checkpoints must not keep purged titles/content accessible.
                db.execute("UPDATE links SET checkpoint='{}' WHERE target=?1",[i.to_string()])?;
                item.title.clear();item.purged=true;item.trashed=true;item.revision+=1;Self::save_entity(db,&item)?;
            }
            Ok(())
        })
    }
    pub fn set_global_mode(&mut self, mode: WritingMode, access: &Access) -> Result<()> {
        self.write(|db| {
            Self::require_app(db, access)?;
            db.execute(
                "UPDATE settings SET value=?1 WHERE key='global_mode'",
                [serde_json::to_string(&mode)?],
            )?;
            Ok(())
        })
    }
    pub fn set_mode(&mut self, id: Id, mode: WritingMode, access: &Access) -> Result<()> {
        self.write_for(id, |db|{Self::authorise(db,id,access,false)?;db.execute("INSERT INTO modes VALUES(?1,?2) ON CONFLICT(entity) DO UPDATE SET mode=excluded.mode",rusqlite::params![id.to_string(),serde_json::to_string(&mode)?])?;Ok(())})
    }
    pub fn mode(&self, id: Id, access: &Access) -> Result<WritingMode> {
        Self::authorise(&self.db, id, access, false)?;
        let mut cursor = Some(id);
        while let Some(i) = cursor {
            let value: Option<String> = self
                .db
                .query_row(
                    "SELECT mode FROM modes WHERE entity=?1",
                    [i.to_string()],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(v) = value {
                return Ok(serde_json::from_str(&v)?);
            }
            cursor = Self::raw_entity(&self.db, i)?.parent;
        }
        Ok(serde_json::from_str(&Self::setting(
            &self.db,
            "global_mode",
        )?)?)
    }
}
