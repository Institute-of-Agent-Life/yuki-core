-- Reader 20. Auxiliary model calls own a real batch root and their selected events.
-- Existing registered identities/foreign keys survive adding the private origin kind.
PRAGMA defer_foreign_keys=ON;
CREATE TABLE expanded_main_origins(ref TEXT PRIMARY KEY NOT NULL CHECK(length(ref)>0),
 kind TEXT NOT NULL CHECK(kind IN('plugin','self','schedule','work','child','memory_extract')),
 conversation TEXT NOT NULL REFERENCES entries(id),parent TEXT REFERENCES executions(id),enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN(0,1)));
INSERT INTO expanded_main_origins SELECT ref,kind,conversation,parent,enabled FROM main_origins;
DROP TABLE main_origins;
ALTER TABLE expanded_main_origins RENAME TO main_origins;
PRAGMA defer_foreign_keys=OFF;
CREATE TABLE extraction_policy(id INTEGER PRIMARY KEY CHECK(id=1),enabled INTEGER NOT NULL CHECK(enabled IN(0,1)));
INSERT INTO extraction_policy VALUES(1,0);
CREATE TABLE extraction_batches(id TEXT PRIMARY KEY,execution TEXT NOT NULL UNIQUE REFERENCES executions(id),owner TEXT NOT NULL,
 logical TEXT NOT NULL UNIQUE,contract TEXT NOT NULL,task TEXT NOT NULL,trigger_reason TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN('prepared','committed','held','invalid')),diagnostic TEXT);
CREATE UNIQUE INDEX extraction_owner_pending ON extraction_batches(owner) WHERE state IN('prepared','held');
CREATE TABLE extraction_events(event INTEGER PRIMARY KEY REFERENCES events(id),owner TEXT NOT NULL,entry TEXT NOT NULL REFERENCES entries(id),
 arrived INTEGER NOT NULL,characters INTEGER NOT NULL,state TEXT NOT NULL CHECK(state IN('queued','reserved','done','erased','blank')),
 batch TEXT REFERENCES extraction_batches(id));
CREATE INDEX extraction_ready ON extraction_events(owner,state,event);
CREATE TABLE extraction_sources(batch TEXT NOT NULL REFERENCES extraction_batches(id),event INTEGER NOT NULL REFERENCES events(id),
 role TEXT NOT NULL CHECK(role IN('primary','context')),version TEXT NOT NULL,subjects TEXT,PRIMARY KEY(batch,event));
CREATE TABLE extraction_results(batch TEXT NOT NULL REFERENCES extraction_batches(id),ordinal INTEGER NOT NULL,event INTEGER REFERENCES events(id),fact TEXT REFERENCES memory_facts(id),
 reason TEXT NOT NULL,PRIMARY KEY(batch,ordinal));
CREATE TABLE extraction_watermarks(owner TEXT PRIMARY KEY,event INTEGER NOT NULL REFERENCES events(id));
CREATE TABLE extract_fact_details(fact TEXT PRIMARY KEY REFERENCES memory_facts(id),category TEXT,kind TEXT,importance INTEGER,
 temporal_mode TEXT,valid_from TEXT,valid_until TEXT);
CREATE TRIGGER queue_live_extract AFTER INSERT ON events WHEN
 (SELECT enabled FROM extraction_policy WHERE id=1) AND NEW.raw_actor IS NOT NULL AND NEW.entry_id IS NOT NULL
 AND json_extract(NEW.envelope,'$.kind')='message' AND EXISTS(SELECT 1 FROM assignments s JOIN persons p ON p.id=NEW.raw_actor JOIN persons r ON r.id=coalesce(p.current_root,p.id) WHERE s.id=NEW.actor_stage AND s.kind='person' AND s.original_person=p.id AND s.status='active' AND p.enabled AND r.enabled)
BEGIN
 INSERT INTO extraction_events SELECT NEW.id,CASE WHEN e.scope='private' THEN 'person:'||coalesce(p.current_root,p.id) ELSE 'space:'||e.space END,e.id,CAST(unixepoch('subsec')*1000 AS INTEGER),
 coalesce((SELECT sum(length(json_extract(j.value,'$.text'))) FROM json_each(NEW.envelope,'$.segments') j WHERE json_extract(j.value,'$.type')='text'),0),'queued',NULL
 FROM entries e JOIN persons p ON p.id=NEW.raw_actor WHERE e.id=NEW.entry_id AND e.closed_at IS NULL;
 UPDATE extraction_events SET state='blank' WHERE event=NEW.id AND NOT EXISTS(SELECT 1 FROM json_each(NEW.envelope,'$.segments') j WHERE json_extract(j.value,'$.type')='text' AND length(trim(json_extract(j.value,'$.text'),char(9,10,11,12,13,32,160,12288)))>0);
END;
CREATE TRIGGER extract_transcript AFTER INSERT ON history_transcripts BEGIN
 UPDATE extraction_events SET characters=coalesce((SELECT sum(length(json_extract(j.value,'$.text'))) FROM events e, json_each(e.envelope,'$.segments') j WHERE e.id=NEW.source AND json_extract(j.value,'$.type')='text'),0)+coalesce((SELECT sum(length(text)) FROM history_transcripts WHERE source=NEW.source),0),state='queued' WHERE event=NEW.source AND state IN('queued','blank') AND batch IS NULL;
END;
CREATE TRIGGER extract_transcript_removed AFTER DELETE ON history_transcripts BEGIN
 UPDATE extraction_events SET characters=coalesce((SELECT sum(length(json_extract(j.value,'$.text'))) FROM events e, json_each(e.envelope,'$.segments') j WHERE e.id=OLD.source AND json_extract(j.value,'$.type')='text'),0)+coalesce((SELECT sum(length(text)) FROM history_transcripts WHERE source=OLD.source),0)
 WHERE event=OLD.source AND state IN('queued','blank') AND batch IS NULL;
END;
CREATE TRIGGER erase_extract AFTER INSERT ON invalidated_sources BEGIN
 UPDATE extraction_events SET state='erased' WHERE event=NEW.source;
 UPDATE extraction_batches SET state='invalid',diagnostic='selected_source_erased' WHERE id IN(SELECT batch FROM extraction_sources WHERE event=NEW.source);
 UPDATE extraction_events SET state='erased' WHERE batch IN(SELECT id FROM extraction_batches WHERE state='invalid') AND state='reserved';
 UPDATE extraction_sources SET subjects=NULL WHERE batch IN(SELECT id FROM extraction_batches WHERE state='invalid');
 UPDATE extract_fact_details SET category=NULL,kind=NULL,importance=NULL,temporal_mode=NULL,valid_from=NULL,valid_until=NULL
 WHERE fact IN(SELECT fact FROM extraction_results WHERE event=NEW.source);
END;
CREATE TABLE extraction_outbound(batch TEXT NOT NULL REFERENCES extraction_batches(id),outbound INTEGER NOT NULL REFERENCES social_outbound(id),version TEXT NOT NULL,PRIMARY KEY(batch,outbound));
