//! Review and exclusive worker-only removal of a published, logically Removed resource.
use super::*;
use std::fs::File;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceEntryKind {
    Missing,
    File,
    Symlink,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceEntry {
    pub kind: ResourceEntryKind,
    pub bytes: Option<u64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsentProvenance {
    BoundLedger,
    RecordedDecision,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReclamationFacts {
    pub resource: ResourceEntry,
    pub part: ResourceEntry,
    pub ledger: ResourceEntry,
    pub consent_provenance: ConsentProvenance,
    pub original_import_consent_ms: Option<i64>,
    pub binding_review_required: bool,
    pub retirement_review_required: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReclamationConsent {
    pub confirmed: bool,
    pub reviewed_unknown_binding: bool,
    pub reviewed_failed_retirement: bool,
    pub reviewed_symlinks: bool,
}
impl ReclamationFacts {
    pub(crate) fn accept(&self, consent: ReclamationConsent) -> Result<()> {
        if !consent.confirmed {
            return Err(Error::Denied);
        }
        if self.binding_review_required && !consent.reviewed_unknown_binding
            || self.retirement_review_required && !consent.reviewed_failed_retirement
            || [&self.resource, &self.part, &self.ledger]
                .iter()
                .any(|r| r.kind == ResourceEntryKind::Symlink)
                && !consent.reviewed_symlinks
        {
            return Err(Error::ReviewRequired);
        }
        Ok(())
    }
}
pub(crate) struct ReclamationLocation {
    root: PathBuf,
    directory: File,
    id: Id,
    library: Id,
    descriptor: Descriptor,
    total: u64,
    prior_consent: Option<i64>,
}
pub(crate) struct ReclamationFiles {
    location: ReclamationLocation,
    facts: ReclamationFacts,
    _transfer: Arc<TransferOwnership>,
    _resource: ResourceExclusive,
}
fn entry(path: &Path) -> Result<ResourceEntry> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => Ok(ResourceEntry {
            kind: ResourceEntryKind::File,
            bytes: Some(meta.len()),
        }),
        Ok(meta) if meta.file_type().is_symlink() => Ok(ResourceEntry {
            kind: ResourceEntryKind::Symlink,
            bytes: None,
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ResourceEntry {
            kind: ResourceEntryKind::Missing,
            bytes: None,
        }),
        Ok(_) => Err(Error::ReviewRequired),
        Err(e) => Err(e.into()),
    }
}
impl ReclamationLocation {
    pub(crate) fn review(
        root: &Path,
        id: Id,
        library: Id,
        descriptor: Descriptor,
        total: u64,
        expected: &str,
        prior_consent: Option<i64>,
    ) -> Result<(Self, ReclamationFacts)> {
        if !std::fs::symlink_metadata(root)?.file_type().is_dir() {
            return Err(Error::Integrity);
        }
        let root = root.canonicalize()?;
        let expected = Path::new(expected);
        if expected.file_name() != Some(std::ffi::OsStr::new(&format!("{id}.resource")))
            || expected
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .canonicalize()?
                != root
        {
            return Err(Error::ReviewRequired);
        }
        let directory = File::open(&root)?;
        let location = Self {
            root,
            directory,
            id,
            library,
            descriptor,
            total,
            prior_consent,
        };
        let facts = location.observe()?;
        Ok((location, facts))
    }
    fn path(&self, suffix: &str) -> PathBuf {
        self.root.join(format!("{}.{suffix}", self.id))
    }
    fn current(&self) -> Result<()> {
        let now = std::fs::symlink_metadata(&self.root)?;
        if !now.file_type().is_dir() {
            return Err(Error::Conflict);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let before = self.directory.metadata()?;
            if now.dev() != before.dev() || now.ino() != before.ino() {
                return Err(Error::Conflict);
            }
        }
        #[cfg(not(unix))]
        {
            return Err(Error::Unsupported);
        }
        Ok(())
    }
    fn observe(&self) -> Result<ReclamationFacts> {
        self.current()?;
        let resource = entry(&self.path("resource"))?;
        let part = entry(&self.path("part"))?;
        let ledger = entry(&self.path("install.json"))?;
        let mut original = None;
        let mut provenance = ConsentProvenance::Unknown;
        if ledger.kind == ResourceEntryKind::File {
            // Invalid/legacy metadata requires explicit review. Known foreign
            // library binding is never overridden by the review flags.
            if let Ok(state) = Transfer::read_resume_state(&self.root, self.id, None) {
                if let Some(binding) = state.binding {
                    if Id::parse(&binding.library)? != self.library {
                        return Err(Error::Conflict);
                    }
                    if state.descriptor == self.descriptor && state.total == self.total {
                        original = Some(binding.consent_ms);
                        provenance = ConsentProvenance::BoundLedger;
                    }
                }
            }
        }
        if provenance == ConsentProvenance::Unknown
            && [resource.kind, part.kind, ledger.kind]
                .iter()
                .all(|k| *k == ResourceEntryKind::Missing)
            && self.prior_consent.is_some()
        {
            original = self.prior_consent;
            provenance = ConsentProvenance::RecordedDecision;
        }
        let retirement_review_required =
            ResourceExclusive::retirement_review_required(&self.path("resource"))?;
        Ok(ReclamationFacts {
            resource,
            part,
            ledger,
            consent_provenance: provenance,
            original_import_consent_ms: original,
            binding_review_required: provenance == ConsentProvenance::Unknown,
            retirement_review_required,
        })
    }
    pub(crate) fn prepare(
        self,
        facts: &ReclamationFacts,
        consent: ReclamationConsent,
    ) -> Result<ReclamationFiles> {
        facts.accept(consent)?;
        self.current()?;
        let transfer = TransferOwnership::acquire(&self.root, self.id)?;
        let resource = ResourceExclusive::reclamation(
            &self.path("resource"),
            consent.reviewed_failed_retirement,
            consent.reviewed_symlinks,
        )?;
        if self.observe()? != *facts {
            return Err(Error::Conflict);
        }
        Ok(ReclamationFiles {
            location: self,
            facts: facts.clone(),
            _transfer: transfer,
            _resource: resource,
        })
    }
}
impl ReclamationFiles {
    pub(crate) fn remove(&self, mut check: impl FnMut() -> Result<()>) -> Result<()> {
        check()?;
        if self.location.observe()? != self.facts {
            return Err(Error::Conflict);
        }
        for (suffix, observed) in [
            ("resource", &self.facts.resource),
            ("part", &self.facts.part),
            ("install.json", &self.facts.ledger),
        ] {
            check()?;
            self.location.current()?;
            let path = self.location.path(suffix);
            if entry(&path)? != *observed {
                return Err(Error::Conflict);
            }
            if observed.kind != ResourceEntryKind::Missing {
                std::fs::remove_file(path)?;
            }
        }
        check()?;
        self.location.current()?;
        self.location.directory.sync_all()?;
        check()
    }
}
