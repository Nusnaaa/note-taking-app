//! Application access control, NOT separate encryption of the working SQLite file.
//! Android must supply a trusted platform authenticator; a model never supplies one.
use crate::{Error, Id, Result};
use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Scope {
    App,
    Entity(Id),
    Block(Id),
    Exercise(Id),
}

/// The host must verify a fresh platform authentication against this challenge.
/// Implementations are trusted application code, not downloadable model plugins.
pub trait Authenticator {
    fn verify(&mut self, challenge: Id, scopes: &BTreeSet<Scope>) -> bool;
}

/// Non-serialisable, bounded capability. Callers cannot populate its private fields.
#[derive(Default)]
pub struct Access {
    scopes: BTreeSet<Scope>,
    expires: Option<Instant>,
    // Identity only, never a permission grant. Every scope mutation must rotate
    // this allocation; cached readers retain an Arc, preventing address reuse.
    grant_epoch: Arc<()>,
}
impl Access {
    pub fn public() -> Self {
        Self::default()
    }
    pub fn authenticate(auth: &mut dyn Authenticator, scopes: BTreeSet<Scope>) -> Result<Self> {
        if scopes.is_empty() || !auth.verify(Id::new(), &scopes) {
            return Err(Error::Denied);
        }
        Ok(Self {
            scopes,
            expires: Some(Instant::now() + Duration::from_secs(300)),
            grant_epoch: Arc::new(()),
        })
    }
    pub fn relock(&mut self) {
        self.scopes.clear();
        self.expires = None;
        self.grant_epoch = Arc::new(());
    }
    pub(crate) fn grant_epoch(&self) -> &Arc<()> {
        &self.grant_epoch
    }
    pub(crate) fn expires_at(&self) -> Option<Instant> {
        self.expires
    }
    pub(crate) fn remaining(&self) -> Option<Duration> {
        self.expires
            .map(|until| until.saturating_duration_since(Instant::now()))
    }
    pub(crate) fn permits(&self, scope: &Scope) -> bool {
        self.expires.is_some_and(|until| Instant::now() < until) && self.scopes.contains(scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        backup::BackupOptions,
        content::Content,
        processing::JobControl,
        store::{Kind, Store},
    };
    struct Proof;
    impl Authenticator for Proof {
        fn verify(&mut self, _: Id, _: &BTreeSet<Scope>) -> bool {
            true
        }
    }
    #[test]
    fn cached_backup_scope_validation_does_not_extend_an_unchanged_grants_expiry() {
        let public = Access::public();
        let mut source = Store::memory().unwrap();
        let note = source.create(Kind::Note, None, "expiry", &public).unwrap();
        source
            .append_content(note.id, Content::Text("x".repeat(200000)), &public)
            .unwrap();
        let mut current =
            Access::authenticate(&mut Proof, [Scope::Entity(note.id)].into_iter().collect())
                .unwrap();
        let revision = source.entity(note.id, &public).unwrap().revision;
        source.set_lock(note.id, revision, true, &current).unwrap();
        let plan = source
            .prepare_stream_backup(
                BackupOptions {
                    include_original_ink: true,
                    include_authorised_source_assets: false,
                },
                &current,
            )
            .unwrap();
        let control = JobControl::new(Duration::from_secs(30)).unwrap();
        let mut cursor = source
            .open_stream_backup_cursor(plan, &mut Proof, true, &control)
            .unwrap();
        assert!(cursor
            .next_chunk(&source, &current, &control)
            .unwrap()
            .is_some());
        let epoch = Arc::clone(current.grant_epoch());
        // Simulate clock expiry without replacing or relocking the capability.
        current.expires = Some(Instant::now());
        assert!(Arc::ptr_eq(&epoch, current.grant_epoch()));
        assert!(matches!(
            cursor.next_chunk(&source, &current, &control),
            Err(Error::Denied)
        ));
    }
}
