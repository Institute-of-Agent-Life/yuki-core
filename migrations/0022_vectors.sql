-- Reader 22. Fact authority remains in memory_facts and memory_versions.
PRAGMA defer_foreign_keys=ON;
CREATE TABLE expanded_main_origins(ref TEXT PRIMARY KEY NOT NULL CHECK(length(ref)>0),
 kind TEXT NOT NULL CHECK(kind IN('plugin','self','schedule','work','child','memory_extract','embedding')),
 conversation TEXT NOT NULL REFERENCES entries(id),parent TEXT REFERENCES executions(id),enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN(0,1)));
INSERT INTO expanded_main_origins SELECT ref,kind,conversation,parent,enabled FROM main_origins;
DROP TABLE main_origins;
ALTER TABLE expanded_main_origins RENAME TO main_origins;
PRAGMA defer_foreign_keys=OFF;

CREATE TABLE embedding_requests(
 logical TEXT PRIMARY KEY REFERENCES model_requests(id), profile TEXT NOT NULL, dimension INTEGER NOT NULL, purpose TEXT NOT NULL CHECK(purpose IN('document','reembed','query')),
 execution TEXT NOT NULL REFERENCES executions(id), cache_key TEXT, privacy_epoch INTEGER NOT NULL, state TEXT NOT NULL CHECK(state IN('prepared','done','partial','failed','unknown','erased')), expected INTEGER NOT NULL);
CREATE TABLE embedding_items(logical TEXT NOT NULL REFERENCES embedding_requests(logical),ordinal INTEGER NOT NULL, fact TEXT NOT NULL REFERENCES memory_facts(id),
 revision INTEGER NOT NULL,content_hash TEXT NOT NULL,state TEXT NOT NULL CHECK(state IN('pending','stored','stale','missing','unknown','released')),
 PRIMARY KEY(logical,ordinal));
CREATE INDEX embedding_fact_jobs ON embedding_items(fact,revision,state,logical);
CREATE TABLE fact_vectors(fact TEXT NOT NULL REFERENCES memory_facts(id),profile TEXT NOT NULL,purpose TEXT NOT NULL CHECK(purpose='document'),
 dimension INTEGER NOT NULL,content_hash TEXT NOT NULL,revision INTEGER NOT NULL,vector BLOB NOT NULL CHECK(length(vector)=4*dimension),
 PRIMARY KEY(fact,profile,purpose,content_hash));
CREATE INDEX vector_coverage ON fact_vectors(profile,revision,fact);
-- Idempotency receipt for a cache hit. No query, vector, result IDs or ACL is stored.
CREATE TABLE embedding_query_hits(logical TEXT PRIMARY KEY,execution TEXT NOT NULL REFERENCES executions(id),profile TEXT NOT NULL,cache_key TEXT,epoch INTEGER NOT NULL);
CREATE TABLE embedding_privacy(id INTEGER PRIMARY KEY CHECK(id=1), epoch INTEGER NOT NULL);
INSERT INTO embedding_privacy VALUES(1,0);
CREATE TRIGGER invalidate_vector_revision AFTER UPDATE OF revision,status ON memory_facts
WHEN NEW.status!='active' OR NEW.revision!=OLD.revision BEGIN
 DELETE FROM fact_vectors WHERE fact=NEW.id;
 UPDATE embedding_items SET state='stale' WHERE fact=NEW.id AND state!='released';
END;
CREATE TRIGGER clean_embedding_source AFTER INSERT ON invalidated_sources BEGIN
 UPDATE embedding_privacy SET epoch=epoch+1;
 UPDATE embedding_query_hits SET cache_key=NULL WHERE execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
 UPDATE embedding_requests SET state='erased' WHERE execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
END;
