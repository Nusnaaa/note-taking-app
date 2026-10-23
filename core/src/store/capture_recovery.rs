//! Authority to inspect private, uncommitted capture evidence. This never grants
//! document access or permission to replay into a trashed/purged destination.
use super::*;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureTargetState {
    Active,
    Trashed,
    Purged,
}

impl Store {
    pub fn capture_recovery_access(
        &self,
        entity: Id,
        access: &Access,
    ) -> Result<CaptureTargetState> {
        Self::require_app(&self.db, access)?;
        let target = Self::raw_entity(&self.db, entity)?;
        if target.id != entity {
            return Err(Error::Integrity);
        }
        let mut current = Some(entity);
        let mut visited = BTreeSet::new();
        let mut state = CaptureTargetState::Active;
        while let Some(id) = current {
            if !visited.insert(id) {
                return Err(Error::Integrity);
            }
            let item = Self::raw_entity(&self.db, id)?;
            if item.id != id {
                return Err(Error::Integrity);
            }
            if item.locked && !access.permits(&Scope::Entity(id)) {
                return Err(Error::Denied);
            }
            if item.purged {
                state = CaptureTargetState::Purged;
            } else if item.trashed && state == CaptureTargetState::Active {
                state = CaptureTargetState::Trashed;
            }
            current = item.parent;
        }
        // Retired destinations are no longer ordinary readable documents. A
        // fresh target-specific proof is required even if they used to be public.
        if state != CaptureTargetState::Active && !access.permits(&Scope::Entity(entity)) {
            return Err(Error::Denied);
        }
        if !target.kind.is_surface() {
            return Err(Error::Invalid("capture target"));
        }
        Ok(state)
    }
}
