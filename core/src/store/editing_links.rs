//! Validate active pairs and content-free retirement barriers independently of
//! any caller's ability to read the other document.
use super::*;
impl Store {
    pub(crate) fn validate_edit_links(
        &self,
        control: Option<&crate::processing::JobControl>,
    ) -> Result<()> {
        let mut query = self.db.prepare("SELECT l.operation,l.peer,l.retired,o.entity,o.active_ms,o.undone FROM edit_links l LEFT JOIN edit_operations o ON o.id=l.operation")?;
        for row in query.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, bool>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, bool>(5)?,
            ))
        })? {
            if let Some(control) = control {
                control.check()?;
            }
            let (operation, peer, retired, owner, tick, undone) = row?;
            if operation <= 0 || peer < 0 || operation == peer || (!retired && peer == 0) {
                return Err(Error::Integrity);
            }
            let other: Option<(String, i64, bool)> = self
                .db
                .query_row(
                    "SELECT entity,active_ms,undone FROM edit_operations WHERE id=?1",
                    [peer],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let has_changes = self
                .db
                .prepare("SELECT 1 FROM edit_changes WHERE operation=?1")?
                .exists([operation])?;
            if retired {
                if peer != 0 || has_changes {
                    return Err(Error::Integrity);
                }
            } else {
                let (other_owner, other_tick, other_undone) = other.ok_or(Error::Integrity)?;
                let reciprocal: Option<(i64, bool)> = self
                    .db
                    .query_row(
                        "SELECT peer,retired FROM edit_links WHERE operation=?1",
                        [peer],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                if owner == other_owner
                    || tick != other_tick
                    || undone != other_undone
                    || !has_changes
                    || reciprocal != Some((operation, false))
                {
                    return Err(Error::Integrity);
                }
            }
        }
        Ok(())
    }
}
