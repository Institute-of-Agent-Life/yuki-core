-- Reader 21. Stable attachment positions survive expiration; original bytes and
-- immutable outbound versions are separate owners of a content-addressed object.
CREATE TABLE media_objects(digest TEXT PRIMARY KEY,size INTEGER NOT NULL,bytes BLOB NOT NULL,CHECK(size=length(bytes)));
CREATE TABLE media_sources(event INTEGER NOT NULL REFERENCES events(id),slot INTEGER NOT NULL,segment INTEGER NOT NULL,stage TEXT NOT NULL REFERENCES assignments(id),
 kind TEXT NOT NULL,origin TEXT,state TEXT NOT NULL DEFAULT 'unprepared' CHECK(state IN('unprepared','cached','expired','erased')),
 digest TEXT REFERENCES media_objects(digest),mime TEXT,expires INTEGER,acquired_via TEXT,PRIMARY KEY(event,slot));
CREATE TABLE media_handles(event INTEGER NOT NULL REFERENCES events(id),slot INTEGER NOT NULL,source INTEGER NOT NULL,source_slot INTEGER NOT NULL,
 PRIMARY KEY(event,slot),FOREIGN KEY(source,source_slot) REFERENCES media_sources(event,slot));
CREATE TABLE media_versions(id TEXT PRIMARY KEY,execution TEXT NOT NULL REFERENCES executions(id),operation TEXT NOT NULL,
 source INTEGER REFERENCES events(id),source_slot INTEGER,digest TEXT REFERENCES media_objects(digest),size INTEGER NOT NULL,
 filename TEXT NOT NULL,mime TEXT NOT NULL,kind TEXT NOT NULL,basis TEXT NOT NULL,UNIQUE(execution,operation));
CREATE INDEX media_source_expiry ON media_sources(expires) WHERE state='cached';
CREATE TABLE media_reads(execution TEXT NOT NULL,operation TEXT NOT NULL,source INTEGER NOT NULL,slot INTEGER NOT NULL,
 digest TEXT REFERENCES media_objects(digest),mime TEXT NOT NULL,PRIMARY KEY(execution,operation),FOREIGN KEY(execution,operation) REFERENCES operations(namespace,operation_id));
CREATE VIEW media_positions AS
 SELECT e.id AS event,CAST(s.key AS INTEGER) AS segment,e.bot_stage AS stage,
 json_extract(s.value,'$.kind') AS kind,json_extract(s.value,'$.reference') AS origin,
 sum(CASE WHEN json_extract(s.value,'$.type') IN ('text','mention','face','reply')
 OR (json_extract(s.value,'$.type')='opaque' AND json_extract(s.value,'$.kind') IN ('yuki_context','agent_target')) THEN 0 ELSE 1 END)
 OVER(PARTITION BY e.id ORDER BY CAST(s.key AS INTEGER))-1 AS slot,json_extract(s.value,'$.type') AS tag
 FROM events e,json_each(e.envelope,'$.segments') s WHERE e.entry_id IS NOT NULL AND json_extract(e.envelope,'$.kind')='message';
INSERT INTO media_sources(event,slot,segment,stage,kind,origin)
 SELECT event,slot,segment,stage,kind,origin FROM media_positions WHERE tag='media' AND kind IN('image','video','file','record','audio');
INSERT INTO media_handles SELECT event,slot,event,slot FROM media_sources;
CREATE TRIGGER index_media_slots AFTER INSERT ON events WHEN NEW.entry_id IS NOT NULL BEGIN
 INSERT INTO media_sources(event,slot,segment,stage,kind,origin) SELECT event,slot,segment,stage,kind,origin FROM media_positions WHERE event=NEW.id AND tag='media' AND kind IN('image','video','file','record','audio');
 INSERT INTO media_handles SELECT event,slot,event,slot FROM media_sources WHERE event=NEW.id;
END;
CREATE TABLE media_transfer_files(relative TEXT PRIMARY KEY,digest TEXT NOT NULL,version TEXT NOT NULL REFERENCES media_versions(id),state TEXT NOT NULL CHECK(state IN('pending','ready','stale')));
CREATE UNIQUE INDEX media_ready_transfer ON media_transfer_files(version) WHERE state='ready';
CREATE TRIGGER erase_media_source AFTER INSERT ON invalidated_sources BEGIN
 UPDATE media_sources SET origin=NULL,digest=NULL,mime=NULL,state='erased' WHERE event=NEW.source;
 DELETE FROM media_handles WHERE event=NEW.source OR source=NEW.source;
 UPDATE media_versions SET digest=NULL,filename='erased',mime='application/octet-stream'
 WHERE source=NEW.source OR execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
 UPDATE media_reads SET digest=NULL WHERE source=NEW.source OR execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
END;
-- A reply uses a locator actually observed by the same receiving stage in
-- this entry. Its presentation handle retains the original source and lifetime.
CREATE VIEW media_reply_slots AS
 SELECT e.id AS event,coalesce((SELECT max(slot)+1 FROM media_positions WHERE event=e.id),0)+s.slot AS slot,s.event AS source,s.slot AS source_slot
 FROM events e JOIN json_each(e.envelope,'$.segments') part ON json_extract(part.value,'$.type')='reply'
 JOIN event_locators l ON l.receiver=e.bot_stage AND l.native_id=json_extract(part.value,'$.native_id')
 JOIN events old ON old.id=l.event_id AND old.id<e.id AND old.entry_id=e.entry_id JOIN media_sources s ON s.event=old.id AND s.state!='erased'
 WHERE NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source IN(e.id,old.id));
INSERT OR IGNORE INTO media_handles SELECT * FROM media_reply_slots;
CREATE TRIGGER index_observed_reply_media AFTER INSERT ON events WHEN NEW.entry_id IS NOT NULL BEGIN
 INSERT OR IGNORE INTO media_handles SELECT * FROM media_reply_slots WHERE event=NEW.id;
END;
CREATE TABLE media_version_reads(execution TEXT NOT NULL,operation TEXT NOT NULL,version TEXT NOT NULL REFERENCES media_versions(id),PRIMARY KEY(execution,operation),FOREIGN KEY(execution,operation) REFERENCES operations(namespace,operation_id));
