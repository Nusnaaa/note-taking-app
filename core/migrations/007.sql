CREATE TABLE pdf_anchors(id TEXT PRIMARY KEY, source TEXT NOT NULL REFERENCES sources(id), block TEXT NOT NULL REFERENCES blocks(id), payload TEXT NOT NULL CHECK(json_valid(payload))) STRICT;
PRAGMA user_version=7;
