//! Stable cooperative resource coordination across registries and processes.
use super::*;
use std::fs::{File, OpenOptions, TryLockError};
use std::sync::atomic::{AtomicBool, Ordering};
pub(crate) struct ResourcePin {
    file: File,
    failed: AtomicBool,
}
pub(crate) struct ResourceExclusive {
    _file: File,
}
fn coordinate(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let name = path.file_name().ok_or(Error::Integrity)?;
    if name.to_string_lossy().starts_with(".stem-resource-")
        && name.to_string_lossy().ends_with(".lock")
    {
        return Err(Error::Invalid("resource coordination file"));
    }
    let key = digest(name.as_encoded_bytes());
    Ok(parent.join(format!(".stem-resource-{key}.lock")))
}
fn open(path: &Path, allow_symlink: bool) -> Result<File> {
    match std::fs::symlink_metadata(path) {
        Ok(meta)
            if meta.file_type().is_file() || allow_symlink && meta.file_type().is_symlink() => {}
        Ok(_) => return Err(Error::Integrity),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let coordinate = coordinate(path)?;
    let parent = coordinate.parent().ok_or(Error::Integrity)?;
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&coordinate)
    {
        Ok(file) => {
            file.sync_all()?;
            File::open(parent)?.sync_all()?;
            file
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if !std::fs::symlink_metadata(&coordinate)?
                .file_type()
                .is_file()
            {
                return Err(Error::Integrity);
            }
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(&coordinate)?
        }
        Err(e) => return Err(e.into()),
    };
    if !file.metadata()?.file_type().is_file() {
        return Err(Error::Integrity);
    }
    Ok(file)
}
fn locked(result: std::result::Result<(), TryLockError>) -> Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(Error::ResourceLimit),
        Err(TryLockError::Error(e)) => Err(e.into()),
    }
}
impl ResourcePin {
    pub(crate) fn acquire(path: &Path) -> Result<Arc<Self>> {
        if !std::fs::symlink_metadata(path)?.file_type().is_file() {
            return Err(Error::Integrity);
        }
        let file = open(path, false)?;
        locked(file.try_lock_shared())?;
        if file.metadata()?.len() != 0 {
            return Err(Error::ReviewRequired);
        }
        Ok(Arc::new(Self {
            file,
            failed: AtomicBool::new(false),
        }))
    }
    pub(crate) fn failed(self: &Arc<Self>) {
        if self.failed.swap(true, Ordering::AcqRel) {
            return;
        }
        // A failed provider destructor/callback may have left untracked work.
        // Persist a review marker when possible and retain this process's shared
        // reservation even if provider unwinding drops every normal owner.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut file = &self.file;
            let _ = file.write_all(b"provider-retirement-uncertain\n");
            let _ = file.sync_all();
        }));
        std::mem::forget(Arc::clone(self));
    }
}
impl ResourceExclusive {
    pub(crate) fn retirement_review_required(path: &Path) -> Result<bool> {
        match std::fs::symlink_metadata(coordinate(path)?) {
            Ok(meta) if meta.file_type().is_file() => Ok(meta.len() != 0),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Ok(_) => Err(Error::ReviewRequired),
            Err(e) => Err(e.into()),
        }
    }
    pub(crate) fn reclamation(
        path: &Path,
        reviewed_failure: bool,
        reviewed_symlink: bool,
    ) -> Result<Self> {
        let file = open(path, reviewed_symlink)?;
        locked(file.try_lock())?;
        if file.metadata()?.len() != 0 && !reviewed_failure {
            return Err(Error::ReviewRequired);
        }
        Ok(Self { _file: file })
    }

    pub(crate) fn acquire(path: &Path) -> Result<Self> {
        let file = open(path, false)?;
        locked(file.try_lock())?;
        if file.metadata()?.len() != 0 {
            return Err(Error::ReviewRequired);
        }
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exclusive_guard_blocks_new_readers_and_pins_the_inode_after_payload_deletion() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("model.resource");
        std::fs::write(&path, b"model").unwrap();
        let owner = ResourceExclusive::acquire(&path).unwrap();
        assert!(matches!(
            ResourcePin::acquire(&path),
            Err(Error::ResourceLimit)
        ));
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(
            ResourceExclusive::acquire(&path),
            Err(Error::ResourceLimit)
        ));
        drop(owner);
        let owner = ResourceExclusive::acquire(&path).unwrap();
        std::fs::write(&path, b"model").unwrap();
        assert!(matches!(
            ResourcePin::acquire(&path),
            Err(Error::ResourceLimit)
        ));
        drop(owner);
        drop(ResourcePin::acquire(&path).unwrap());
    }
    #[test]
    fn coordination_names_and_symlink_or_nonempty_control_files_are_never_adopted_or_removed() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("model.resource");
        std::fs::write(&path, b"model").unwrap();
        drop(ResourcePin::acquire(&path).unwrap());
        let control = root
            .path()
            .join(format!(".stem-resource-{}.lock", digest(b"model.resource")));
        assert!(matches!(
            ResourceLeases::default().remove(&control),
            Err(Error::Invalid(_))
        ));
        std::fs::write(&control, b"uncertain retirement").unwrap();
        assert!(matches!(
            ResourcePin::acquire(&path),
            Err(Error::ReviewRequired)
        ));
        assert!(matches!(
            ResourceExclusive::acquire(&path),
            Err(Error::ReviewRequired)
        ));
        #[cfg(unix)]
        {
            std::fs::remove_file(&control).unwrap();
            let outside = root.path().join("outside");
            std::fs::write(&outside, b"independent").unwrap();
            std::os::unix::fs::symlink(&outside, &control).unwrap();
            assert!(matches!(ResourcePin::acquire(&path), Err(Error::Integrity)));
            assert_eq!(std::fs::read(&outside).unwrap(), b"independent");
        }
    }
}
