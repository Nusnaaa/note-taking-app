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
}
impl Store {
    pub fn propose(
        &mut self,
        completed: CompletedRecognition,
        access: &Access,
    ) -> Result<Proposal> {
        let p = Proposal {
            id: Id::new(),
            block: completed.snapshot.block,
            revision: completed.snapshot.revision,
            fingerprint: completed.snapshot.fingerprint.clone(),
            engine: completed.descriptor,
            output: completed.output,
            correction: None,
            state: ProposalState::Pending,
        };
        self.write(|db| {
            let current = Self::make_snapshot(db, p.block, access)?;
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
        let payload: String = db
            .query_row(
                "SELECT payload FROM proposals WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Unavailable)?;
        let p: Proposal = serde_json::from_str(&payload)?;
        Self::read_block(db, p.block, access)?;
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
    pub fn correct_proposal(
        &mut self,
        id: Id,
        correction: Content,
        access: &Access,
    ) -> Result<Proposal> {
        correction.validate()?;
        self.write(|db| {
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
        self.write(|db| {
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
        self.write(|db| {
            let mut p = Self::read_proposal(db, id, access)?;
            if p.state != ProposalState::Pending {
                return Err(Error::Conflict);
            }
            let snapshot = Self::make_snapshot(db, p.block, access)?;
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
            b.content = content;
            b.display = Display::Editable;
            b.manually_corrected = p.correction.is_some();
            b.revision += 1;
            Self::persist_block(db, &b)?;
            p.state = ProposalState::Accepted;
            Self::save_proposal(db, &p)?;
            Ok(b)
        })
    }
}
