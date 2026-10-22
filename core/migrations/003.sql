-- Exercise membership is independent of headings and view visibility.
CREATE TABLE exercises(id TEXT PRIMARY KEY, master TEXT NOT NULL REFERENCES entities(id), heading TEXT REFERENCES blocks(id), payload TEXT NOT NULL CHECK(json_valid(payload))) STRICT;
CREATE INDEX exercises_master ON exercises(master);
CREATE UNIQUE INDEX exercises_heading ON exercises(heading) WHERE heading IS NOT NULL;
CREATE TABLE exercise_members(block TEXT PRIMARY KEY REFERENCES blocks(id), exercise TEXT NOT NULL REFERENCES exercises(id)) STRICT;
CREATE INDEX exercise_members_owner ON exercise_members(exercise);
CREATE TABLE membership_versions(block TEXT PRIMARY KEY REFERENCES blocks(id), revision INTEGER NOT NULL) STRICT;
CREATE TABLE confirmed_headings(block TEXT PRIMARY KEY REFERENCES blocks(id)) STRICT;
CREATE TABLE exercise_rules(master TEXT PRIMARY KEY REFERENCES entities(id), keyword TEXT NOT NULL) STRICT;
CREATE TABLE membership_operations(id TEXT PRIMARY KEY, master TEXT NOT NULL REFERENCES entities(id), payload TEXT NOT NULL CHECK(json_valid(payload))) STRICT;
PRAGMA user_version=3;
