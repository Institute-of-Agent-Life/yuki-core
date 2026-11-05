-- Reader 44. Capture metadata against the actual fact revision; old unknown history is not invented.
CREATE TABLE fact_metadata_versions(
 fact TEXT NOT NULL, revision INTEGER NOT NULL, memory_key TEXT, category TEXT, kind TEXT, importance INTEGER,
 temporal_mode TEXT,valid_from TEXT,valid_until TEXT,
 PRIMARY KEY(fact,revision),FOREIGN KEY(fact,revision) REFERENCES memory_versions(fact,revision));
INSERT INTO fact_metadata_versions
 SELECT f.id,f.revision,CASE WHEN f.status!='erased' THEN f.fact_key END,d.category,d.kind,d.importance,d.temporal_mode,d.valid_from,d.valid_until
 FROM memory_facts f LEFT JOIN extract_fact_details d ON d.fact=f.id;
CREATE TRIGGER fact_metadata_version AFTER INSERT ON memory_versions BEGIN
 INSERT INTO fact_metadata_versions
 SELECT f.id,NEW.revision,CASE WHEN f.status!='erased' THEN f.fact_key END,d.category,d.kind,d.importance,d.temporal_mode,d.valid_from,d.valid_until
 FROM memory_facts f LEFT JOIN extract_fact_details d ON d.fact=f.id WHERE f.id=NEW.fact;
END;
CREATE TRIGGER fact_metadata_initial AFTER INSERT ON extract_fact_details
 WHEN (SELECT status FROM memory_facts WHERE id=NEW.fact)!='erased' BEGIN
 UPDATE fact_metadata_versions SET category=NEW.category,kind=NEW.kind,importance=NEW.importance,
 temporal_mode=NEW.temporal_mode,valid_from=NEW.valid_from,valid_until=NEW.valid_until
 WHERE fact=NEW.fact AND revision=(SELECT revision FROM memory_facts WHERE id=NEW.fact);
END;
CREATE TRIGGER fact_metadata_current AFTER UPDATE ON extract_fact_details
 WHEN (SELECT status FROM memory_facts WHERE id=NEW.fact)!='erased' BEGIN
 UPDATE fact_metadata_versions SET category=NEW.category,kind=NEW.kind,importance=NEW.importance,
 temporal_mode=NEW.temporal_mode,valid_from=NEW.valid_from,valid_until=NEW.valid_until
 WHERE fact=NEW.fact AND revision=(SELECT revision FROM memory_facts WHERE id=NEW.fact);
END;
CREATE TRIGGER fact_metadata_key AFTER UPDATE OF fact_key ON memory_facts WHEN NEW.status!='erased' BEGIN
 UPDATE fact_metadata_versions SET memory_key=NEW.fact_key WHERE fact=NEW.id AND revision=NEW.revision;
 DELETE FROM memory_retrieval_fts WHERE rowid=NEW.number;
 INSERT INTO memory_retrieval_fts(rowid,content,memory_key,category) SELECT number,content,fact_key,category FROM retrieval_documents WHERE id=NEW.id;
END;
CREATE TRIGGER erase_fact_metadata AFTER UPDATE OF status ON memory_facts WHEN NEW.status='erased' BEGIN
 UPDATE fact_metadata_versions SET memory_key=NULL,category=NULL,kind=NULL,importance=NULL,
 temporal_mode=NULL,valid_from=NULL,valid_until=NULL WHERE fact=NEW.id;
END;
UPDATE fact_metadata_versions SET memory_key=NULL,category=NULL,kind=NULL,importance=NULL,temporal_mode=NULL,valid_from=NULL,valid_until=NULL
 WHERE fact IN(SELECT id FROM memory_facts WHERE status='erased');
