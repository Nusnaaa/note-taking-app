//! Consent-based, resumable local model-resource transfer. No URL fetching or code
//! loading. The host feeds explicitly selected bytes and retains the install ID.
use crate::{
    digest,
    processing::{Descriptor, Manager},
    Error, Id, Result,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    id: Id,
    descriptor: Descriptor,
    total: u64,
    committed: u64,
    complete: bool,
}
pub struct Transfer {
    root: PathBuf,
    state: State,
}
impl Transfer {
    pub fn begin(root: &Path, descriptor: Descriptor, total: u64, consented: bool) -> Result<Self> {
        if !consented {
            return Err(Error::Denied);
        }
        Manager::validate(&descriptor)?;
        let hash = descriptor
            .resource_sha256
            .as_deref()
            .ok_or(Error::Invalid("resource hash"))?;
        if hash.len() != 64
            || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            || total == 0
            || total > 16 * 1024 * 1024 * 1024
        {
            return Err(Error::Invalid("resource manifest"));
        }
        std::fs::create_dir_all(root)?;
        let transfer = Self {
            root: root.canonicalize()?,
            state: State {
                id: Id::new(),
                descriptor,
                total,
                committed: 0,
                complete: false,
            },
        };
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(transfer.part())?
            .sync_all()?;
        transfer.persist()?;
        Ok(transfer)
    }
    pub fn resume(root: &Path, id: Id) -> Result<Self> {
        let root = root.canonicalize()?;
        let meta = root.join(format!("{id}.install.json"));
        if std::fs::symlink_metadata(&meta)?.file_type().is_symlink()
            || std::fs::metadata(&meta)?.len() > 65536
        {
            return Err(Error::Integrity);
        }
        let state: State = serde_json::from_slice(&std::fs::read(meta)?)?;
        Manager::validate(&state.descriptor)?;
        if state.id != id || state.committed > state.total || state.total > 16 * 1024 * 1024 * 1024
        {
            return Err(Error::Integrity);
        }
        let transfer = Self { root, state };
        let path = transfer.part();
        if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(Error::Integrity);
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        if file.metadata()?.len() < transfer.state.committed {
            return Err(Error::Integrity);
        }
        // Bytes after the last durable receipt were never acknowledged.
        file.set_len(transfer.state.committed)?;
        file.sync_all()?;
        Ok(transfer)
    }
    pub fn id(&self) -> Id {
        self.state.id
    }
    pub fn committed_bytes(&self) -> u64 {
        self.state.committed
    }
    fn part(&self) -> PathBuf {
        self.root.join(format!("{}.part", self.state.id))
    }
    fn persist(&self) -> Result<()> {
        let temp = self
            .root
            .join(format!("{}.{}.tmp", self.state.id, Id::new()));
        let result = (|| -> Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(&serde_json::to_vec(&self.state)?)?;
            file.sync_all()?;
            std::fs::rename(
                &temp,
                self.root.join(format!("{}.install.json", self.state.id)),
            )?;
            std::fs::File::open(&self.root)?.sync_all()?;
            Ok(())
        })();
        let _ = std::fs::remove_file(temp);
        result
    }
    pub fn append(&mut self, offset: u64, bytes: &[u8]) -> Result<u64> {
        if self.state.complete {
            return Err(Error::Conflict);
        }
        if bytes.is_empty() || bytes.len() > 4 * 1024 * 1024 {
            return Err(Error::ResourceLimit);
        }
        let end = offset
            .checked_add(bytes.len() as u64)
            .ok_or(Error::ResourceLimit)?;
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.part())?;
        if offset < self.state.committed && end <= self.state.committed {
            let mut existing = vec![0; bytes.len()];
            file.seek(SeekFrom::Start(offset))?;
            file.read_exact(&mut existing)?;
            if existing != bytes {
                return Err(Error::Conflict);
            }
            return Ok(self.state.committed);
        }
        if offset != self.state.committed || end > self.state.total {
            return Err(Error::Conflict);
        }
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        let previous = self.state.committed;
        self.state.committed = end;
        if let Err(error) = self.persist() {
            self.state.committed = previous;
            return Err(error);
        }
        Ok(end)
    }
    pub fn finish(&mut self) -> Result<PathBuf> {
        if self.state.committed != self.state.total {
            return Err(Error::Conflict);
        }
        let mut file = std::fs::File::open(self.part())?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let size = file.read(&mut buffer)?;
            if size == 0 {
                break;
            }
            hash.update(&buffer[..size]);
        }
        if Some(format!("{:x}", hash.finalize()).as_str())
            != self.state.descriptor.resource_sha256.as_deref()
        {
            return Err(Error::Integrity);
        }
        let target = self.root.join(format!("{}.resource", self.state.id));
        if !target.exists() {
            std::fs::hard_link(self.part(), &target)?;
            std::fs::File::open(&self.root)?.sync_all()?;
        }
        // An interrupted finish may leave target; it must still match the source hash.
        let mut published = std::fs::File::open(&target)?;
        let mut h = Sha256::new();
        loop {
            let n = published.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            h.update(&buffer[..n]);
        }
        if Some(format!("{:x}", h.finalize()).as_str())
            != self.state.descriptor.resource_sha256.as_deref()
        {
            return Err(Error::Integrity);
        }
        self.state.complete = true;
        self.persist()?;
        Ok(target)
    }
    pub fn cancel(self) -> Result<()> {
        if self.state.complete {
            return Err(Error::ReviewRequired);
        }
        std::fs::remove_file(self.part())?;
        std::fs::remove_file(self.root.join(format!("{}.install.json", self.state.id)))?;
        std::fs::File::open(&self.root)?.sync_all()?;
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct ResourceLeases {
    state: Arc<Mutex<BTreeMap<PathBuf, usize>>>,
}
/// An owned read handle whose reservation survives transfer to another worker.
/// Fields drop in declaration order: close the descriptor before releasing the
/// reservation, so removal cannot race the final descriptor's retirement.
pub struct ResourceLease {
    file: std::fs::File,
    reservation: ResourceReservation,
}
struct ResourceReservation {
    state: Arc<Mutex<BTreeMap<PathBuf, usize>>>,
    path: PathBuf,
}
impl ResourceLeases {
    pub fn acquire(&self, path: &Path, expected_sha256: &str) -> Result<ResourceLease> {
        let path = path.canonicalize()?;
        let mut state = self.state.lock().map_err(|_| Error::WorkerFailed)?;
        let mut file = std::fs::File::open(&path)?;
        let mut h = Sha256::new();
        let mut buf = [0u8; 65536];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        if format!("{:x}", h.finalize()) != expected_sha256 {
            return Err(Error::Integrity);
        }
        file.seek(SeekFrom::Start(0))?;
        let count = state.entry(path.clone()).or_default();
        *count = count.checked_add(1).ok_or(Error::ResourceLimit)?;
        Ok(ResourceLease {
            file,
            reservation: ResourceReservation {
                state: self.state.clone(),
                path,
            },
        })
    }
    pub fn remove(&self, path: &Path) -> Result<()> {
        let path = path.canonicalize()?;
        let state = self.state.lock().map_err(|_| Error::WorkerFailed)?;
        if state.get(&path).copied().unwrap_or(0) > 0 {
            return Err(Error::Conflict);
        }
        std::fs::remove_file(path)?;
        Ok(())
    }
}
impl ResourceLease {
    /// Clone a guarded reader, never an untracked owned File. Like File::try_clone,
    /// descriptors share the underlying seek position; callers coordinate reads.
    pub fn file(&self) -> Result<Self> {
        let mut state = self
            .reservation
            .state
            .lock()
            .map_err(|_| Error::WorkerFailed)?;
        let file = self.file.try_clone()?;
        let count = state
            .get_mut(&self.reservation.path)
            .ok_or(Error::Integrity)?;
        *count = count.checked_add(1).ok_or(Error::ResourceLimit)?;
        Ok(Self {
            file,
            reservation: ResourceReservation {
                state: self.reservation.state.clone(),
                path: self.reservation.path.clone(),
            },
        })
    }
}
impl Read for ResourceLease {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buffer)
    }
}
impl Seek for ResourceLease {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.file.seek(position)
    }
}
impl Drop for ResourceReservation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(count) = state.get_mut(&self.path) {
                *count -= 1;
                if *count == 0 {
                    state.remove(&self.path);
                }
            }
        }
    }
}
/// Content-addressed comparison helper for host-side receipts; no content in logs.
pub fn resource_digest(bytes: &[u8]) -> String {
    digest(bytes)
}
