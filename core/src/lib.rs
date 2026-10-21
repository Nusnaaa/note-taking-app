//! Authoritative, offline notebook core. No network or production H.I. is present.
//! Models receive immutable snapshots, never the database connection.
#![forbid(unsafe_code)]

pub mod content;
pub mod converters;
pub mod crypto;
pub mod help;
pub mod ink;
pub mod portable;
pub mod processing;
pub mod security;
pub mod sources;
pub mod store;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(Uuid);

impl Id {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
    pub fn parse(value: &str) -> Result<Self> {
        Uuid::parse_str(value)
            .map(Self)
            .map_err(|_| Error::Invalid("identifier"))
    }
}
impl Default for Id {
    fn default() -> Self {
        Self::new()
    }
}
impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Error text is deliberately content-free. Host diagnostics must not dump requests.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid {0}")]
    Invalid(&'static str),
    #[error("item unavailable")]
    Unavailable,
    #[error("authentication required")]
    Denied,
    #[error("source changed; reload before applying this operation")]
    Conflict,
    #[error("dependency review required")]
    ReviewRequired,
    #[error("storage operation failed")]
    Storage(#[from] rusqlite::Error),
    #[error("invalid structured payload")]
    Json(#[from] serde_json::Error),
    #[error("unsupported format or schema version")]
    Unsupported,
    #[error("operation cancelled")]
    Cancelled,
    #[error("operation timed out")]
    Timeout,
    #[error("engine unavailable or incompatible")]
    EngineUnavailable,
    #[error("worker failed; original content is unchanged")]
    WorkerFailed,
    #[error("resource limit reached")]
    ResourceLimit,
    #[error("integrity or credential verification failed")]
    Integrity,
    #[error("injected interruption")]
    Interrupted,
}
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
