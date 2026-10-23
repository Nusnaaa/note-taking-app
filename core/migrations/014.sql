-- Atomic source-layer view state. Unassigned blocks belong to the implicit base layer.
CREATE TABLE canvas_layers(canvas TEXT PRIMARY KEY REFERENCES entities(id),payload TEXT NOT NULL CHECK(json_valid(payload))) STRICT;
CREATE TRIGGER journal_canvas_layers_insert AFTER INSERT ON canvas_layers
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'canvas_layers',NEW.canvas,NULL,json_array(NEW.canvas,NEW.payload))
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
CREATE TRIGGER journal_canvas_layers_update AFTER UPDATE ON canvas_layers
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'canvas_layers',NEW.canvas,json_array(OLD.canvas,OLD.payload),json_array(NEW.canvas,NEW.payload))
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
CREATE TRIGGER journal_canvas_layers_delete AFTER DELETE ON canvas_layers
WHEN (SELECT value FROM settings WHERE key='edit_operation') <> '' BEGIN
INSERT INTO edit_changes VALUES(CAST((SELECT value FROM settings WHERE key='edit_operation') AS INTEGER),'canvas_layers',OLD.canvas,json_array(OLD.canvas,OLD.payload),NULL)
ON CONFLICT(operation,table_name,item) DO UPDATE SET after_row=excluded.after_row;
END;
CREATE TRIGGER workspace_layer_identity BEFORE INSERT ON workspace_panes
WHEN EXISTS(SELECT 1 FROM canvas_layers c,json_each(c.payload,'$.layers') l WHERE json_extract(l.value,'$.id')=NEW.slot)
BEGIN SELECT RAISE(ABORT,'workspace layer identity'); END;
CREATE TRIGGER stroke_layer_identity BEFORE INSERT ON strokes
WHEN EXISTS(SELECT 1 FROM canvas_layers c,json_each(c.payload,'$.layers') l WHERE json_extract(l.value,'$.id')=NEW.id)
BEGIN SELECT RAISE(ABORT,'stroke layer identity'); END;
CREATE TRIGGER view_layer_identity BEFORE INSERT ON view_settings
WHEN EXISTS(SELECT 1 FROM canvas_layers c,json_each(c.payload,'$.layers') l WHERE json_extract(l.value,'$.id')=NEW.id)
BEGIN SELECT RAISE(ABORT,'view layer identity'); END;
PRAGMA user_version=14;
