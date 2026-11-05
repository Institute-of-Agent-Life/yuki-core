-- Reader 15. Subjects and original source material have separate authorization.
CREATE TABLE memory_facts(
 number INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,kind TEXT NOT NULL CHECK(kind IN('person','person_group','group','self')),
 person TEXT REFERENCES persons(id),space TEXT,visibility TEXT NOT NULL CHECK(visibility IN('subject','global','private','group')),
 visibility_entry TEXT REFERENCES entries(id),fact_key TEXT NOT NULL,created_execution TEXT NOT NULL REFERENCES executions(id),
 revision INTEGER NOT NULL DEFAULT 1, status TEXT NOT NULL CHECK(status IN('active','retracted','erased')),
 CHECK((kind IN('person','person_group'))=(person IS NOT NULL)),CHECK((kind IN('group','person_group'))=(space IS NOT NULL)),
 CHECK((kind='self')=(visibility!='subject')),CHECK((visibility IN('group','private'))=(visibility_entry IS NOT NULL)));
CREATE INDEX memory_subject ON memory_facts(kind,person,space,id);
CREATE TABLE memory_versions(fact TEXT NOT NULL REFERENCES memory_facts(id),revision INTEGER NOT NULL,body TEXT,confidence REAL CHECK(confidence>=0 AND confidence<=1),
 execution TEXT NOT NULL REFERENCES executions(id),created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY(fact,revision));
CREATE TABLE memory_evidence(id INTEGER PRIMARY KEY AUTOINCREMENT,fact TEXT NOT NULL REFERENCES memory_facts(id),
 kind TEXT NOT NULL CHECK(kind IN('event','tool','run')),source_key TEXT NOT NULL,source_version TEXT NOT NULL,entry TEXT NOT NULL REFERENCES entries(id),
 author TEXT NOT NULL REFERENCES persons(id),origin_execution TEXT REFERENCES executions(id),event INTEGER REFERENCES events(id),quote TEXT,
 CHECK((kind='event')=(event IS NOT NULL)),UNIQUE(fact,kind,source_key,source_version,quote));
CREATE INDEX memory_evidence_sources ON memory_evidence(event,origin_execution,fact);
CREATE TABLE memory_operations(execution TEXT NOT NULL REFERENCES executions(id),original_key TEXT NOT NULL,request_hash TEXT NOT NULL,
 fact TEXT NOT NULL REFERENCES memory_facts(id),revision INTEGER NOT NULL,outcome TEXT NOT NULL,PRIMARY KEY(execution,original_key));
CREATE TABLE memory_paid_reads(execution TEXT NOT NULL REFERENCES executions(id),fact TEXT NOT NULL REFERENCES memory_facts(id),revision INTEGER NOT NULL,proof TEXT NOT NULL,PRIMARY KEY(execution,fact));
CREATE TABLE memory_command_events(event INTEGER PRIMARY KEY REFERENCES events(id),execution TEXT UNIQUE NOT NULL REFERENCES executions(id),reply_state TEXT);
CREATE VIEW memory_group_relations AS SELECT DISTINCT coalesce(p.current_root,p.id) AS person,d.space FROM events e JOIN entries d ON d.id=e.entry_id AND d.scope='group'
 JOIN persons p ON p.id=e.raw_actor AND p.enabled JOIN persons root ON root.id=coalesce(p.current_root,p.id) AND root.enabled
 WHERE json_extract(e.envelope,'$.kind')='message' AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id);
CREATE TRIGGER erase_memory_source AFTER INSERT ON invalidated_sources BEGIN
 UPDATE memory_facts SET status='erased' WHERE id IN(SELECT fact FROM memory_evidence WHERE event=NEW.source OR origin_execution IN(
 SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source))
 OR id IN(SELECT fact FROM memory_versions WHERE execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source));
 UPDATE memory_versions SET body=NULL WHERE fact IN(SELECT id FROM memory_facts WHERE status='erased');
 UPDATE memory_evidence SET quote=NULL WHERE fact IN(SELECT id FROM memory_facts WHERE status='erased');
END;
