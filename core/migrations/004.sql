CREATE TABLE image_assets(id TEXT PRIMARY KEY, entity TEXT NOT NULL REFERENCES entities(id), mime TEXT NOT NULL, bytes BLOB NOT NULL, digest TEXT NOT NULL) STRICT;
CREATE TABLE layouts(block TEXT PRIMARY KEY REFERENCES blocks(id), payload TEXT NOT NULL CHECK(json_valid(payload))) STRICT;
CREATE TABLE object_groups(id TEXT PRIMARY KEY, entity TEXT NOT NULL REFERENCES entities(id), payload TEXT NOT NULL CHECK(json_valid(payload))) STRICT;
CREATE TABLE embeds(id TEXT PRIMARY KEY, host TEXT NOT NULL REFERENCES entities(id), source TEXT NOT NULL REFERENCES entities(id), payload TEXT NOT NULL CHECK(json_valid(payload))) STRICT;
CREATE INDEX embeds_source ON embeds(source);
CREATE TABLE view_settings(id TEXT PRIMARY KEY, entity TEXT NOT NULL REFERENCES entities(id), payload TEXT NOT NULL CHECK(json_valid(payload))) STRICT;
PRAGMA user_version=4;
