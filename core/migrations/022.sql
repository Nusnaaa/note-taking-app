-- Paired document-local Undo operations. A retired member is a content-free
-- barrier: deleting one participant must never permit partial replay.
CREATE TABLE edit_links(
 operation INTEGER PRIMARY KEY REFERENCES edit_operations(id) ON DELETE CASCADE,
 peer INTEGER NOT NULL CHECK(peer<>operation),
 retired INTEGER NOT NULL DEFAULT 0 CHECK(retired IN(0,1))
) STRICT;
CREATE INDEX edit_links_peer ON edit_links(peer);
CREATE TRIGGER edit_link_retire BEFORE DELETE ON edit_operations BEGIN
 DELETE FROM edit_changes WHERE operation IN(SELECT operation FROM edit_links WHERE peer=OLD.id);
 UPDATE edit_links SET retired=1,peer=0 WHERE peer=OLD.id;
END;
PRAGMA user_version=22;
