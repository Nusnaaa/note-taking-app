use super::*;

use crate::{
    content::Content,
    processing::{CompletedRecognition, Descriptor, RecognitionOutput},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProposalState {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub id: Id,
    pub block: Id,
    pub revision: u64,
    pub fingerprint: String,
    pub engine: Descriptor,
    pub output: RecognitionOutput,
    pub correction: Option<Content>,
    pub state: ProposalState,
    #[serde(default)]
    pub region: Option<Id>,
}

/// Bounded discovery without copying transcription bodies into every list row.
#[derive(Debug, Clone, Serialize)]
pub struct ProposalSummary {
    pub id: Id,
    pub block: Id,
    pub region: Option<Id>,
    pub revision: u64,
    pub engine: String,
    pub state: ProposalState,
    pub has_correction: bool,
    pub candidates: usize,
}

impl Store {
    pub fn propose(
        &mut self,
        completed: CompletedRecognition,
        access: &Access,
    ) -> Result<Proposal> {
        let master = Self::read_block(&self.db, completed.snapshot.block, access)?.entity;
        completed.check_delivery()?;
        let p = Proposal {
            id: completed
                .authority
                .as_ref()
                .map_or_else(Id::new, |authority| authority.ticket),
            block: completed.snapshot.block,
            revision: completed.snapshot.revision,
            fingerprint: completed.snapshot.fingerprint.clone(),
            engine: completed.descriptor,
            output: completed.output,
            correction: None,
            state: ProposalState::Pending,
            region: completed.snapshot.region,
        };

        // A host ticket is chosen before inference. Retry acknowledges the same
        // persisted review, including later correction/state, without reapplying it.
        if self
            .db
            .prepare("SELECT 1 FROM proposals WHERE id=?1")?
            .exists([p.id.to_string()])?
        {
            let existing = Self::read_proposal(&self.db, p.id, access)?;
            if existing.block != p.block
                || existing.revision != p.revision
                || existing.fingerprint != p.fingerprint
                || existing.region != p.region
                || existing.engine != p.engine
                || existing.output != p.output
            {
                return Err(Error::Conflict);
            }
            return Ok(existing);
        }
        self.write_for(master, |db| {
            let current = Self::make_region_snapshot(db, p.block, p.region, access)?;

            if current.fingerprint != p.fingerprint || current.revision != p.revision {
                return Err(Error::Conflict);
            }

            db.execute(
                "INSERT INTO proposals VALUES(?1,?2,?3,'pending')",
                rusqlite::params![
                    p.id.to_string(),
                    p.block.to_string(),
                    serde_json::to_string(&p)?
                ],
            )?;

            Ok(p.clone())
        })
    }

    fn read_proposal(db: &Connection, id: Id, access: &Access) -> Result<Proposal> {
        Self::require_app(db, access)?;
        let owner: String = db
            .query_row(
                "SELECT block FROM proposals WHERE id=?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;
        let owner = Id::parse(&owner).map_err(|_| Error::Integrity)?;
        // SQL owns the relation. Authorise it before parsing derived private
        // transcription bytes; a payload field cannot nominate a public owner.
        Self::read_block(db, owner, access)?;
        let payload: String = db
            .query_row(
                "SELECT payload FROM proposals WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;

        let p: Proposal = serde_json::from_str(&payload)?;

        if p.id != id || p.block != owner {
            return Err(Error::Integrity);
        }

        Ok(p)
    }

    fn save_proposal(db: &Connection, p: &Proposal) -> Result<()> {
        db.execute(
            "UPDATE proposals SET payload=?1,status=?2 WHERE id=?3",
            rusqlite::params![
                serde_json::to_string(p)?,
                serde_json::to_string(&p.state)?,
                p.id.to_string()
            ],
        )?;

        Ok(())
    }

    pub fn proposal(&self, id: Id, access: &Access) -> Result<Proposal> {
        Self::read_proposal(&self.db, id, access)
    }

    pub fn proposals_for(
        &self,
        block: Id,
        offset: u32,
        limit: u32,
        access: &Access,
    ) -> Result<Vec<ProposalSummary>> {
        Self::read_block(&self.db, block, access)?;
        if !(1..=512).contains(&limit) || offset > 100_000 {
            return Err(Error::Invalid("proposal page"));
        }
        // Optional discovery indexing cannot prevent opening/writing a library.
        // It changes no logical row/portable format and is attempted only here.
        self.db.execute(
            "CREATE INDEX IF NOT EXISTS proposals_block_id ON proposals(block,id)",
            [],
        )?;
        let mut query = self
            .db
            .prepare("SELECT id FROM proposals WHERE block=?1 ORDER BY id LIMIT ?2 OFFSET ?3")?;
        let mut result = Vec::new();
        for row in query.query_map(rusqlite::params![block.to_string(), limit, offset], |row| {
            row.get::<_, String>(0)
        })? {
            let id = Id::parse(&row?).map_err(|_| Error::Integrity)?;
            let proposal = Self::read_proposal(&self.db, id, access)?;
            if proposal.block != block || proposal.engine.id.len() > 256 {
                return Err(Error::Integrity);
            }
            result.push(ProposalSummary {
                id,
                block,
                region: proposal.region,
                revision: proposal.revision,
                engine: proposal.engine.id,
                state: proposal.state,
                has_correction: proposal.correction.is_some(),
                candidates: proposal.output.candidates.len(),
            });
        }
        Ok(result)
    }

    pub fn correct_proposal(
        &mut self,
        id: Id,
        correction: Content,
        access: &Access,
    ) -> Result<Proposal> {
        correction.validate()?;

        let master = Self::read_block(&self.db, self.proposal(id, access)?.block, access)?.entity;

        self.write_for(master, |db| {
            let mut p = Self::read_proposal(db, id, access)?;

            if p.state != ProposalState::Pending {
                return Err(Error::Conflict);
            }

            p.correction = Some(correction);

            Self::save_proposal(db, &p)?;

            Ok(p)
        })
    }

    pub fn reject_proposal(&mut self, id: Id, access: &Access) -> Result<()> {
        let master = Self::read_block(&self.db, self.proposal(id, access)?.block, access)?.entity;

        self.write_for(master, |db| {
            let mut p = Self::read_proposal(db, id, access)?;

            if p.state != ProposalState::Pending {
                return Err(Error::Conflict);
            }

            p.state = ProposalState::Rejected;

            Self::save_proposal(db, &p)
        })
    }

    /// Explicit acceptance is the ONLY operation committing recogniser output.
    /// The host may offer Copy afterwards, but this operation cannot write clipboard.
    pub fn accept_proposal(&mut self, id: Id, candidate: usize, access: &Access) -> Result<Block> {
        let master = Self::read_block(&self.db, self.proposal(id, access)?.block, access)?.entity;

        self.write_for(master, |db| {
            let mut p = Self::read_proposal(db, id, access)?;

            if p.state != ProposalState::Pending {
                return Err(Error::Conflict);
            }

            let snapshot = Self::make_region_snapshot(db, p.block, p.region, access)?;

            if snapshot.revision != p.revision || snapshot.fingerprint != p.fingerprint {
                return Err(Error::Conflict);
            }

            let content = p
                .correction
                .clone()
                .or_else(|| p.output.candidates.get(candidate).cloned())
                .ok_or(Error::Invalid("candidate selection"))?;

            content.validate()?;

            let mut b = Self::read_block(db, p.block, access)?;

            Self::apply_region_content(&mut b, p.region, content)?;

            b.display = Display::Editable;

            b.manually_corrected =
                p.correction.is_some() || (p.region.is_some() && b.manually_corrected);

            b.revision += 1;

            Self::persist_block(db, &b)?;

            p.state = ProposalState::Accepted;

            Self::save_proposal(db, &p)?;

            Ok(b)
        })
    }

    /// Accept only explicit selections. Other pending proposals remain unchanged.
    /// A stale selection rolls back the entire batch, including earlier members.
    pub fn accept_proposals(
        &mut self,
        selections: &[(Id, usize)],
        confirmed: bool,
        access: &Access,
    ) -> Result<Vec<Block>> {
        if !confirmed {
            return Err(Error::ReviewRequired);
        }

        if selections.is_empty() || selections.len() > 512 {
            return Err(Error::Invalid("proposal batch"));
        }

        let owners = selections
            .iter()
            .map(|(id, _)| {
                self.proposal(*id, access)
                    .and_then(|p| self.block(p.block, access).map(|b| b.entity))
            })
            .collect::<Result<std::collections::BTreeSet<_>>>()?;

        for &owner in &owners {
            self.document_save_states.insert(owner, SaveState::Saving);
        }

        let result = self.write(|db| {
            let mut seen = std::collections::BTreeSet::new();
            let mut targets =
                std::collections::BTreeMap::<Id, std::collections::BTreeSet<Option<Id>>>::new();
            let mut blocks = std::collections::BTreeMap::<Id, Block>::new();
            let mut order = Vec::new();
            let mut prepared = Vec::new();
            // Validate every source against the same pre-mutation state. Distinct
            // regions may share a block; duplicate/whole-region overlaps may not.
            for &(id, candidate) in selections {
                let p = Self::read_proposal(db, id, access)?;
                if p.state != ProposalState::Pending || !seen.insert(p.id) {
                    return Err(Error::Conflict);
                }
                let regions = targets.entry(p.block).or_default();
                if regions.contains(&None)
                    || (p.region.is_none() && !regions.is_empty())
                    || !regions.insert(p.region)
                {
                    return Err(Error::Conflict);
                }
                let snapshot = Self::make_region_snapshot(db, p.block, p.region, access)?;
                if snapshot.revision != p.revision || snapshot.fingerprint != p.fingerprint {
                    return Err(Error::Conflict);
                }
                if let std::collections::btree_map::Entry::Vacant(entry) = blocks.entry(p.block) {
                    entry.insert(Self::read_block(db, p.block, access)?);
                    order.push(p.block);
                }
                let content = p
                    .correction
                    .clone()
                    .or_else(|| p.output.candidates.get(candidate).cloned())
                    .ok_or(Error::Invalid("candidate selection"))?;
                prepared.push((p, content));
            }
            for (p, content) in &prepared {
                let block = blocks.get_mut(&p.block).ok_or(Error::Integrity)?;
                Self::apply_region_content(block, p.region, content.clone())?;
                block.display = Display::Editable;
                if p.region.is_none() {
                    block.manually_corrected =
                        p.correction.is_some() || (p.region.is_some() && block.manually_corrected);
                } else {
                    block.manually_corrected |= p.correction.is_some();
                }
            }
            for block in blocks.values_mut() {
                block.revision = block
                    .revision
                    .checked_add(1)
                    .filter(|revision| *revision <= i64::MAX as u64)
                    .ok_or(Error::ResourceLimit)?;
            }
            for owner in &owners {
                Self::journal_for(db, *owner, |db| {
                    for id in &order {
                        let block = blocks.get(id).ok_or(Error::Integrity)?;
                        if block.entity == *owner {
                            Self::persist_block(db, block)?;
                        }
                    }
                    Ok(())
                })?;
            }
            for (mut proposal, _) in prepared {
                proposal.state = ProposalState::Accepted;
                Self::save_proposal(db, &proposal)?;
            }
            order
                .into_iter()
                .map(|id| blocks.remove(&id).ok_or(Error::Integrity))
                .collect::<Result<Vec<_>>>()
        });

        for owner in owners {
            self.document_save_states.insert(
                owner,
                if result.is_ok() {
                    SaveState::SavedLocally
                } else {
                    SaveState::NeedsAttention
                },
            );
        }

        result
    }
}
