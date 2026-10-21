//! Disposable verification executable. No production Human Interface.
use stem_core::{
    security::Access,
    store::{Kind, Store},
    Id,
};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let path = &args[1];
    let mode = &args[2];
    let a = Access::public();
    if mode == "seed" {
        let mut s = Store::open(path).unwrap();
        let e = s
            .create(Kind::Note, None, "durable before crash", &a)
            .unwrap();
        println!("{}", e.id);
        return;
    }
    let id = Id::parse(&args[3]).unwrap();
    if mode == "commit-and-exit" {
        let mut s = Store::open(path).unwrap();
        let e = s.entity(id, &a).unwrap();
        s.rename(id, e.revision, "durable after commit", &a)
            .unwrap();
        std::process::exit(99);
    }
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute_batch("PRAGMA synchronous=FULL; BEGIN IMMEDIATE;")
        .unwrap();
    db.execute(
        "UPDATE entities SET payload=json_set(payload,'$.title','uncommitted change') WHERE id=?1",
        [id.to_string()],
    )
    .unwrap();
    // exit bypasses Rust destructor rollback: recovery is performed on reopening.
    std::process::exit(99);
}
