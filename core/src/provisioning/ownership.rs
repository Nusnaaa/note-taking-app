//! Stable advisory coordination inode. Never unlink/replace while the depot may
//! be in use: a new inode could admit a second owner alongside an old lock.
use super::*;
use std::fs::{File, OpenOptions, TryLockError};
pub(crate) struct TransferOwnership {
    _file: File,
}
impl TransferOwnership {
    pub(super) fn acquire(root: &Path, id: Id) -> Result<Arc<Self>> {
        let path = root.join(format!("{id}.owner.lock"));
        let created;
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                created = true;
                file
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = std::fs::symlink_metadata(&path)?;
                if !metadata.file_type().is_file() || metadata.len() != 0 {
                    return Err(Error::Integrity);
                }
                created = false;
                OpenOptions::new().read(true).write(true).open(&path)?
            }
            Err(error) => return Err(error.into()),
        };
        if !file.metadata()?.file_type().is_file() || file.metadata()?.len() != 0 {
            return Err(Error::Integrity);
        }
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(Error::ResourceLimit),
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
        if created {
            file.sync_all()?;
            File::open(root)?.sync_all()?;
        }
        Ok(Arc::new(Self { _file: file }))
    }
}
