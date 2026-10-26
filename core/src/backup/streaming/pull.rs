//! Bounded, resumable pulls through the authoritative connection. No read
//! transaction, SQL statement, encryptor, password or caller sink survives a pull.
use super::*;
use std::sync::Arc;
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
    delegated: bool,
    table: usize,
    last_row: Option<i64>,
    row: Option<(i64, usize)>,
    count: u64,
    phase: Phase,
    pending: Vec<u8>,
    pending_offset: usize,
    failed: bool,
    engine_payload: Option<Vec<u8>>,
    verified_current: Option<Arc<()>>,
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
        let permit = plan.authenticate(auth)?;
        matches()?;
        self.open_stream_backup_cursor_authorized(plan, permit, control)
    }
    pub(crate) fn open_stream_backup_cursor_authorized(
        &self,
        plan: StreamBackupPlan,
        permit: crate::backup::BackupReadPermit,
        control: &JobControl,
    ) -> Result<StreamBackupCursor> {
        control.check()?;
        if permit.library != plan.library
            || permit.generation != plan.generation
            || permit.options
                != (
                    plan.options.include_original_ink,
                    plan.options.include_authorised_source_assets,
                )
            || Self::setting(&self.db, "library_id")? != plan.library
            || Self::setting(&self.db, "generation")? != plan.generation
        {
            return Err(Error::Conflict);
        }
        if !plan.scopes.is_empty() && permit.proof.remaining().is_none_or(|d| d.is_zero()) {
            return Err(Error::Denied);
        }
        let mut pending = BODY.to_vec();
        pending.extend_from_slice(&19u32.to_be_bytes());
        pending.extend_from_slice(&(plan.omissions.len() as u32).to_be_bytes());
        for omission in &plan.omissions {
            write_bytes(&mut pending, omission.as_bytes())?;
        }
        Ok(StreamBackupCursor {
            plan,
            proof: permit.proof,
            delegated: permit.delegated,
            table: 0,
            last_row: None,
            row: None,
            count: 0,
            phase: Phase::TableStart,
            pending,
            pending_offset: 0,
            failed: false,
            engine_payload: None,
            verified_current: None,
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
        self.verified_current = None;
    }
    fn check(
        &mut self,
        store: &Store,
        current: Option<&Access>,
        control: &JobControl,
    ) -> Result<()> {
        control.check()?;
        if self.failed {
            return Err(Error::Cancelled);
        }
        if !self.plan.scopes.is_empty() {
            // The private proof was constructed from exactly plan.scopes and is
            // never mutated except on permanent poison. Only its clock changes.
            if self.proof.remaining().is_none_or(|d| d.is_zero()) {
                return Err(Error::Denied);
            }
            if let Some(current) = current {
                if current.remaining().is_none_or(|d| d.is_zero()) {
                    return Err(Error::Denied);
                }
                if self
                    .verified_current
                    .as_ref()
                    .is_none_or(|epoch| !Arc::ptr_eq(epoch, current.grant_epoch()))
                {
                    for (index, scope) in self.plan.scopes.iter().enumerate() {
                        if index % 64 == 0 {
                            control.check()?;
                        }
                        if !current.permits(scope) {
                            return Err(Error::Denied);
                        }
                    }
                    self.verified_current = Some(Arc::clone(current.grant_epoch()));
                }
            }
        }
        if let Some(current) = current {
            Store::require_app(&store.db, current)?;
        }
        control.check()?;
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
        self.read_chunk(
            store,
            if self.delegated { None } else { Some(current) },
            control,
        )
    }
    pub fn validate_complete(
        &mut self,
        store: &Store,
        current: &Access,
        control: &JobControl,
    ) -> Result<()> {
        let result = self.check_complete(
            store,
            if self.delegated { None } else { Some(current) },
            control,
        );
        if result.is_err() {
            self.poison();
        }
        result
    }
    pub(super) fn check_complete(
        &mut self,
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
                        let (sql, parameter) =
                            row_query(table, self.row.map(|r| r.0), self.last_row);
                        let mut query = tx.prepare(&sql)?;
                        if query.column_count() != width + 1 {
                            return Err(Error::Unsupported);
                        }
                        let mut rows = if let Some(value) = parameter {
                            query.query([value])?
                        } else {
                            query.query([])?
                        };
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

// Exact/next-row seeks must stay separate from the first-row query: nullable OR
// predicates prevent SQLite's rowid seek. No class predicate may scan a run of
// omitted cache rows inside one step. CASE avoids loading their blob column.
fn row_query(table: &str, current: Option<i64>, after: Option<i64>) -> (String, Option<i64>) {
    let columns = if table == "source_assets" {
        "rowid,id,source,class,authority,CASE WHEN json_extract(class,'$')='LiveCache' THEN NULL ELSE bytes END,CASE WHEN json_extract(class,'$')='LiveCache' THEN NULL ELSE digest END"
    } else {
        "rowid,*"
    };
    let (predicate, parameter) = if let Some(row) = current {
        (" WHERE rowid=?1", Some(row))
    } else if let Some(row) = after {
        (" WHERE rowid>?1", Some(row))
    } else {
        ("", None)
    };
    (
        format!("SELECT {columns} FROM {table}{predicate} ORDER BY rowid LIMIT 1"),
        parameter,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{Connection, StatementStatus};
    #[test]
    fn exact_and_next_row_queries_seek_the_rowid_index_instead_of_scanning_a_prefix() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE source_assets(id TEXT PRIMARY KEY,source TEXT,class TEXT,authority TEXT,bytes BLOB,digest TEXT);
        WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000)
        INSERT INTO source_assets SELECT CAST(x AS TEXT),'source','\"LiveCache\"',NULL,zeroblob(8192),'digest' FROM n;
        INSERT INTO source_assets VALUES('retained','source','\"HistoricalSnapshot\"','authority',X'010203','retained-digest');
        INSERT INTO settings(rowid,key,value) VALUES(-7,'first','first'),(1,'next','next');").unwrap();
        for table in ["settings", "source_assets"] {
            for (current, after) in [(Some(1), None), (None, Some(0))] {
                let (sql, parameter) = row_query(table, current, after);
                let plan = db
                    .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                    .unwrap()
                    .query_map([parameter.unwrap()], |r| r.get::<_, String>(3))
                    .unwrap()
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .unwrap()
                    .join(" ");
                assert!(plan.contains("SEARCH") && plan.contains("rowid"), "{plan}");
            }
        }
        let (sql, _) = row_query("settings", None, None);
        assert_eq!(db.query_row(&sql, [], |r| r.get::<_, i64>(0)).unwrap(), -7); // no assumed positive first row
        let (sql, param) = row_query("settings", None, Some(-7));
        assert_eq!(
            db.query_row(&sql, [param.unwrap()], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        let (sql, param) = row_query("source_assets", None, Some(0));
        let mut next = db.prepare(&sql).unwrap();
        {
            let mut rows = next.query([param.unwrap()]).unwrap();
            let row = rows.next().unwrap().unwrap();
            assert_eq!(row.get::<_, i64>(0).unwrap(), 1);
            assert!(matches!(row.get_ref(5).unwrap(), ValueRef::Null));
            assert!(matches!(row.get_ref(6).unwrap(), ValueRef::Null));
        }
        let next_steps = next.get_status(StatementStatus::VmStep);
        let mut old=db.prepare("SELECT rowid,* FROM source_assets WHERE (?1 IS NULL OR rowid>?1) AND (?2 IS NULL OR rowid=?2) AND json_extract(class,'$') <> 'LiveCache' ORDER BY rowid LIMIT 1").unwrap();
        {
            let mut rows = old.query(rusqlite::params![0, None::<i64>]).unwrap();
            assert_eq!(
                rows.next().unwrap().unwrap().get::<_, i64>(0).unwrap(),
                1001
            );
        }
        // Relative VM work in this fixture, not a wall-time/device SLO.
        assert!(next_steps < old.get_status(StatementStatus::VmStep));
        let (sql, param) = row_query("source_assets", Some(1001), Some(1000));
        assert_eq!(
            db.query_row(&sql, [param.unwrap()], |r| r.get::<_, Vec<u8>>(5))
                .unwrap(),
            vec![1, 2, 3]
        );
    }
}
