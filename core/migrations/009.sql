-- Durable authoring undo journal. Source observations and security policy are never replayed.
CREATE TABLE edit_operations(id INTEGER PRIMARY KEY AUTOINCREMENT, entity TEXT NOT NULL REFERENCES entities(id), active_ms INTEGER NOT NULL, undone INTEGER NOT NULL DEFAULT 0 CHECK(undone IN(0,1))) STRICT;
CREATE TABLE edit_changes(operation INTEGER NOT NULL REFERENCES edit_operations(id) ON DELETE CASCADE, table_name TEXT NOT NULL, item TEXT NOT NULL, before_row TEXT, after_row TEXT, PRIMARY KEY(operation,table_name,item)) STRICT;
INSERT INTO settings VALUES('edit_operation','');
CREATE TRIGGER journal_blocks_insert AFTER INSERT ON blocks
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'blocks',NEW.id,NULL,json_array(NEW.id,NEW.entity,NEW.ordinal,NEW.payload))
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
CREATE TRIGGER journal_blocks_update AFTER UPDATE ON blocks
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'blocks',NEW.id,json_array(OLD.id,OLD.entity,OLD.ordinal,OLD.payload),json_array(NEW.id,NEW.entity,NEW.ordinal,NEW.payload))
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
CREATE TRIGGER journal_blocks_delete AFTER DELETE ON blocks
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'blocks',OLD.id,json_array(OLD.id,OLD.entity,OLD.ordinal,OLD.payload),NULL)
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
CREATE TRIGGER journal_layouts_insert AFTER INSERT ON layouts
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'layouts',NEW.block,NULL,json_array(NEW.block,NEW.payload))
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
CREATE TRIGGER journal_layouts_update AFTER UPDATE ON layouts
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'layouts',NEW.block,json_array(OLD.block,OLD.payload),json_array(NEW.block,NEW.payload))
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
CREATE TRIGGER journal_layouts_delete AFTER DELETE ON layouts
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'layouts',OLD.block,json_array(OLD.block,OLD.payload),NULL)
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
CREATE TRIGGER journal_object_groups_insert AFTER INSERT ON object_groups
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'object_groups',NEW.id,NULL,json_array(NEW.id,NEW.entity,NEW.payload))
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
CREATE TRIGGER journal_object_groups_update AFTER UPDATE ON object_groups
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'object_groups',NEW.id,json_array(OLD.id,OLD.entity,OLD.payload),json_array(NEW.id,NEW.entity,NEW.payload))
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
CREATE TRIGGER journal_object_groups_delete AFTER DELETE ON object_groups
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'object_groups',OLD.id,json_array(OLD.id,OLD.entity,OLD.payload),NULL)
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
CREATE TRIGGER journal_embeds_insert AFTER INSERT ON embeds
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'embeds',NEW.id,NULL,json_array(NEW.id,NEW.host,NEW.source,NEW.payload))
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
CREATE TRIGGER journal_embeds_update AFTER UPDATE ON embeds
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'embeds',NEW.id,json_array(OLD.id,OLD.host,OLD.source,OLD.payload),json_array(NEW.id,NEW.host,NEW.source,NEW.payload))
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
CREATE TRIGGER journal_embeds_delete AFTER DELETE ON embeds
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'embeds',OLD.id,json_array(OLD.id,OLD.host,OLD.source,OLD.payload),NULL)
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
PRAGMA user_version=9;
