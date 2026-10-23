use std::process::Command;
use stem_core::{
    security::Access,
    store::{Order, Store},
    Error, Id,
};
#[test]
fn process_exit_before_and_after_commit_preserves_correct_durable_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crash.db");
    let exe = env!("CARGO_BIN_EXE_recovery_probe");
    let seed = Command::new(exe).arg(&path).arg("seed").output().unwrap();
    assert!(seed.status.success());
    let id = Id::parse(String::from_utf8(seed.stdout).unwrap().trim()).unwrap();
    for (mode, title) in [
        ("crash-before-commit", "durable before crash"),
        ("commit-and-exit", "durable after commit"),
    ] {
        let status = Command::new(exe)
            .arg(&path)
            .arg(mode)
            .arg(id.to_string())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(99));
        let s = Store::open(&path).unwrap();
        assert_eq!(s.entity(id, &Access::public()).unwrap().title, title);
        s.check_integrity().unwrap();
    }
}
#[test]
fn v1_to_current_migration_preserves_entities_and_is_repeatable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../migrations/001.sql"))
        .unwrap();
    let id = Id::new();
    let json=serde_json::json!({"id":id,"kind":"Note","parent":null,"title":"old note","created_ms":1,"modified_ms":1,"revision":0,"locked":false,"trashed":false,"purged":false}).to_string();
    db.execute(
        "INSERT INTO entities VALUES(?1,NULL,?2)",
        rusqlite::params![id.to_string(), json],
    )
    .unwrap();
    drop(db);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 11);
    assert_eq!(s.entity(id, &Access::public()).unwrap().created_ms, 1);
    drop(s);
    let s = Store::open(path).unwrap();
    s.check_integrity().unwrap();
    assert_eq!(
        s.list(None, Order::Name, None, None, &Access::public())
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn failed_migration_rolls_back_and_newer_schema_is_not_downgraded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../migrations/001.sql"))
        .unwrap();
    db.execute_batch("CREATE TABLE history(x); ").unwrap();
    drop(db);
    assert!(Store::open(&path).is_err());
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='blocks'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    db.pragma_update(None, "user_version", 999).unwrap();
    drop(db);
    assert!(matches!(Store::open(path), Err(Error::Unsupported)));
}
#[test]
fn two_connections_cannot_silently_overwrite_same_revision() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("concurrent.db");
    let a = Access::public();
    let mut first = Store::open(&path).unwrap();
    let e = first
        .create(stem_core::store::Kind::Note, None, "start", &a)
        .unwrap();
    let mut second = Store::open(path).unwrap();
    first.rename(e.id, 0, "first", &a).unwrap();
    assert!(matches!(
        second.rename(e.id, 0, "second", &a),
        Err(Error::Conflict)
    ));
    assert_eq!(second.entity(e.id, &a).unwrap().title, "first");
}
