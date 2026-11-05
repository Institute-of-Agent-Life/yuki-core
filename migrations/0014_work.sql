-- Reader 14. Older raw execution.work_id values stay readable as unimported references.
CREATE TABLE work_nodes(id TEXT PRIMARY KEY,root TEXT NOT NULL REFERENCES work_nodes(id),parent TEXT REFERENCES work_nodes(id),
 owner_execution TEXT NOT NULL REFERENCES executions(id),principal TEXT NOT NULL,entry TEXT NOT NULL REFERENCES entries(id),
 budget TEXT NOT NULL REFERENCES root_budgets(root),profile TEXT NOT NULL,goal TEXT,note TEXT,status TEXT NOT NULL
 CHECK(status IN('queued','active','need_input','completed','cancelled','failed')),summary TEXT,
 updated INTEGER NOT NULL,terminal_operation TEXT,UNIQUE(owner_execution,terminal_operation));
CREATE TABLE work_sources(work TEXT NOT NULL REFERENCES work_nodes(id),event INTEGER NOT NULL REFERENCES events(id),PRIMARY KEY(work,event));
CREATE TABLE work_accept_sources(event INTEGER PRIMARY KEY REFERENCES events(id),work TEXT UNIQUE NOT NULL REFERENCES work_nodes(id));
CREATE INDEX work_children ON work_nodes(parent,id);
CREATE INDEX work_directory ON work_nodes(entry,principal,updated,id);
CREATE TABLE work_runs(execution TEXT PRIMARY KEY REFERENCES executions(id),work TEXT NOT NULL REFERENCES work_nodes(id),occurrence TEXT NOT NULL,
 kind TEXT NOT NULL,UNIQUE(work,occurrence));
CREATE VIEW main_work_lanes AS SELECT t.execution, CASE WHEN r.kind IN('child','resume') THEN 'work:' || r.work ELSE 'entry:' || t.conversation END AS lane FROM main_turns t LEFT JOIN work_runs r ON r.execution=t.execution;
CREATE INDEX work_activations ON work_runs(work,execution);
CREATE TABLE work_proposals(execution TEXT NOT NULL REFERENCES work_runs(execution),operation TEXT NOT NULL,logical TEXT NOT NULL REFERENCES model_requests(id),
 work TEXT NOT NULL REFERENCES work_nodes(id),summary TEXT,kind TEXT NOT NULL CHECK(kind IN('completed','need_input')),PRIMARY KEY(execution,operation));
CREATE TABLE work_mail(id INTEGER PRIMARY KEY AUTOINCREMENT,work TEXT NOT NULL REFERENCES work_nodes(id),origin_execution TEXT NOT NULL REFERENCES executions(id),
 source_key TEXT NOT NULL,kind TEXT NOT NULL,body TEXT,reply_to INTEGER REFERENCES work_mail(id),UNIQUE(work,origin_execution,source_key));
CREATE INDEX work_inbox ON work_mail(work,id);
CREATE TABLE work_consumption(work TEXT PRIMARY KEY REFERENCES work_nodes(id),watermark INTEGER NOT NULL);
CREATE TABLE work_activities(work TEXT NOT NULL REFERENCES work_nodes(id),execution TEXT NOT NULL REFERENCES executions(id),kind TEXT NOT NULL,ref TEXT NOT NULL,
 PRIMARY KEY(work,execution,kind,ref));
CREATE TRIGGER retain_work_receipt AFTER UPDATE OF state ON operations
WHEN NEW.state!=OLD.state AND NEW.state!='intent' BEGIN
 INSERT OR IGNORE INTO work_activities SELECT r.work,NEW.namespace,NEW.state,NEW.operation_id
 FROM work_runs r WHERE r.execution=NEW.namespace;
END;
CREATE TRIGGER erase_work_sources AFTER INSERT ON invalidated_sources BEGIN
 UPDATE work_nodes SET goal=NULL,note=NULL,summary=NULL WHERE id IN (SELECT work FROM work_sources WHERE event=NEW.source) OR owner_execution IN
 (SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
 UPDATE work_proposals SET summary=NULL WHERE execution IN
 (SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
 UPDATE work_mail SET body=NULL WHERE origin_execution IN
 (SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
END;
