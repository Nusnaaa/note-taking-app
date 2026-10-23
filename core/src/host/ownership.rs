//! Cooperative native-host ownership. The lock follows the database inode, not a
//! sidecar path, and is explicitly unlocked even if a child inherited the FD.
use crate::{Error, Result};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};

pub(super) struct LibraryLease {
    file: File,
    released: bool,
}
impl LibraryLease {
    pub fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.try_lock().map_err(|_| Error::Unavailable)?;
        Ok(Self {
            file,
            released: false,
        })
    }
    pub fn release(mut self) -> Result<()> {
        self.file.unlock()?;
        self.released = true;
        Ok(())
    }
}
impl Drop for LibraryLease {
    fn drop(&mut self) {
        if !self.released {
            let _ = self.file.unlock();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inherited_descriptor_does_not_extend_released_library_ownership() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("ownership.db");
        let lease = LibraryLease::acquire(&path).unwrap();
        let inherited = lease.file.try_clone().unwrap();
        assert!(LibraryLease::acquire(&path).is_err());
        drop(lease);
        // A fork/dup may retain the same open file description. Closing only the
        // parent's FD would leave that inherited description holding the lock.
        let reopened = LibraryLease::acquire(&path).unwrap();
        drop(inherited);
        assert!(LibraryLease::acquire(&path).is_err());
        reopened.release().unwrap();
        LibraryLease::acquire(&path).unwrap().release().unwrap();
    }
}
