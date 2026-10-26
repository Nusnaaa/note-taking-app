//! Exclusive, explicitly authorised removal of known unpublished transfer files.
use super::*;
pub(crate) struct DiscardFiles {
    root: PathBuf,
    state: State,
    _ownership: Arc<TransferOwnership>,
}
impl DiscardFiles {
    pub(crate) fn prepare(
        root: &Path,
        id: Id,
        library: &str,
        prior: Option<(&Descriptor, u64, u64, i64)>,
    ) -> Result<Self> {
        if !std::fs::symlink_metadata(root)?.file_type().is_dir() {
            return Err(Error::Integrity);
        }
        let root = root.canonicalize()?;
        let read = || -> Result<State> {
            let state = match Transfer::read_resume_state(&root, id, None) {
                Ok(state) => state,
                Err(Error::Io(error))
                    if error.kind() == std::io::ErrorKind::NotFound && prior.is_some() =>
                {
                    for suffix in ["part", "resource"] {
                        match std::fs::symlink_metadata(root.join(format!("{id}.{suffix}"))) {
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                            Ok(_) => return Err(Error::ReviewRequired),
                            Err(e) => return Err(e.into()),
                        }
                    }
                    let (descriptor, total, committed, consent_ms) =
                        prior.ok_or(Error::ReviewRequired)?;
                    State {
                        id,
                        descriptor: descriptor.clone(),
                        total,
                        committed,
                        complete: false,
                        binding: Some(TransferBinding {
                            library: library.into(),
                            consent_ms,
                        }),
                    }
                }
                Err(error) => return Err(error),
            };
            let binding = state.binding.as_ref().ok_or(Error::ReviewRequired)?;
            if Id::parse(&binding.library)? != Id::parse(library)? {
                return Err(Error::Conflict);
            }
            if let Some((descriptor, total, _, consent)) = prior {
                if state.descriptor != *descriptor
                    || state.total != total
                    || binding.consent_ms != consent
                {
                    return Err(Error::Conflict);
                }
            }
            Ok(state)
        };
        read()?;
        let ownership = TransferOwnership::acquire(&root, id)?;
        let state = read()?;
        let files = Self {
            root,
            state,
            _ownership: ownership,
        };
        files.check_files()?;
        Ok(files)
    }
    fn check_files(&self) -> Result<()> {
        for suffix in ["resource", "part", "install.json"] {
            match std::fs::symlink_metadata(self.root.join(format!("{}.{suffix}", self.state.id))) {
                Ok(meta) if meta.file_type().is_file() => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => return Err(Error::ReviewRequired),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
    pub(crate) fn descriptor(&self) -> &Descriptor {
        &self.state.descriptor
    }
    pub(crate) fn total(&self) -> u64 {
        self.state.total
    }
    pub(crate) fn committed(&self) -> u64 {
        self.state.committed
    }
    pub(crate) fn consent_ms(&self) -> Result<i64> {
        self.state
            .binding
            .as_ref()
            .map(|b| b.consent_ms)
            .ok_or(Error::ReviewRequired)
    }
    pub(crate) fn remove(&self, mut check: impl FnMut() -> Result<()>) -> Result<()> {
        check()?;
        self.check_files()?;
        // Ledger is last: an interrupted operation preserves its binding for retry.
        for suffix in ["resource", "part", "install.json"] {
            check()?;
            let path = self.root.join(format!("{}.{suffix}", self.state.id));
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_file() => std::fs::remove_file(path)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => return Err(Error::ReviewRequired),
                Err(error) => return Err(error.into()),
            }
        }
        check()?;
        std::fs::File::open(&self.root)?.sync_all()?;
        check()
    }
}
