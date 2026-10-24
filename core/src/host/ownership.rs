//! SQLite owns both the exclusive connection lease and all of its descriptors.
//! Opening/closing a separate raw database FD can disturb POSIX SQLite locks.
use crate::{store::Store, Error, Result};
use rusqlite::{Connection, ErrorCode};
use std::{path::Path, time::Duration};

pub(super) fn open(path: &Path) -> Result<Store> {
    let db = Connection::open(path)?;
    // Claim before migrations or authoring. Exclusive mode retains SQLite's own
    // database lock across commits; WAL also supports this single-connection mode.
    db.busy_timeout(Duration::ZERO)?;
    db.execute_batch(
        "PRAGMA trusted_schema=OFF; PRAGMA locking_mode=EXCLUSIVE; BEGIN EXCLUSIVE; COMMIT;",
    )
    .map_err(|error| match error {
        rusqlite::Error::SqliteFailure(code, _)
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            Error::Unavailable
        }
        other => Error::Storage(other),
    })?;
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
