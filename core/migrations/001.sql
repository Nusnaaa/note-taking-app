CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
INSERT INTO settings VALUES('generation','0'),('app_locked','false'),('global_mode','"Manual"'),('active_ms','0'),('history_hours','72');
CREATE TABLE entities(id TEXT PRIMARY KEY, parent TEXT REFERENCES entities(id), payload TEXT NOT NULL CHECK(json_valid(payload))) STRICT;
CREATE INDEX entities_parent ON entities(parent);
CREATE TABLE links(id TEXT PRIMARY KEY, origin TEXT NOT NULL REFERENCES entities(id), target TEXT NOT NULL REFERENCES entities(id), anchor TEXT, checkpoint TEXT NOT NULL) STRICT;
CREATE INDEX links_target ON links(target);
CREATE TABLE modes(entity TEXT PRIMARY KEY REFERENCES entities(id), mode TEXT NOT NULL) STRICT;
PRAGMA user_version=1;
