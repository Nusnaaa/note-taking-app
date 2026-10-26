-- Durable explicit resource decisions, independent of notebook contents and files.
CREATE TABLE engine_resource_actions(
 id TEXT PRIMARY KEY,
 version TEXT NOT NULL,
 payload TEXT NOT NULL CHECK(json_valid(payload))
) STRICT;
CREATE INDEX engine_resource_actions_version ON engine_resource_actions(version);
PRAGMA user_version=17;
