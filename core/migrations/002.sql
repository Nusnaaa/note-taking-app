CREATE TABLE blocks(id TEXT PRIMARY KEY, entity TEXT NOT NULL REFERENCES entities(id), ordinal INTEGER NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload))) STRICT;
CREATE INDEX blocks_entity ON blocks(entity,ordinal);
CREATE TABLE strokes(id TEXT PRIMARY KEY, block TEXT NOT NULL REFERENCES blocks(id), payload TEXT NOT NULL, digest TEXT NOT NULL) STRICT;
CREATE TABLE history(block TEXT NOT NULL REFERENCES blocks(id), revision INTEGER NOT NULL, active_ms INTEGER NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(block,revision)) STRICT;
CREATE TABLE proposals(id TEXT PRIMARY KEY, block TEXT NOT NULL REFERENCES blocks(id), payload TEXT NOT NULL, status TEXT NOT NULL) STRICT;
CREATE TABLE sources(id TEXT PRIMARY KEY, context TEXT NOT NULL REFERENCES entities(id), payload TEXT NOT NULL) STRICT;
CREATE TABLE source_assets(id TEXT PRIMARY KEY, source TEXT NOT NULL REFERENCES sources(id), class TEXT NOT NULL, authority TEXT, bytes BLOB NOT NULL, digest TEXT NOT NULL) STRICT;
PRAGMA user_version=2;
