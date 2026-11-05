-- Reader 16. Absolute deadlines and paid-request watermarks survive replay.
CREATE TABLE work_request_marks(logical TEXT PRIMARY KEY REFERENCES model_requests(id),event INTEGER NOT NULL);
CREATE TRIGGER record_work_request_mark AFTER INSERT ON model_requests BEGIN
 INSERT INTO work_request_marks VALUES(NEW.id,coalesce((SELECT max(id) FROM events),0));
END;
CREATE TABLE work_waits(id TEXT PRIMARY KEY,work TEXT NOT NULL REFERENCES work_nodes(id),execution TEXT NOT NULL REFERENCES executions(id),
 logical TEXT NOT NULL REFERENCES model_requests(id),operation TEXT NOT NULL,mode TEXT NOT NULL CHECK(mode IN('any','all')),
 generation INTEGER NOT NULL,since_event INTEGER NOT NULL,created INTEGER NOT NULL,deadline INTEGER,
 state TEXT NOT NULL CHECK(state IN('proposed','armed','woke','cancelled','expired')),wake_execution TEXT REFERENCES executions(id),UNIQUE(execution,operation));
CREATE UNIQUE INDEX one_active_wait ON work_waits(work) WHERE state IN('proposed','armed');
CREATE TABLE work_wait_terms(wait TEXT NOT NULL REFERENCES work_waits(id),ordinal INTEGER NOT NULL,kind TEXT NOT NULL,
 target TEXT,detail TEXT,due INTEGER,hit TEXT,event INTEGER REFERENCES events(id),PRIMARY KEY(wait,ordinal));
CREATE TABLE work_wait_routed(event INTEGER NOT NULL REFERENCES events(id),work TEXT NOT NULL REFERENCES work_nodes(id),PRIMARY KEY(event,work));
CREATE TABLE work_explicit_answers(event INTEGER PRIMARY KEY REFERENCES events(id),requested TEXT NOT NULL,matched INTEGER NOT NULL CHECK(matched IN(0,1)));
CREATE TABLE work_suspensions(work TEXT PRIMARY KEY REFERENCES work_nodes(id),reason TEXT NOT NULL);
CREATE TABLE work_progress(execution TEXT NOT NULL,operation TEXT NOT NULL,latest TEXT,updates INTEGER NOT NULL,
 PRIMARY KEY(execution,operation),FOREIGN KEY(execution,operation) REFERENCES operations(namespace,operation_id));
CREATE TABLE work_user_questions(work TEXT PRIMARY KEY REFERENCES work_nodes(id),execution TEXT NOT NULL REFERENCES executions(id),
 generation INTEGER NOT NULL,source_event INTEGER NOT NULL, outbound INTEGER REFERENCES social_outbound(id));
CREATE TABLE work_wait_mail(mail INTEGER NOT NULL REFERENCES work_mail(id),source_event INTEGER NOT NULL REFERENCES events(id),PRIMARY KEY(mail,source_event));
CREATE TRIGGER cancel_work_wait AFTER UPDATE OF status ON work_nodes WHEN NEW.status IN ('completed','cancelled','failed') BEGIN
 UPDATE work_waits SET state='cancelled' WHERE work=NEW.id AND state IN('armed','proposed');
 DELETE FROM work_user_questions WHERE work=NEW.id;
END;
CREATE TRIGGER erase_wait_material AFTER INSERT ON invalidated_sources BEGIN
 UPDATE work_wait_terms SET hit=NULL,event=NULL WHERE event=NEW.source AND wait IN(SELECT id FROM work_waits WHERE state IN('proposed','armed'));
 UPDATE work_mail SET body=NULL WHERE id IN(SELECT mail FROM work_wait_mail WHERE source_event=NEW.source);
 DELETE FROM work_suspensions WHERE work IN(SELECT id FROM work_nodes WHERE owner_execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source) UNION SELECT work FROM work_sources WHERE event=NEW.source);
 UPDATE work_progress SET latest=NULL WHERE execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
END;
