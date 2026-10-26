-- Explicit reviewed reclamation history; no model bytes or serializable file authority.
CREATE TABLE engine_reclamations(
 id TEXT PRIMARY KEY,
 version TEXT NOT NULL,
 payload TEXT NOT NULL CHECK(json_valid(payload))
) STRICT;
CREATE INDEX engine_reclamations_version ON engine_reclamations(version);
PRAGMA user_version=18;
