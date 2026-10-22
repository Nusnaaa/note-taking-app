CREATE TABLE engine_versions(id TEXT PRIMARY KEY, engine TEXT NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload))) STRICT;
CREATE TABLE engine_active(engine TEXT PRIMARY KEY, version TEXT NOT NULL REFERENCES engine_versions(id)) STRICT;
CREATE TABLE engine_defaults(task TEXT PRIMARY KEY, engine TEXT NOT NULL) STRICT;
PRAGMA user_version=6;
