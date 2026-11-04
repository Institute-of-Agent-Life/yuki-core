-- Reader 11. Durable source slots keep numeric ordering after a tombstone. Every projection
-- is derivative, can be rebuilt from its original, and is discarded on source invalidation.
CREATE TABLE history_items(
 id INTEGER PRIMARY KEY AUTOINCREMENT,entry TEXT NOT NULL REFERENCES entries(id),source_event INTEGER UNIQUE REFERENCES events(id),
 outbound INTEGER UNIQUE REFERENCES social_outbound(id),actor TEXT,kind TEXT NOT NULL,revision INTEGER NOT NULL DEFAULT 1,
 indexed INTEGER NOT NULL DEFAULT 0,visible INTEGER NOT NULL DEFAULT 1,
 CHECK((source_event IS NULL)!=(outbound IS NULL))
);
CREATE INDEX history_entry_cursor ON history_items(entry,id);
CREATE INDEX history_pending ON history_items(id) WHERE indexed!=revision;
CREATE TABLE history_fields(item INTEGER NOT NULL REFERENCES history_items(id),label TEXT NOT NULL,raw TEXT NOT NULL,base TEXT NOT NULL,simplified TEXT NOT NULL,PRIMARY KEY(item,label));
CREATE VIRTUAL TABLE history_fts USING fts5(terms,tokenize='ascii');
CREATE TABLE history_transcripts(source INTEGER NOT NULL REFERENCES events(id),segment INTEGER NOT NULL CHECK(segment>=0),text TEXT NOT NULL CHECK(length(text)>0),PRIMARY KEY(source,segment));
CREATE TRIGGER add_history_input AFTER INSERT ON events WHEN NEW.entry_id IS NOT NULL AND json_extract(NEW.envelope,'$.kind')='message' BEGIN
 INSERT INTO history_items(entry,source_event,actor,kind) VALUES(NEW.entry_id,NEW.id,NEW.raw_actor,'message');
END;
CREATE TRIGGER add_history_output AFTER INSERT ON social_outbound BEGIN
 INSERT INTO history_items(entry,outbound,actor,kind,visible) VALUES(NEW.entry,NEW.id,NULL,'outbound',NEW.body!='{}');
END;
INSERT INTO history_items(entry,source_event,actor,kind) SELECT e.entry_id,e.id,e.raw_actor,'message' FROM events e
 WHERE e.entry_id IS NOT NULL AND json_extract(e.envelope,'$.kind')='message' AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id) ORDER BY e.id;
INSERT INTO history_items(entry,outbound,actor,kind,visible) SELECT entry,id,NULL,'outbound',body!='{}' FROM social_outbound ORDER BY id;

CREATE TRIGGER history_erase_event AFTER INSERT ON invalidated_sources BEGIN
 DELETE FROM history_transcripts WHERE source=NEW.source;
 UPDATE history_items SET visible=0,revision=revision+1 WHERE source_event=NEW.source;
END;
CREATE TRIGGER history_erase_outbound AFTER UPDATE OF body ON social_outbound WHEN NEW.body='{}' BEGIN
 UPDATE history_items SET visible=0,revision=revision+1 WHERE outbound=NEW.id;
END;
CREATE TRIGGER drop_history_projection AFTER UPDATE OF visible ON history_items WHEN NEW.visible=0 BEGIN
 DELETE FROM history_fields WHERE item=NEW.id;
 DELETE FROM history_fts WHERE rowid=NEW.id;
 UPDATE history_items SET indexed=NEW.revision WHERE id=NEW.id;
END;
CREATE TABLE history_anchors(execution TEXT NOT NULL REFERENCES executions(id),operation TEXT NOT NULL,query TEXT NOT NULL,items TEXT NOT NULL,cursor INTEGER,PRIMARY KEY(execution,operation));
CREATE TABLE history_anchor_rows(execution TEXT NOT NULL,operation TEXT NOT NULL,item INTEGER NOT NULL REFERENCES history_items(id),
 projection TEXT,PRIMARY KEY(execution,operation,item),FOREIGN KEY(execution,operation) REFERENCES history_anchors(execution,operation));
CREATE INDEX history_anchor_sources ON history_anchor_rows(item);
CREATE TRIGGER erase_history_snapshots AFTER UPDATE OF visible ON history_items WHEN NOT NEW.visible BEGIN
 UPDATE history_anchor_rows SET projection=NULL WHERE item=NEW.id;
END;
ALTER TABLE social_outbound ADD COLUMN observed_at INTEGER;
