-- Reader 42. Public history shares the canonical event IDs already published by Client9.
-- Legacy history item IDs and anchors remain unchanged.
CREATE TRIGGER add_history_external AFTER INSERT ON events
WHEN NEW.entry_id IS NOT NULL AND json_extract(NEW.envelope,'$.kind')='external_event' BEGIN
 INSERT INTO history_items(entry,source_event,actor,kind) VALUES(NEW.entry_id,NEW.id,NULL,'external_event');
END;
INSERT INTO history_items(entry,source_event,actor,kind)
 SELECT entry_id,id,NULL,'external_event' FROM events e
 WHERE entry_id IS NOT NULL AND json_extract(envelope,'$.kind')='external_event'
 AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id) ORDER BY id;
CREATE VIEW history_public_events AS
 SELECT h.id AS item,e.id AS event,h.entry,h.source_event,h.outbound,h.kind
 FROM history_items h JOIN events e ON e.id=h.source_event
 WHERE h.visible AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)
 UNION ALL
 SELECT h.id,d.event,h.entry,NULL,h.outbound,'message'
 FROM history_items h JOIN personal_live_deliveries d ON d.id=h.outbound WHERE h.visible;
CREATE TABLE history_public_anchors(
 execution TEXT NOT NULL,operation TEXT NOT NULL,args TEXT NOT NULL,blueprint TEXT NOT NULL,
 PRIMARY KEY(execution,operation),FOREIGN KEY(execution,operation) REFERENCES operations(namespace,operation_id));
CREATE TABLE history_public_rows(
 execution TEXT NOT NULL,operation TEXT NOT NULL,event INTEGER NOT NULL REFERENCES events(id),
 item INTEGER NOT NULL REFERENCES history_items(id),body TEXT,
 PRIMARY KEY(execution,operation,event),
 FOREIGN KEY(execution,operation) REFERENCES history_public_anchors(execution,operation));
CREATE TRIGGER erase_public_history_row AFTER UPDATE OF visible ON history_items WHEN NOT NEW.visible BEGIN
 UPDATE history_public_rows SET body=NULL WHERE item=NEW.id;
END;
CREATE TRIGGER erase_public_history_event AFTER INSERT ON invalidated_sources BEGIN
 UPDATE history_public_rows SET body=NULL WHERE event=NEW.source;
 UPDATE history_public_anchors SET args='{}',blueprint='{}'
 WHERE execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
 UPDATE history_public_rows SET body=NULL
 WHERE execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
END;
CREATE TRIGGER recall_public_history_row AFTER UPDATE OF recalled,body ON social_outbound
WHEN NEW.recalled OR NEW.body='{}' BEGIN
 UPDATE history_public_rows SET body=NULL WHERE item IN(SELECT id FROM history_items WHERE outbound=NEW.id);
END;
