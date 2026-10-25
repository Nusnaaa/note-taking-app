//! Incremental fixed-table logical backup. Only encrypted output leaves this
//! module. Restore stages in a private, independent SQLite file; publication
//! remains explicit and never overwrites another library.
mod pull;
use super::*;
use crate::{
    crypto::stream::{DecryptStream, EncryptStream},
    processing::JobControl,
};
pub use pull::StreamBackupCursor;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};
use zeroize::Zeroizing;
const BODY: &[u8; 8] = b"STEMLIB2";
const END: &[u8; 8] = b"STEMEND2";
const CELL_LIMIT: usize = 64 * 1024 * 1024;
const ROW_LIMIT: u64 = 1_000_000;
const ASSET_OMISSION: &str =
    "External PDF bytes or disposable source cache excluded; source relationship retained.";
const MODEL_OMISSION: &str =
    "Optional model resource excluded; reinstall and verify locally after restore.";
/// Review metadata only, not an in-memory copy of the library. Generation and
/// library binding are checked again before taking the consistent read snapshot.
pub struct StreamBackupPlan {
    library: String,
    generation: String,
    scopes: BTreeSet<Scope>,
    options: BackupOptions,
    includes_original_ink: bool,
    source_assets: usize,
    omissions: Vec<String>,
}
impl StreamBackupPlan {
    pub(crate) fn admission_bytes(&self) -> Result<usize> {
        self.scopes
            .len()
            .checked_mul(64)
            .and_then(|n| n.checked_add(self.library.len() + self.generation.len() + 1024))
            .ok_or(Error::ResourceLimit)
    }
    pub fn includes_original_ink(&self) -> bool {
        self.includes_original_ink
    }
    pub fn source_assets(&self) -> usize {
        self.source_assets
    }
    pub fn omissions(&self) -> &[String] {
        &self.omissions
    }
}
fn include_asset(class: &str, authority: Option<&str>, options: BackupOptions) -> Result<bool> {
    let class: AssetClass = serde_json::from_str(class)?;
    if class == AssetClass::LiveCache || !options.include_authorised_source_assets {
        return Ok(false);
    }
    let authority: Retention = serde_json::from_str(authority.ok_or(Error::Denied)?)?;
    if authority.authority.is_empty() || !authority.export_allowed {
        return Err(Error::Denied);
    }
    Ok(true)
}
fn write_bytes(out: &mut impl Write, value: &[u8]) -> Result<()> {
    if value.len() > CELL_LIMIT {
        return Err(Error::ResourceLimit);
    }
    out.write_all(&(value.len() as u64).to_be_bytes())?;
    out.write_all(value)?;
    Ok(())
}
fn read_u64(input: &mut impl Read) -> Result<u64> {
    let mut bytes = [0; 8];
    input.read_exact(&mut bytes)?;
    Ok(u64::from_be_bytes(bytes))
}
fn read_bytes(input: &mut impl Read, limit: u64) -> Result<Vec<u8>> {
    let len = read_u64(input)?;
    if len > (CELL_LIMIT as u64).min(limit) {
        return Err(Error::ResourceLimit);
    }
    let mut bytes = vec![0; len as usize];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn text(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes).map_err(|_| Error::Integrity)
}
impl Store {
    pub fn prepare_stream_backup(
        &self,
        options: BackupOptions,
        access: &Access,
    ) -> Result<StreamBackupPlan> {
        Self::require_app(&self.db, access)?;
        let tx = self.db.unchecked_transaction()?;
        let library = Self::setting(&tx, "library_id")?;
        let generation = Self::setting(&tx, "generation")?;
        let scopes = self.backup_scopes()?;
        if scopes.iter().any(|s| !access.permits(s)) {
            return Err(Error::Denied);
        }
        let ink = tx.prepare("SELECT 1 FROM strokes LIMIT 1")?.exists([])?;
        if ink && !options.include_original_ink {
            return Err(Error::ReviewRequired);
        }
        let mut source_assets = 0;
        let mut omissions = BTreeSet::new();
        {
            let mut q = tx.prepare("SELECT class,authority FROM source_assets")?;
            for row in q.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })? {
                let (class, authority) = row?;
                if include_asset(&class, authority.as_deref(), options)? {
                    source_assets += 1;
                } else {
                    omissions.insert(ASSET_OMISSION.to_string());
                }
            }
        }
        {
            let mut q = tx.prepare("SELECT payload FROM engine_versions")?;
            for row in q.query_map([], |r| r.get::<_, String>(0))? {
                if serde_json::from_str::<EngineVersion>(&row?)?
                    .local_resource
                    .is_some()
                {
                    omissions.insert(MODEL_OMISSION.to_string());
                }
            }
        }
        tx.commit()?;
        Ok(StreamBackupPlan {
            library,
            generation,
            scopes,
            options,
            includes_original_ink: ink,
            source_assets,
            omissions: omissions.into_iter().collect(),
        })
    }
    /// Execute on an optional worker. Sink receives encrypted bytes only, may
    /// contain an incomplete archive after failure, and must not be published as
    /// complete until this returns success. Control bounds cooperative work.
    pub fn finish_stream_backup<W: Write>(
        &self,
        plan: StreamBackupPlan,
        password: &str,
        auth: &mut dyn Authenticator,
        confirmed: bool,
        sink: W,
        control: &JobControl,
    ) -> Result<(W, Zeroizing<String>)> {
        let mut cursor = self.open_stream_backup_cursor(plan, auth, confirmed, control)?;
        let mut out = EncryptStream::new(sink, password)?;
        while let Some(chunk) = cursor.read_chunk(self, None, control)? {
            out.write_all(&chunk)?;
        }
        cursor.check_complete(self, None, control)?;
        let finished = out.finish()?;
        cursor.check_complete(self, None, control)?;
        Ok(finished)
    }

    /// Stage a current-schema stream in a private directory. Caller chooses a
    /// finite decrypted-byte budget; malformed/incomplete streams cannot return
    /// a usable Store. The live library is never opened or overwritten here.
    pub fn restore_stream_backup<R: Read>(
        source: R,
        credential: crypto::Credential<'_>,
        max_plaintext_bytes: u64,
        private_directory: &Path,
        control: &JobControl,
    ) -> Result<StagedBackupRestore> {
        control.check()?;
        let mut input = DecryptStream::new(source, credential, max_plaintext_bytes)?;
        let metadata = fs::symlink_metadata(private_directory)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Invalid("private restore directory"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(Error::Denied);
            }
        }
        let path = private_directory.join(format!(".stem-stream-restore-{}.db", Id::new()));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path)?;
        drop(file);
        let mut stage = StagedBackupRestore { store: None, path };
        stage.store = Some(Store::open(&stage.path)?);
        {
            let destination = stage.store.as_mut().ok_or(Error::Integrity)?;
            let tx = destination.db.transaction()?;
            tx.execute_batch("PRAGMA defer_foreign_keys=ON; DELETE FROM workspace_panes; DELETE FROM workspace_state; DELETE FROM settings;")?;
            let mut magic = [0; 8];
            input.read_exact(&mut magic)?;
            let mut schema = [0; 4];
            input.read_exact(&mut schema)?;
            if &magic != BODY || u32::from_be_bytes(schema) != 15 {
                return Err(Error::Unsupported);
            }
            let mut omissions = [0; 4];
            input.read_exact(&mut omissions)?;
            let omissions = u32::from_be_bytes(omissions);
            if omissions > 8 {
                return Err(Error::ResourceLimit);
            }
            for _ in 0..omissions {
                let omission = text(read_bytes(&mut input, 4096)?)?;
                if !matches!(omission.as_str(), ASSET_OMISSION | MODEL_OMISSION) {
                    return Err(Error::Unsupported);
                }
            }
            let mut count = 0u64;
            for (index, &(table, width)) in TABLES.iter().enumerate() {
                control.check()?;
                let mut marker = [0; 2];
                input.read_exact(&mut marker)?;
                if u16::from_be_bytes(marker) as usize != index {
                    return Err(Error::Integrity);
                }
                let placeholders = (1..=width)
                    .map(|n| format!("?{n}"))
                    .collect::<Vec<_>>()
                    .join(",");
                let mut statement =
                    tx.prepare(&format!("INSERT INTO {table} VALUES({placeholders})"))?;
                loop {
                    control.check()?;
                    let mut present = [0];
                    input.read_exact(&mut present)?;
                    if present[0] == 0 {
                        break;
                    }
                    if present[0] != 1 {
                        return Err(Error::Integrity);
                    }
                    count = count
                        .checked_add(1)
                        .filter(|n| *n <= ROW_LIMIT)
                        .ok_or(Error::ResourceLimit)?;
                    let mut values = Vec::with_capacity(width);
                    for _ in 0..width {
                        control.check()?;
                        let mut kind = [0];
                        input.read_exact(&mut kind)?;
                        values.push(match kind[0] {
                            0 => Value::Null,
                            1 => {
                                let mut bytes = [0; 8];
                                input.read_exact(&mut bytes)?;
                                Value::Integer(i64::from_be_bytes(bytes))
                            }
                            2 => Value::Text(text(read_bytes(&mut input, max_plaintext_bytes)?)?),
                            3 => Value::Blob(read_bytes(&mut input, max_plaintext_bytes)?),
                            _ => return Err(Error::Integrity),
                        });
                    }
                    statement.execute(params_from_iter(values))?;
                }
            }
            input.read_exact(&mut magic)?;
            if &magic != END || read_u64(&mut input)? != count {
                return Err(Error::Integrity);
            }
            let mut tail = [0];
            if input.read(&mut tail)? != 0 {
                return Err(Error::Integrity);
            }
            input.finish()?; // authenticated finality and exact encrypted EOF BEFORE commit
            control.check()?;
            tx.commit()?;
            destination.validate_backup_state_controlled(9, Some(control))?;
            control.check()?;
            destination.retire_restored_runtime_state(Some(control))?;
            checkpoint_stage(&destination.db)?;
        }
        control.check()?;
        Ok(stage)
    }
}
// FULL synchronous SQLite commits/checkpoint own database syncing and locks.
// A second raw database descriptor can disturb process-scoped POSIX SQLite locks
// when closed. Check the checkpoint result rather than opening the live stage.
pub(super) fn checkpoint_stage(db: &rusqlite::Connection) -> Result<()> {
    let (busy, frames, checkpointed): (i64, i64, i64) =
        db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    if busy != 0 || frames < 0 || frames != checkpointed {
        return Err(Error::Conflict);
    }
    Ok(())
}

/// Owns private scratch through validation/review/publication. Drop closes SQLite
/// before removing its own random scratch files. Publishing uses the existing
/// fresh-authenticated, atomic no-overwrite restore operation.
pub struct StagedBackupRestore {
    store: Option<Store>,
    path: PathBuf,
}
impl StagedBackupRestore {
    /// Aggregate metadata only. Protected titles/payloads are not exposed by review.
    pub fn summary(&self) -> Result<RestoreSummary> {
        let store = self.store();
        let count = |table: &str| -> Result<u64> {
            Ok(store
                .db
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?)
        };
        let mut protected = u64::from(Store::setting(&store.db, "app_locked")? == "true");
        for table in ["entities", "blocks", "exercises"] {
            protected += store.db.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE json_extract(payload,'$.locked')=1"),
                [],
                |r| r.get::<_, u64>(0),
            )?;
        }
        Ok(RestoreSummary {
            entities: count("entities")?,
            blocks: count("blocks")?,
            strokes: count("strokes")?,
            source_references: count("sources")?,
            source_assets: count("source_assets")?,
            protected_scopes: protected,
        })
    }
    /// Trusted recovered-copy acceptance. Fresh platform verification is required
    /// even for an unprotected archive; protection flags remain in the new copy.
    pub fn publish(
        &self,
        destination: &Path,
        auth: &mut dyn Authenticator,
        confirmed: bool,
        control: &JobControl,
    ) -> Result<RestorePublication> {
        if !confirmed {
            return Err(Error::ReviewRequired);
        }
        control.check()?;
        private_directory(destination.parent().ok_or(Error::Invalid("restore path"))?)?;
        let mut scopes = self.store().backup_scopes()?;
        scopes.insert(Scope::App);
        let access = Access::authenticate(auth, scopes)?;
        control.check()?;
        self.store()
            .persist_verified_restore(destination, &access, Some(control))
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct RestoreSummary {
    pub entities: u64,
    pub blocks: u64,
    pub strokes: u64,
    pub source_references: u64,
    pub source_assets: u64,
    pub protected_scopes: u64,
}
fn private_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Invalid("private restore directory"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(Error::Denied);
        }
    }
    Ok(())
}
impl Store {
    /// Current streaming envelopes and bounded legacy envelopes share validation,
    /// runtime/grant retirement and a private stage. The input never becomes SQL.
    pub fn restore_any_backup<R: Read>(
        mut source: R,
        credential: crypto::Credential<'_>,
        max_plaintext_bytes: u64,
        private_directory_path: &Path,
        control: &JobControl,
    ) -> Result<StagedBackupRestore> {
        private_directory(private_directory_path)?;
        control.check()?;
        let mut magic = [0; 8];
        source.read_exact(&mut magic)?;
        let mut input = std::io::Cursor::new(magic).chain(source);
        if &magic == b"STEMENC2" {
            return Self::restore_stream_backup(
                input,
                credential,
                max_plaintext_bytes,
                private_directory_path,
                control,
            );
        }
        if &magic != b"STEMENC1" {
            return Err(Error::Unsupported);
        }
        let maximum = usize::try_from(max_plaintext_bytes.min(64 * 1024 * 1024))
            .map_err(|_| Error::ResourceLimit)?
            + crypto::LEGACY_HEADER_BYTES
            + 16;
        let mut envelope = Vec::new();
        let mut chunk = [0; 65536];
        loop {
            control.check()?;
            let count = input.read(&mut chunk)?;
            if count == 0 {
                break;
            }
            if count > maximum - envelope.len() {
                return Err(Error::ResourceLimit);
            }
            envelope.extend_from_slice(&chunk[..count]);
        }
        let plain = crypto::decrypt(&envelope, credential)?;
        if plain.len() as u64 > max_plaintext_bytes {
            return Err(Error::ResourceLimit);
        }
        control.check()?;
        let memory = Self::restore_backup_plain_controlled(&plain, Some(control))?;
        control.check()?;
        let path = private_directory_path.join(format!(".stem-stream-restore-{}.db", Id::new()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        drop(options.open(&path)?);
        let mut stage = StagedBackupRestore { store: None, path };
        memory.db.execute(
            "VACUUM INTO ?1",
            [stage.path.to_str().ok_or(Error::Invalid("restore path"))?],
        )?;
        drop(memory);
        stage.store = Some(Self::open(&stage.path)?);
        control.check()?;
        Ok(stage)
    }
}
impl StagedBackupRestore {
    pub fn store(&self) -> &Store {
        self.store.as_ref().expect("validated restore stage")
    }
    pub fn store_mut(&mut self) -> &mut Store {
        self.store.as_mut().expect("validated restore stage")
    }
}
impl Drop for StagedBackupRestore {
    fn drop(&mut self) {
        drop(self.store.take());
        for suffix in ["-wal", "-shm", ""] {
            let mut path = self.path.as_os_str().to_os_string();
            path.push(suffix);
            let _ = fs::remove_file(PathBuf::from(path));
        }
    }
}
