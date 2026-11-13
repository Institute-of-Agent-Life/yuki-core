-- Reader 57. Older batches kept the resolved alias but omitted its event dependency.
-- A historical native key may have multiple matching locators. Bind every candidate
-- consistent with the stored author so erasure does not depend on a guessed match.
INSERT OR IGNORE INTO main_dependencies(execution,event)
 SELECT b.execution, target.id FROM extraction_batches b
 JOIN extraction_sources s ON s.batch=b.id
 JOIN events reply ON reply.id=s.event
 JOIN event_locators locator ON locator.receiver=reply.bot_stage
  AND locator.native_id=(SELECT json_extract(part.value,'$.native_id')
   FROM json_each(reply.envelope,'$.segments') part
   WHERE json_extract(part.value,'$.type')='reply' LIMIT 1)
 JOIN events target ON target.id=locator.event_id AND target.entry_id=reply.entry_id
  AND target.raw_actor=json_extract(s.subjects,'$.reply_author');

CREATE TEMP TABLE extraction_revoked_upgrade(batch TEXT PRIMARY KEY,execution TEXT NOT NULL);
INSERT INTO extraction_revoked_upgrade
 SELECT b.id,b.execution FROM extraction_batches b
 WHERE EXISTS(SELECT 1 FROM main_dependencies d JOIN invalidated_sources i ON i.source=d.event WHERE d.execution=b.execution)
  OR EXISTS(SELECT 1 FROM extraction_sources s JOIN invalidated_sources i ON i.source=s.event WHERE s.batch=b.id);
CREATE TEMP TABLE extraction_revoked_facts(fact TEXT PRIMARY KEY);
INSERT INTO extraction_revoked_facts
 SELECT DISTINCT fact FROM memory_versions WHERE execution IN(SELECT execution FROM extraction_revoked_upgrade);
UPDATE memory_facts SET status='erased' WHERE status!='erased' AND id IN(SELECT fact FROM extraction_revoked_facts);
UPDATE memory_versions SET body=NULL WHERE fact IN(SELECT fact FROM extraction_revoked_facts);
UPDATE memory_evidence SET quote=NULL WHERE fact IN(SELECT fact FROM extraction_revoked_facts);
DROP TABLE extraction_revoked_facts;
UPDATE model_attempts SET response=NULL,partial_bytes=NULL,intents_json=NULL,retry_proof=0,diagnostic='source_erased'
 WHERE logical IN(SELECT logical FROM extraction_batches WHERE id IN(SELECT batch FROM extraction_revoked_upgrade));
UPDATE model_requests SET request_json='{}'
 WHERE id IN(SELECT logical FROM extraction_batches WHERE id IN(SELECT batch FROM extraction_revoked_upgrade));
UPDATE extraction_batches SET state='invalid',diagnostic='selected_source_erased',task='{}'
 WHERE id IN(SELECT batch FROM extraction_revoked_upgrade);
UPDATE extraction_events SET state='erased' WHERE state='reserved' AND batch IN(SELECT batch FROM extraction_revoked_upgrade);
UPDATE extraction_sources SET subjects=NULL WHERE batch IN(SELECT batch FROM extraction_revoked_upgrade);
DROP TABLE extraction_revoked_upgrade;

DROP TRIGGER erase_extract;
CREATE TRIGGER erase_extract AFTER INSERT ON invalidated_sources BEGIN
 UPDATE extraction_events SET state='erased' WHERE event=NEW.source;
 UPDATE extraction_batches SET state='invalid',diagnostic='selected_source_erased',task='{}'
 WHERE execution IN(SELECT execution FROM main_dependencies WHERE event=NEW.source)
  OR id IN(SELECT batch FROM extraction_sources WHERE event=NEW.source);
 UPDATE extraction_events SET state='erased' WHERE batch IN(SELECT id FROM extraction_batches WHERE state='invalid') AND state='reserved';
 UPDATE extraction_sources SET subjects=NULL WHERE batch IN(SELECT id FROM extraction_batches WHERE state='invalid');
END;
