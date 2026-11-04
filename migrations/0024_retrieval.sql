-- Reader 24. Rebuildable trigram postings mirror only active, non-erased facts.
CREATE VIRTUAL TABLE memory_retrieval_fts USING fts5(content,memory_key,category,tokenize='trigram');
CREATE VIEW retrieval_documents AS SELECT f.number,f.id,f.kind AS scope_type,f.person,f.space,f.visibility_entry,f.fact_key,
  v.body AS content,v.confidence,v.created_at,d.valid_from,d.valid_until,coalesce(d.kind,'fact') AS kind,coalesce(d.category,'') AS category,
  coalesce(d.importance,3) AS importance,f.revision
 FROM memory_facts f JOIN memory_versions v ON v.fact=f.id AND v.revision=f.revision LEFT JOIN extract_fact_details d ON d.fact=f.id
 WHERE f.status='active' AND v.body IS NOT NULL;
INSERT INTO memory_retrieval_fts(rowid,content,memory_key,category) SELECT number,content,fact_key,category FROM retrieval_documents;
CREATE TRIGGER retrieval_revision AFTER INSERT ON memory_versions BEGIN
 DELETE FROM memory_retrieval_fts WHERE rowid=(SELECT number FROM memory_facts WHERE id=NEW.fact);
 INSERT INTO memory_retrieval_fts(rowid,content,memory_key,category) SELECT number,content,fact_key,category FROM retrieval_documents WHERE id=NEW.fact;
END;
CREATE TRIGGER retrieval_category AFTER INSERT ON extract_fact_details BEGIN
 UPDATE memory_retrieval_fts SET category=coalesce(NEW.category,'') WHERE rowid=(SELECT number FROM memory_facts WHERE id=NEW.fact AND status='active');
END;
CREATE TRIGGER retrieval_category_edit AFTER UPDATE OF category ON extract_fact_details BEGIN
 UPDATE memory_retrieval_fts SET category=coalesce(NEW.category,'') WHERE rowid=(SELECT number FROM memory_facts WHERE id=NEW.fact AND status='active');
END;
CREATE TRIGGER forget_retrieval_fact AFTER UPDATE OF status ON memory_facts WHEN NEW.status!='active' BEGIN
 DELETE FROM memory_retrieval_fts WHERE rowid=NEW.number;
END;
CREATE TABLE retrieval_profiles(profile TEXT PRIMARY KEY,dim INTEGER NOT NULL,upper_bound INTEGER NOT NULL,cursor INTEGER NOT NULL DEFAULT 0);
CREATE TABLE retrieval_index_queue(profile TEXT NOT NULL REFERENCES retrieval_profiles(profile),fact TEXT NOT NULL REFERENCES memory_facts(id),
 PRIMARY KEY(profile,fact));
CREATE TABLE retrieval_index_members(profile TEXT NOT NULL REFERENCES retrieval_profiles(profile),fact TEXT NOT NULL REFERENCES memory_facts(id),
 revision INTEGER NOT NULL,content_hash TEXT NOT NULL,PRIMARY KEY(profile,fact));
CREATE TRIGGER queue_vector_insert AFTER INSERT ON fact_vectors BEGIN
 INSERT OR IGNORE INTO retrieval_index_queue SELECT profile,NEW.fact FROM retrieval_profiles WHERE profile=NEW.profile;
END;
CREATE TRIGGER queue_vector_delete AFTER DELETE ON fact_vectors BEGIN
 INSERT OR IGNORE INTO retrieval_index_queue SELECT profile,OLD.fact FROM retrieval_profiles WHERE profile=OLD.profile;
END;
CREATE TRIGGER queue_vector_update AFTER UPDATE ON fact_vectors BEGIN
 INSERT OR IGNORE INTO retrieval_index_queue SELECT profile,NEW.fact FROM retrieval_profiles WHERE profile=NEW.profile;
 INSERT OR IGNORE INTO retrieval_index_queue SELECT profile,OLD.fact FROM retrieval_profiles WHERE profile=OLD.profile;
END;
