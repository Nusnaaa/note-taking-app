//! SQLite owns both the exclusive connection lease and all of its descriptors.
//! Opening/closing a separate raw database FD can disturb POSIX SQLite locks.
use crate::{store::Store, Error, Result};
use rusqlite::{Connection, ErrorCode, OpenFlags};
use std::{path::Path, time::Duration};

pub(super) fn open(path: &Path) -> Result<Store> {
    finish_open(Connection::open(path)?)
}
/// No raw database FD and no CREATE flag. Reject unknown/empty files before any
/// migration or write transaction; SQLite's own VFS retains descriptor ownership.
pub(super) fn open_existing(path: &Path) -> Result<Store> {
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    db.busy_timeout(Duration::ZERO)?;
    db.execute_batch("PRAGMA trusted_schema=OFF;")?;
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(storage_error)?;
    if !(1..=19).contains(&version) {
        return Err(Error::Unsupported);
    }
    let tables: i64 = db.query_row(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name IN ('settings','entities','links','modes')",
        [], |r| r.get(0),
    ).map_err(storage_error)?;
    if tables != 4 {
        return Err(Error::Integrity);
    }
    Store::setting(&db, "generation")?
        .parse::<u64>()
        .map_err(|_| Error::Integrity)?;
    if !matches!(
        Store::setting(&db, "app_locked")?.as_str(),
        "true" | "false"
    ) {
        return Err(Error::Integrity);
    }
    if version >= 8 {
        crate::Id::parse(&Store::setting(&db, "library_id")?)?;
    }
    finish_open(db)
}
fn storage_error(error: rusqlite::Error) -> Error {
    match error {
        rusqlite::Error::SqliteFailure(code, _)
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            Error::Unavailable
        }
        other => Error::Storage(other),
    }
}
fn finish_open(db: Connection) -> Result<Store> {
    // Claim before migrations or authoring. Exclusive mode retains SQLite's own
    // database lock across commits; WAL also supports this single-connection mode.
    db.busy_timeout(Duration::ZERO)?;
    db.execute_batch(
        "PRAGMA trusted_schema=OFF; PRAGMA locking_mode=EXCLUSIVE; BEGIN EXCLUSIVE; COMMIT;",
    )
    .map_err(storage_error)?;
    Store::from_connection(db)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{security::Access, store::Kind};
    #[test]
    fn sqlite_managed_ownership_survives_commits_and_failed_duplicate_connections() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("ownership.db");
        let mut owner = open(&path).unwrap();
        let access = Access::public();
        let note = owner
            .create(Kind::Note, None, "preserved", &access)
            .unwrap();
        assert_eq!(
            owner
                .db
                .pragma_query_value(None, "locking_mode", |r| r.get::<_, String>(0))
                .unwrap(),
            "exclusive"
        );
        assert!(matches!(open(&path), Err(Error::Unavailable)));
        let duplicate = Connection::open(&path).unwrap();
        duplicate.busy_timeout(Duration::ZERO).unwrap();
        assert!(duplicate
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .is_err());
        drop(duplicate);
        // SQLite's VFS defers unsafe descriptor closure; the owner still works
        // and remains exclusive after rejected/closed connection attempts.
        owner
            .rename(note.id, note.revision, "still preserved", &access)
            .unwrap();
        assert!(matches!(open(&path), Err(Error::Unavailable)));
        drop(owner);
        let reopened = open(&path).unwrap();
        assert_eq!(
            reopened.entity(note.id, &access).unwrap().title,
            "still preserved"
        );
    }
    #[test]
    fn strict_existing_open_never_initializes_missing_empty_or_unrelated_files() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing.db");
        assert!(open_existing(&missing).is_err());
        assert!(!missing.exists());
        let empty = root.path().join("empty.db");
        std::fs::write(&empty, []).unwrap();
        assert!(open_existing(&empty).is_err());
        assert!(std::fs::read(&empty).unwrap().is_empty());
        let foreign = root.path().join("foreign.db");
        let db = Connection::open(&foreign).unwrap();
        db.execute_batch("CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES('preserved'); PRAGMA user_version=15;").unwrap();
        drop(db);
        let before = std::fs::read(&foreign).unwrap();
        assert!(open_existing(&foreign).is_err());
        assert_eq!(std::fs::read(&foreign).unwrap(), before);
    }
    #[test]
    fn strict_existing_duplicate_open_keeps_owner_locks_and_committed_edits() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("library.db");
        let access = Access::public();
        let note = {
            let mut store = open(&path).unwrap();
            store.create(Kind::Note, None, "before", &access).unwrap()
        };
        let mut owner = open_existing(&path).unwrap();
        assert!(matches!(open_existing(&path), Err(Error::Unavailable)));
        owner
            .rename(note.id, note.revision, "after", &access)
            .unwrap();
        assert!(matches!(open(&path), Err(Error::Unavailable)));
        drop(owner);
        assert_eq!(
            open_existing(&path)
                .unwrap()
                .entity(note.id, &access)
                .unwrap()
                .title,
            "after"
        );
    }
    #[test]
    fn strict_existing_open_migrates_a_real_v1_library_but_rejects_bad_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("v1.db");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(include_str!("../../migrations/001.sql"))
            .unwrap();
        drop(db);
        let store = open_existing(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 19);
        store
            .db
            .execute(
                "UPDATE settings SET value='invalid' WHERE key='library_id'",
                [],
            )
            .unwrap();
        drop(store);
        assert!(open_existing(&path).is_err());
    }
    #[test]
    fn bounded_backup_pulls_leave_exclusive_owner_free_to_commit_authoring() {
        use crate::{
            backup::BackupOptions, content::Content, processing::JobControl,
            security::Authenticator, Id,
        };
        struct Proof;
        impl Authenticator for Proof {
            fn verify(
                &mut self,
                _: Id,
                _: &std::collections::BTreeSet<crate::security::Scope>,
            ) -> bool {
                true
            }
        }
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("pull.db");
        let mut owner = open(&path).unwrap();
        let access = Access::public();
        let note = owner
            .create(Kind::Note, None, "exclusive pull", &access)
            .unwrap();
        owner
            .append_content(note.id, Content::Text("x".repeat(200000)), &access)
            .unwrap();
        let plan = owner
            .prepare_stream_backup(
                BackupOptions {
                    include_original_ink: true,
                    include_authorised_source_assets: false,
                },
                &access,
            )
            .unwrap();
        let control = JobControl::new(Duration::from_secs(30)).unwrap();
        let mut cursor = owner
            .open_stream_backup_cursor(plan, &mut Proof, true, &control)
            .unwrap();
        assert!(cursor
            .next_chunk(&owner, &access, &control)
            .unwrap()
            .is_some());
        assert!(matches!(open(&path), Err(Error::Unavailable)));
        owner
            .append_content(
                note.id,
                Content::Text("authoring still commits".into()),
                &access,
            )
            .unwrap();
        assert!(matches!(
            cursor.next_chunk(&owner, &access, &control),
            Err(Error::Conflict)
        ));
        assert!(matches!(open(&path), Err(Error::Unavailable)));
        drop(owner);
        let reopened = open(&path).unwrap();
        assert_eq!(reopened.blocks(note.id, 0, 10, &access).unwrap().len(), 2);
    }
}
