//! Bounded, resumable pulls through the authoritative connection. No read
//! transaction, SQL statement, encryptor, password or caller sink survives a pull.
use super::*;
const PULL_BYTES: usize = 64 * 1024;
const PULL_STEPS: usize = 64;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    TableStart,
    Rows,
    TableEnd,
    Footer,
    Done,
}
/// Non-deserialisable read capability for trusted backup transport. Pulls yield
/// at most 64 KiB and perform at most 64 cursor steps. Empty Some means yield,
/// not EOF. Generation changes permanently poison a cursor; retry from review.
/// This does not grant access to model code and is never a JSON input/output.
pub struct StreamBackupCursor {
    plan: StreamBackupPlan,
    proof: Access,
    table: usize,
    last_row: Option<i64>,
    row: Option<(i64, usize)>,
    count: u64,
    phase: Phase,
    pending: Vec<u8>,
    pending_offset: usize,
    failed: bool,
    engine_payload: Option<Vec<u8>>,
}
impl Store {
    pub fn open_stream_backup_cursor(
        &self,
        plan: StreamBackupPlan,
        auth: &mut dyn Authenticator,
        confirmed: bool,
        control: &JobControl,
    ) -> Result<StreamBackupCursor> {
        if !confirmed {
            return Err(Error::ReviewRequired);
        }
        control.check()?;
        let matches = || -> Result<()> {
            if Self::setting(&self.db, "library_id")? != plan.library
                || Self::setting(&self.db, "generation")? != plan.generation
            {
                return Err(Error::Conflict);
            }
            Ok(())
        };
        matches()?;
        let proof = if plan.scopes.is_empty() {
            Access::public()
        } else {
            Access::authenticate(auth, plan.scopes.clone())?
        };
        matches()?;
        control.check()?;
        let mut pending = BODY.to_vec();
        pending.extend_from_slice(&15u32.to_be_bytes());
        pending.extend_from_slice(&(plan.omissions.len() as u32).to_be_bytes());
        for omission in &plan.omissions {
            write_bytes(&mut pending, omission.as_bytes())?;
        }
        Ok(StreamBackupCursor {
            plan,
            proof,
            table: 0,
            last_row: None,
            row: None,
            count: 0,
            phase: Phase::TableStart,
            pending,
            pending_offset: 0,
            failed: false,
            engine_payload: None,
        })
    }
}
impl StreamBackupCursor {
    fn poison(&mut self) {
        self.failed = true;
        self.proof.relock();
        self.pending.clear();
        self.pending_offset = 0;
        self.engine_payload = None;
    }
    fn check(&self, store: &Store, current: Option<&Access>, control: &JobControl) -> Result<()> {
        control.check()?;
        if self.failed {
            return Err(Error::Cancelled);
        }
        for scope in &self.plan.scopes {
            if !self.proof.permits(scope) || current.is_some_and(|a| !a.permits(scope)) {
                return Err(Error::Denied);
            }
        }
        if let Some(current) = current {
            Store::require_app(&store.db, current)?;
        }
        if Store::setting(&store.db, "library_id")? != self.plan.library
            || Store::setting(&store.db, "generation")? != self.plan.generation
        {
            return Err(Error::Conflict);
        }
        Ok(())
    }
    pub fn next_chunk(
        &mut self,
        store: &Store,
        current: &Access,
        control: &JobControl,
    ) -> Result<Option<Vec<u8>>> {
        self.read_chunk(store, Some(current), control)
    }
    pub fn validate_complete(
        &mut self,
        store: &Store,
        current: &Access,
        control: &JobControl,
    ) -> Result<()> {
        let result = self.check_complete(store, Some(current), control);
        if result.is_err() {
            self.poison();
        }
        result
    }
    pub(super) fn check_complete(
        &self,
        store: &Store,
        current: Option<&Access>,
        control: &JobControl,
    ) -> Result<()> {
        self.check(store, current, control)?;
        if self.phase != Phase::Done || self.pending_offset != self.pending.len() {
            return Err(Error::ReviewRequired);
        }
        Ok(())
    }
    pub(super) fn read_chunk(
        &mut self,
        store: &Store,
        current: Option<&Access>,
        control: &JobControl,
    ) -> Result<Option<Vec<u8>>> {
        let result = (|| {
            self.check(store, current, control)?;
            if self.phase == Phase::Done && self.pending_offset == self.pending.len() {
                return Ok(None);
            }
            let tx = store.db.unchecked_transaction()?;
            let mut output = Vec::with_capacity(PULL_BYTES);
            for _ in 0..PULL_STEPS {
                control.check()?;
                if self.pending_offset < self.pending.len() {
                    let n =
                        (self.pending.len() - self.pending_offset).min(PULL_BYTES - output.len());
                    output.extend_from_slice(
                        &self.pending[self.pending_offset..self.pending_offset + n],
                    );
                    self.pending_offset += n;
                    if output.len() == PULL_BYTES {
                        break;
                    }
                    self.pending.clear();
                    self.pending_offset = 0;
                }
                match self.phase {
                    Phase::TableStart => {
                        if self.table == TABLES.len() {
                            self.phase = Phase::Footer;
                            continue;
                        }
                        self.pending
                            .extend_from_slice(&(self.table as u16).to_be_bytes());
                        self.phase = Phase::Rows;
                    }
                    Phase::Rows => {
                        let (table, width) = TABLES[self.table];
                        if table == "source_assets"
                            && !self.plan.options.include_authorised_source_assets
                        {
                            self.phase = Phase::TableEnd;
                            continue;
                        }
                        let asset_filter = if table == "source_assets" {
                            " AND json_extract(class,'$') <> 'LiveCache'"
                        } else {
                            ""
                        };
                        let mut query=tx.prepare(&format!("SELECT rowid,* FROM {table} WHERE (?1 IS NULL OR rowid>?1) AND (?2 IS NULL OR rowid=?2){asset_filter} ORDER BY rowid LIMIT 1"))?;
                        if query.column_count() != width + 1 {
                            return Err(Error::Unsupported);
                        }
                        let mut rows =
                            query.query(rusqlite::params![self.last_row, self.row.map(|r| r.0)])?;
                        let Some(row) = rows.next()? else {
                            if self.row.is_some() {
                                return Err(Error::Conflict);
                            }
                            self.phase = Phase::TableEnd;
                            continue;
                        };
                        let row_id = row.get::<_, i64>(0)?;
                        let offset = self.row.map_or(0, |r| r.1);
                        if self.row.is_none() {
                            if table == "source_assets"
                                && !include_asset(
                                    row.get_ref(3)?.as_str().map_err(|_| Error::Integrity)?,
                                    row.get::<_, Option<String>>(4)?.as_deref(),
                                    self.plan.options,
                                )?
                            {
                                self.last_row = Some(row_id);
                                continue;
                            }
                            self.count = self
                                .count
                                .checked_add(1)
                                .filter(|n| *n <= ROW_LIMIT)
                                .ok_or(Error::ResourceLimit)?;
                            if table == "engine_versions" {
                                let mut version: EngineVersion = serde_json::from_str(
                                    row.get_ref(3)?.as_str().map_err(|_| Error::Integrity)?,
                                )?;
                                version.local_resource = None;
                                let payload = serde_json::to_vec(&version)?;
                                if payload.len() > CELL_LIMIT {
                                    return Err(Error::ResourceLimit);
                                }
                                self.engine_payload = Some(payload);
                            }
                        }
                        let before = output.len();
                        let mut slice = SliceWriter {
                            output: &mut output,
                            skip: offset,
                            position: 0,
                        };
                        slice.write_all(&[1])?;
                        for column in 0..width {
                            control.check()?;
                            match row.get_ref(column + 1)? {
                                ValueRef::Null => slice.write_all(&[0])?,
                                ValueRef::Integer(n) => {
                                    slice.write_all(&[1])?;
                                    slice.write_all(&n.to_be_bytes())?;
                                }
                                ValueRef::Text(bytes) => {
                                    if offset == 0 {
                                        std::str::from_utf8(bytes).map_err(|_| Error::Integrity)?;
                                    }
                                    slice.write_all(&[2])?;
                                    if table == "engine_versions" && column == 2 {
                                        write_bytes(
                                            &mut slice,
                                            self.engine_payload
                                                .as_deref()
                                                .ok_or(Error::Integrity)?,
                                        )?;
                                    } else {
                                        write_bytes(&mut slice, bytes)?;
                                    }
                                }
                                ValueRef::Blob(bytes) => {
                                    slice.write_all(&[3])?;
                                    write_bytes(&mut slice, bytes)?;
                                }
                                ValueRef::Real(_) => return Err(Error::Unsupported),
                            }
                        }
                        let total = slice.position;
                        let next = offset
                            .checked_add(output.len() - before)
                            .ok_or(Error::ResourceLimit)?;
                        if next == total {
                            self.last_row = Some(row_id);
                            self.row = None;
                            self.engine_payload = None;
                        } else if next < total && next > offset {
                            self.row = Some((row_id, next));
                        } else {
                            return Err(Error::Integrity);
                        }
                    }
                    Phase::TableEnd => {
                        self.pending.push(0);
                        self.table += 1;
                        self.last_row = None;
                        self.phase = Phase::TableStart;
                    }
                    Phase::Footer => {
                        self.pending.extend_from_slice(END);
                        self.pending.extend_from_slice(&self.count.to_be_bytes());
                        self.phase = Phase::Done;
                    }
                    Phase::Done => break,
                }
                if output.len() == PULL_BYTES {
                    break;
                }
            }
            tx.commit()?;
            // Authoring may run between pulls, and even external WAL mutation
            // during a pull is checked after the read snapshot has ended.
            self.check(store, current, control)?;
            Ok(Some(output))
        })();
        if result.is_err() {
            self.poison();
        }
        result
    }
}
struct SliceWriter<'a> {
    output: &'a mut Vec<u8>,
    skip: usize,
    position: usize,
}
impl Write for SliceWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let begin = self.skip.saturating_sub(self.position).min(bytes.len());
        let count = (bytes.len() - begin).min(PULL_BYTES - self.output.len());
        self.output.extend_from_slice(&bytes[begin..begin + count]);
        self.position = self
            .position
            .checked_add(bytes.len())
            .ok_or_else(|| std::io::Error::other("backup row limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
