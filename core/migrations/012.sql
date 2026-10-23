-- Library-local activation state; source contents/grants never live in this table.
CREATE TABLE workspace_panes(
 slot TEXT PRIMARY KEY,
 ordinal INTEGER NOT NULL UNIQUE CHECK(ordinal BETWEEN 0 AND 7),
 kind TEXT NOT NULL CHECK(kind IN('View','Pdf','Unavailable')),
 view TEXT REFERENCES view_settings(id) ON DELETE SET NULL,
 source TEXT REFERENCES sources(id) ON DELETE SET NULL,
 weight INTEGER NOT NULL CHECK(weight BETWEEN 1 AND 1000000),
 tool TEXT NOT NULL CHECK(tool IN('Pen','Eraser','Selection','Text','Shape')),
 CHECK((kind='View' AND source IS NULL) OR (kind='Pdf' AND view IS NULL) OR (kind='Unavailable' AND view IS NULL AND source IS NULL))
) STRICT;
CREATE TABLE workspace_state(
 id INTEGER PRIMARY KEY CHECK(id=1),
 revision INTEGER NOT NULL CHECK(revision>=0),
 active TEXT REFERENCES workspace_panes(slot) ON DELETE SET NULL,
 axis TEXT NOT NULL CHECK(axis IN('Horizontal','Vertical'))
) STRICT;
INSERT INTO workspace_state VALUES(1,0,NULL,'Horizontal');
-- Opaque public slot IDs must never echo private content identifiers.
CREATE TRIGGER workspace_slot_identity BEFORE INSERT ON workspace_panes
WHEN EXISTS(SELECT 1 FROM entities WHERE id=NEW.slot
 UNION ALL SELECT 1 FROM blocks WHERE id=NEW.slot
 UNION ALL SELECT 1 FROM strokes WHERE id=NEW.slot
 UNION ALL SELECT 1 FROM exercises WHERE id=NEW.slot
 UNION ALL SELECT 1 FROM view_settings WHERE id=NEW.slot
 UNION ALL SELECT 1 FROM sources WHERE id=NEW.slot
 UNION ALL SELECT 1 FROM image_assets WHERE id=NEW.slot
 UNION ALL SELECT 1 FROM object_groups WHERE id=NEW.slot
 UNION ALL SELECT 1 FROM embeds WHERE id=NEW.slot
 UNION ALL SELECT 1 FROM links WHERE id=NEW.slot
 UNION ALL SELECT 1 FROM pdf_anchors WHERE id=NEW.slot
 UNION ALL SELECT 1 FROM engine_versions WHERE id=NEW.slot)
BEGIN SELECT RAISE(ABORT,'workspace slot identity'); END;
CREATE TRIGGER view_workspace_identity BEFORE INSERT ON view_settings
WHEN EXISTS(SELECT 1 FROM workspace_panes WHERE slot=NEW.id)
BEGIN SELECT RAISE(ABORT,'view workspace identity'); END;
CREATE TRIGGER stroke_workspace_identity BEFORE INSERT ON strokes
WHEN EXISTS(SELECT 1 FROM workspace_panes WHERE slot=NEW.id)
BEGIN SELECT RAISE(ABORT,'stroke workspace identity'); END;
CREATE TRIGGER retire_workspace_reference AFTER UPDATE OF view,source ON workspace_panes
WHEN (OLD.view IS NOT NULL AND NEW.view IS NULL) OR (OLD.source IS NOT NULL AND NEW.source IS NULL)
BEGIN
 UPDATE workspace_panes SET kind='Unavailable',tool='Pen' WHERE slot=NEW.slot;
 UPDATE workspace_state SET revision=revision+1 WHERE id=1;
END;
PRAGMA user_version=12;
