//! Native JSON v1 for the currently implemented core subset. Unknown fields fail
//! closed. It is NOT yet the complete v0.5.8 interchange contract (see status).
use crate::{
    crypto, digest,
    ink::Stroke,
    now_ms,
    security::{Access, Authenticator, Scope},
    store::{Block, Entity, HistoryEntry, Link, Store},
    Error, Id, Result,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeBlock {
    pub current: Block,
    pub strokes: Vec<Stroke>,
    pub history: Vec<HistoryEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDocument {
    pub version: u32,
    pub created_ms: i64,
    pub entities: Vec<Entity>,
    pub blocks: Vec<NativeBlock>,
    pub links: Vec<Link>,
    pub omissions: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checked {
    payload: String,
    sha256: String,
}

pub struct ExportPlan {
    generation: String,
    document: NativeDocument,
    scopes: BTreeSet<Scope>,
    pub selected_entities: usize,
    pub selected_blocks: usize,
    pub includes_raw_ink: bool,
    pub omissions: Vec<String>,
}
pub enum ExportProtection<'a> {
    Encrypted { password: &'a str },
    Plaintext { acknowledge_protected: bool },
}
pub struct Exported {
    pub bytes: Vec<u8>,
    pub recovery_key: Option<zeroize::Zeroizing<String>>,
}

impl NativeDocument {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(Error::ResourceLimit);
        }
        let checked: Checked = serde_json::from_slice(bytes)?;
        if digest(checked.payload.as_bytes()) != checked.sha256 {
            return Err(Error::Integrity);
        }
        let doc: Self = serde_json::from_str(&checked.payload)?;
        doc.validate()?;
        Ok(doc)
    }
    fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let payload = serde_json::to_string(self)?;
        Ok(serde_json::to_vec(&Checked {
            sha256: digest(payload.as_bytes()),
            payload,
        })?)
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(Error::Unsupported);
        }
        if self.entities.len() > 10_000 || self.blocks.len() > 100_000 {
            return Err(Error::ResourceLimit);
        }
        let mut entities = BTreeMap::new();
        for e in &self.entities {
            if e.purged
                || e.trashed
                || e.title.len() > 4096
                || e.revision > i64::MAX as u64
                || entities.insert(e.id, e).is_some()
            {
                return Err(Error::Invalid("native entity"));
            }
        }
        for e in &self.entities {
            let mut cursor = e.parent;
            let mut visited = BTreeSet::from([e.id]);
            let mut depth = usize::from(e.kind == crate::store::Kind::Module);
            if let Some(p) = cursor {
                let parent = entities.get(&p).ok_or(Error::Invalid("native parent"))?;
                if !parent.kind.accepts(e.kind) {
                    return Err(Error::Invalid("native parent kind"));
                }
            }
            while let Some(p) = cursor {
                if !visited.insert(p) {
                    return Err(Error::Invalid("native cycle"));
                }
                let parent = entities.get(&p).ok_or(Error::Invalid("native ancestor"))?;
                depth += usize::from(parent.kind == crate::store::Kind::Module);
                cursor = parent.parent;
            }
            if depth > 2 {
                return Err(Error::Invalid("native Module depth"));
            }
        }
        let mut block_ids = BTreeSet::new();
        let mut stroke_ids = BTreeSet::new();
        for n in &self.blocks {
            let b = &n.current;
            b.validate()?;
            if b.revision > i64::MAX as u64
                || !block_ids.insert(b.id)
                || !entities.get(&b.entity).is_some_and(|e| e.kind.is_surface())
            {
                return Err(Error::Invalid("native block"));
            }
            let ids = n.strokes.iter().map(|s| s.id).collect::<Vec<_>>();
            if ids != b.source_ids {
                return Err(Error::Invalid("native ink membership"));
            }
            for s in &n.strokes {
                s.validate()?;
                if !stroke_ids.insert(s.id) {
                    return Err(Error::Invalid("native duplicate stroke"));
                }
            }
            let mut revisions = BTreeSet::new();
            for h in &n.history {
                h.block.validate()?;
                if h.block.id != b.id
                    || h.block.entity != b.entity
                    || h.revision != h.block.revision
                    || h.revision > b.revision
                    || h.active_ms > i64::MAX as u64
                    || !revisions.insert(h.revision)
                    || h.block.source_ids.iter().any(|i| !b.source_ids.contains(i))
                {
                    return Err(Error::Invalid("native history"));
                }
            }
        }
        let mut links = BTreeSet::new();
        for l in &self.links {
            if !links.insert(l.id)
                || !entities.contains_key(&l.origin)
                || !entities.contains_key(&l.target)
                || l.anchor.is_some_and(|a| {
                    !self
                        .blocks
                        .iter()
                        .any(|b| b.current.id == a && b.current.entity == l.origin)
                })
            {
                return Err(Error::Invalid("native link"));
            }
        }
        Ok(())
    }
}

impl Store {
    fn protection_scopes(&self, entity: Id, block: Option<Id>) -> Result<BTreeSet<Scope>> {
        let mut scopes = BTreeSet::new();
        if Self::setting(&self.db, "app_locked")? == "true" {
            scopes.insert(Scope::App);
        }
        let mut cursor = Some(entity);
        while let Some(id) = cursor {
            let e = Self::raw_entity(&self.db, id)?;
            if e.locked {
                scopes.insert(Scope::Entity(id));
            }
            cursor = e.parent;
        }
        if let Some(id) = block {
            if Self::raw_block(&self.db, id)?.locked {
                scopes.insert(Scope::Block(id));
            }
        }
        Ok(scopes)
    }
    /// Explicit exact-entity selection: no implicit descendant or linked-source export.
    /// Lossless original ink/history require affirmative inclusion. Unsupported data
    /// are refused, not silently exported as a supposedly complete native document.
    pub fn prepare_native_export(
        &self,
        ids: &[Id],
        include_original_ink: bool,
        access: &Access,
    ) -> Result<ExportPlan> {
        let generation = Self::setting(&self.db, "generation")?;
        if ids.is_empty() {
            return Err(Error::Invalid("export scope"));
        }
        let selected: BTreeSet<_> = ids.iter().copied().collect();
        if selected.len() != ids.len() {
            return Err(Error::Invalid("duplicate export scope"));
        }
        let mut entities = Vec::new();
        let mut blocks = Vec::new();
        let mut links = Vec::new();
        let mut omissions = Vec::new();
        let mut scopes = BTreeSet::new();
        let mut has_ink = false;
        for &id in ids {
            let mut e = Self::authorise(&self.db, id, access, false)?;
            scopes.extend(self.protection_scopes(id, None)?);
            // Refuse unsupported source packaging instead of silently losing PDF state.
            if self
                .db
                .prepare("SELECT 1 FROM sources WHERE context=?1")?
                .exists([id.to_string()])?
            {
                return Err(Error::Unsupported);
            }
            if e.parent.is_some_and(|p| !selected.contains(&p)) {
                e.parent = None;
                omissions.push("Unselected parent omitted; selected entity becomes a root in the recovered copy.".into());
            }
            if !self.protection_scopes(id, None)?.is_empty() {
                e.locked = true;
            }
            let mut stmt = self
                .db
                .prepare("SELECT id FROM blocks WHERE entity=?1 ORDER BY ordinal,id")?;
            let block_ids = stmt
                .query_map([id.to_string()], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for bid in block_ids {
                let bid = Id::parse(&bid)?;
                let b = Self::read_block(&self.db, bid, access)?;
                scopes.extend(self.protection_scopes(id, Some(bid))?);
                has_ink |= !b.source_ids.is_empty();
                if !include_original_ink && !b.source_ids.is_empty() {
                    return Err(Error::ReviewRequired);
                }
                blocks.push(NativeBlock {
                    strokes: Self::original_strokes(&self.db, &b)?,
                    history: self.history(bid, access)?,
                    current: b,
                });
            }
            for l in self.links(id, false, access)? {
                if selected.contains(&l.target) {
                    links.push(l)
                } else {
                    omissions.push("Unselected link target omitted; current subset format does not preserve external native references.".into());
                }
            }
            entities.push(e);
        }
        let document = NativeDocument {
            version: 1,
            created_ms: now_ms(),
            entities,
            blocks,
            links,
            omissions: omissions.clone(),
        };
        document.validate()?;
        if Self::setting(&self.db, "generation")? != generation {
            return Err(Error::Conflict);
        }
        Ok(ExportPlan {
            generation,
            selected_entities: document.entities.len(),
            selected_blocks: document.blocks.len(),
            includes_raw_ink: has_ink,
            document,
            scopes,
            omissions,
        })
    }
    pub fn finish_native_export(
        &self,
        plan: ExportPlan,
        protection: ExportProtection<'_>,
        auth: &mut dyn Authenticator,
        acknowledge_omissions: bool,
    ) -> Result<Exported> {
        if Self::setting(&self.db, "generation")? != plan.generation {
            return Err(Error::Conflict);
        }
        if !plan.omissions.is_empty() && !acknowledge_omissions {
            return Err(Error::ReviewRequired);
        }
        if !plan.scopes.is_empty() && !auth.verify(Id::new(), &plan.scopes) {
            return Err(Error::Denied);
        }
        let plain = zeroize::Zeroizing::new(plan.document.encode()?);
        let exported = match protection {
            ExportProtection::Encrypted { password } => {
                let encrypted = crypto::encrypt(&plain, password)?;
                Exported {
                    bytes: encrypted.bytes,
                    recovery_key: Some(encrypted.recovery_key),
                }
            }
            ExportProtection::Plaintext {
                acknowledge_protected,
            } => {
                if !plan.scopes.is_empty() && !acknowledge_protected {
                    return Err(Error::ReviewRequired);
                }
                Exported {
                    bytes: plain.to_vec(),
                    recovery_key: None,
                }
            }
        };
        // Authentication/encryption may be slow. Recheck after both before releasing
        // any bytes; a second connection may have changed locks or revoked sources.
        if Self::setting(&self.db, "generation")? != plan.generation {
            return Err(Error::Conflict);
        }
        Ok(exported)
    }
    /// Restore to independent IDs by default. Never upsert into an existing object.
    /// Protection stays set; destination platform credentials are established later.
    pub fn import_native(&mut self, bytes: &[u8], access: &Access) -> Result<Vec<Id>> {
        let mut doc = NativeDocument::decode(bytes)?;
        let mut ids = BTreeMap::new();
        for e in &doc.entities {
            ids.insert(e.id, Id::new());
        }
        for b in &doc.blocks {
            ids.insert(b.current.id, Id::new());
            for s in &b.strokes {
                ids.insert(s.id, Id::new());
            }
        }
        for e in &mut doc.entities {
            e.id = ids[&e.id];
            e.parent = e.parent.map(|p| ids[&p]);
            e.title = format!("{} (recovered)", e.title);
        }
        let remap_block = |b: &mut Block| {
            b.id = ids[&b.id];
            b.entity = ids[&b.entity];
            b.source_ids = b.source_ids.iter().map(|i| ids[i]).collect();
            b.visible_strokes = b.visible_strokes.iter().map(|i| ids[i]).collect();
        };
        for b in &mut doc.blocks {
            remap_block(&mut b.current);
            for h in &mut b.history {
                remap_block(&mut h.block);
            }
            for s in &mut b.strokes {
                s.id = ids[&s.id];
            }
        }
        for l in &mut doc.links {
            l.id = Id::new();
            l.origin = ids[&l.origin];
            l.target = ids[&l.target];
            l.anchor = l.anchor.map(|a| ids[&a]);
        }
        doc.validate()?;
        self.write(|db| {
            Self::require_app(db, access)?;
            // Parent insertion order is not trusted. Defer FK checks until commit.
            db.execute_batch("PRAGMA defer_foreign_keys=ON;")?;
            for e in &doc.entities {
                Self::save_entity(db, e)?;
            }
            for (ordinal, b) in doc.blocks.iter().enumerate() {
                db.execute(
                    "INSERT INTO blocks VALUES(?1,?2,?3,?4)",
                    rusqlite::params![
                        b.current.id.to_string(),
                        b.current.entity.to_string(),
                        ordinal as i64,
                        serde_json::to_string(&b.current)?
                    ],
                )?;
                for s in &b.strokes {
                    let p = serde_json::to_string(s)?;
                    db.execute(
                        "INSERT INTO strokes VALUES(?1,?2,?3,?4)",
                        rusqlite::params![
                            s.id.to_string(),
                            b.current.id.to_string(),
                            p,
                            digest(p.as_bytes())
                        ],
                    )?;
                }
                // Translate historical active-use times to destination clock while
                // keeping their age relative to the newest retained entry.
                let tick: i64 = Self::setting(db, "active_ms")?
                    .parse()
                    .map_err(|_| Error::Integrity)?;
                let newest = b.history.iter().map(|h| h.active_ms).max().unwrap_or(0);
                for h in &b.history {
                    let age = newest - h.active_ms;
                    let t = tick.saturating_sub(age.min(i64::MAX as u64) as i64).max(0);
                    db.execute(
                        "INSERT INTO history VALUES(?1,?2,?3,?4)",
                        rusqlite::params![
                            b.current.id.to_string(),
                            h.revision,
                            t,
                            serde_json::to_string(&h.block)?
                        ],
                    )?;
                }
            }
            for l in &doc.links {
                db.execute(
                    "INSERT INTO links VALUES(?1,?2,?3,?4,'{}')",
                    rusqlite::params![
                        l.id.to_string(),
                        l.origin.to_string(),
                        l.target.to_string(),
                        l.anchor.map(|i| i.to_string())
                    ],
                )?;
            }
            Ok(doc.entities.iter().map(|e| e.id).collect())
        })
    }
}
