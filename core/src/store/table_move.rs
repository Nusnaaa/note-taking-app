//! Cut a whole table to an explicit writing target. The source stays retained
//! and hidden; destination identities are independent, with coupled Undo.
use super::*;
impl Store {
    pub fn move_table(
        &mut self,
        block: Id,
        revision: u64,
        target: WritingTarget,
        access: &Access,
    ) -> Result<Vec<Block>> {
        let original = Self::read_block(&self.db, block, access)?;
        let source_owner = original.entity;
        let target_owner = Self::writing_owner(&self.db, target, access)?;
        let linked = source_owner != target_owner;
        self.document_save_states
            .insert(source_owner, SaveState::Saving);
        self.document_save_states
            .insert(target_owner, SaveState::Saving);
        let result = self.write(|db| {
            let mut source = Self::read_block(db, block, access)?;
            source.validate()?;
            if source.revision != revision || source.entity != source_owner {
                return Err(Error::Conflict);
            }
            let mut cut = |db: &Connection| -> Result<()> {
                Self::authorise_block_metadata(db, source.id, access)?;
                source.visible = false;
                source.revision = source.revision.checked_add(1).ok_or(Error::ResourceLimit)?;
                Self::persist_block(db, &source)
            };
            let copied = if linked {
                let (copied, destination_operation) =
                    Self::journal_for_operation(db, target_owner, |db| {
                        Self::copy_table_in_transaction(
                            db,
                            &original_checked(db, block, revision, source_owner, access)?,
                            target,
                            access,
                        )
                    })?;
                let (_, source_operation) =
                    Self::journal_for_operation(db, source_owner, &mut cut)?;
                db.execute(
                    "INSERT INTO edit_links(operation,peer) VALUES(?1,?2),(?2,?1)",
                    rusqlite::params![source_operation, destination_operation],
                )?;
                copied
            } else {
                Self::journal_for(db, target_owner, |db| {
                    let current = original_checked(db, block, revision, source_owner, access)?;
                    let copied = Self::copy_table_in_transaction(db, &current, target, access)?;
                    cut(db)?;
                    Ok(copied)
                })?
            };
            Ok(vec![source, copied])
        });
        let state = if result.is_ok() {
            SaveState::SavedLocally
        } else {
            SaveState::NeedsAttention
        };
        self.document_save_states.insert(source_owner, state);
        self.document_save_states.insert(target_owner, state);
        result
    }
}
fn original_checked(
    db: &Connection,
    block: Id,
    revision: u64,
    owner: Id,
    access: &Access,
) -> Result<Block> {
    let source = Store::read_block(db, block, access)?;
    if source.revision != revision || source.entity != owner {
        return Err(Error::Conflict);
    }
    Ok(source)
}
