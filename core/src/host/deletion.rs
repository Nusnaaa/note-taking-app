//! Opaque, bounded, host-local destructive-action reviews. No deserialisable
//! DeletionReview crosses this boundary; confirmation rechecks Store authority.
use super::*;
use crate::store::DeletionReview;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const REVIEW_LIMIT: usize = 32;
const REVIEW_LIFETIME: Duration = Duration::from_secs(300);
struct Pending {
    review: DeletionReview,
    permanent: bool,
    born: Instant,
}
#[derive(Default)]
pub(super) struct DeletionState {
    reviews: BTreeMap<Id, Pending>,
}
impl DeletionState {
    pub fn clear(&mut self) {
        self.reviews.clear();
    }
    fn expire(&mut self) {
        self.reviews
            .retain(|_, p| p.born.elapsed() < REVIEW_LIFETIME);
    }
    pub fn prepare(
        &mut self,
        store: &Store,
        access: &Access,
        entity: Id,
        permanent: bool,
    ) -> Result<Value> {
        self.expire();
        if self.reviews.len() >= REVIEW_LIMIT {
            return Err(Error::ResourceLimit);
        }
        let review = if permanent {
            store.review_purge(entity, access)?
        } else {
            store.review_deletion(entity, access)?
        };
        let token = Id::new();
        let result = json!({"review":token,"permanent":permanent,"has_dependencies":review.has_dependencies,"visible_dependants":review.visible_dependants,"capture_targets":review.capture_targets()});
        self.reviews.insert(
            token,
            Pending {
                review,
                permanent,
                born: Instant::now(),
            },
        );
        Ok(result)
    }
    pub fn validate(&mut self, store: &Store, access: &Access, token: Id) -> Result<()> {
        self.expire();
        let pending = self.reviews.get(&token).ok_or(Error::Unavailable)?;
        store.validate_deletion_review(&pending.review, access)
    }
    pub fn confirm(
        &mut self,
        store: &mut Store,
        access: &Access,
        token: Id,
        confirmed: bool,
    ) -> Result<()> {
        self.expire();
        if !confirmed {
            return Err(Error::ReviewRequired);
        }
        let pending = self.reviews.remove(&token).ok_or(Error::Unavailable)?;
        if pending.permanent {
            store.purge_reviewed(pending.review, true, access)
        } else {
            store.trash(pending.review, true, access)
        }
    }
    pub fn release(&mut self, token: Id) {
        self.reviews.remove(&token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_and_capacity_bounded_reviews_never_delete_content() {
        let a = Access::public();
        let mut s = Store::memory().unwrap();
        let n = s.create(Kind::Note, None, "retained", &a).unwrap();
        let mut state = DeletionState::default();
        let value = state.prepare(&s, &a, n.id, false).unwrap();
        let token: Id = serde_json::from_value(value["review"].clone()).unwrap();
        state.reviews.get_mut(&token).unwrap().born = Instant::now() - REVIEW_LIFETIME;
        assert!(matches!(
            state.confirm(&mut s, &a, token, true),
            Err(Error::Unavailable)
        ));
        assert!(s.entity(n.id, &a).is_ok());
        for _ in 0..REVIEW_LIMIT {
            state.prepare(&s, &a, n.id, false).unwrap();
        }
        assert!(matches!(
            state.prepare(&s, &a, n.id, false),
            Err(Error::ResourceLimit)
        ));
        state.clear();
        assert!(state.prepare(&s, &a, n.id, false).is_ok());
    }
}
