mod common;
use sha2::{Digest, Sha256};
use stem_core::{
    content::Content,
    ink::Transform,
    security::Access,
    store::{Block, Display, Entity, Kind, Store},
    Id,
};
#[test]
fn v2_upgrade_keeps_existing_content_and_enables_exercises() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v2.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../migrations/001.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/002.sql"))
        .unwrap();
    // Seed the old schema itself; opening an empty database cannot demonstrate
    // that migration preserves existing source observations or history.
    let n = Entity {
        id: Id::new(),
        kind: Kind::Note,
        parent: None,
        title: "v2 original".into(),
        created_ms: 123,
        modified_ms: 456,
        revision: 7,
        locked: false,
        trashed: false,
        purged: false,
    };
    let stroke = common::stroke(0);
    let b = Block {
        id: Id::new(),
        entity: n.id,
        revision: 0,
        content: Content::Ink,
        source_ids: vec![stroke.id],
        visible_strokes: vec![stroke.id],
        transform: Transform::default(),
        display: Display::Original,
        visible: true,
        locked: false,
        manually_corrected: false,
        regions: vec![],
    };
    db.execute(
        "INSERT INTO entities VALUES(?1,NULL,?2)",
        rusqlite::params![n.id.to_string(), serde_json::to_string(&n).unwrap()],
    )
    .unwrap();
    let block_json = serde_json::to_string(&b).unwrap();
    db.execute(
        "INSERT INTO blocks VALUES(?1,?2,0,?3)",
        rusqlite::params![b.id.to_string(), n.id.to_string(), block_json],
    )
    .unwrap();
    db.execute(
        "INSERT INTO history VALUES(?1,0,0,?2)",
        rusqlite::params![b.id.to_string(), block_json],
    )
    .unwrap();
    let stroke_json = serde_json::to_string(&stroke).unwrap();
    db.execute(
        "INSERT INTO strokes VALUES(?1,?2,?3,?4)",
        rusqlite::params![
            stroke.id.to_string(),
            b.id.to_string(),
            stroke_json,
            format!("{:x}", Sha256::digest(stroke_json.as_bytes()))
        ],
    )
    .unwrap();
    drop(db);
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 9);
    let a = Access::public();
    assert_eq!(s.entity(n.id, &a).unwrap(), n);
    assert_eq!(s.block(b.id, &a).unwrap(), b);
    assert_eq!(s.source_ink(b.id, &a).unwrap(), vec![stroke.clone()]);
    assert_eq!(s.history(b.id, &a).unwrap()[0].block, b);
    let e = s.create_exercise(n.id, "view", &[b.id], &a).unwrap();
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.exercise(e.id, &a).unwrap(), e);
    assert_eq!(s.exercise_blocks(e.id, 0, 10, &a).unwrap(), vec![b.clone()]);
    assert_eq!(s.source_ink(b.id, &a).unwrap(), vec![stroke]);
    s.check_integrity().unwrap();
}
#[test]
fn failed_v3_migration_rolls_back_all_new_tables_and_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collision.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../migrations/001.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/002.sql"))
        .unwrap();
    db.execute_batch("CREATE TABLE exercise_rules(x);").unwrap();
    drop(db);
    assert!(Store::open(&path).is_err());
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='exercises'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
