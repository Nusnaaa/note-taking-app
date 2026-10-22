//! Application access control, NOT separate encryption of the working SQLite file.
//! Android must supply a trusted platform authenticator; a model never supplies one.
use crate::{Error, Id, Result};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
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
        })
    }
    pub fn relock(&mut self) {
        self.scopes.clear();
        self.expires = None;
    }
    pub(crate) fn permits(&self, scope: &Scope) -> bool {
        self.expires.is_some_and(|until| Instant::now() < until) && self.scopes.contains(scope)
    }
}
