-- Reader 30. Same root and exact operation own all direct and composed tool charges.
CREATE TABLE root_tool_budgets(root TEXT PRIMARY KEY REFERENCES root_budgets(root),max_calls INTEGER CHECK(max_calls>=0),used_calls INTEGER NOT NULL DEFAULT 0 CHECK(used_calls>=0));
CREATE TABLE tool_charges(execution TEXT NOT NULL,operation TEXT NOT NULL,root TEXT NOT NULL REFERENCES root_tool_budgets(root),charged INTEGER NOT NULL CHECK(charged IN(0,1)),accepted INTEGER NOT NULL CHECK(accepted IN(0,1)),admitted_at INTEGER,PRIMARY KEY(execution,operation),FOREIGN KEY(execution,operation) REFERENCES operations(namespace,operation_id));
INSERT INTO root_tool_budgets(root) SELECT root FROM root_budgets;
INSERT INTO tool_charges SELECT i.execution,i.operation_id,m.root,CASE WHEN o.action='external' AND o.target IN('task_control','subagent_start','subagent_control','complete','read_tool_artifact') THEN 0 ELSE 1 END,1,NULL
 FROM model_tool_intents i JOIN operations o ON o.namespace=i.execution AND o.operation_id=i.operation_id JOIN budget_members m ON m.execution=i.execution WHERE o.state!='intent';
UPDATE root_tool_budgets SET used_calls=(SELECT coalesce(sum(charged),0) FROM tool_charges c WHERE c.root=root_tool_budgets.root);
CREATE TABLE code_contracts(plugin TEXT NOT NULL,revision TEXT NOT NULL,body TEXT NOT NULL,PRIMARY KEY(plugin,revision));

CREATE TABLE code_parents(handle TEXT PRIMARY KEY REFERENCES host_calls(handle),execution TEXT NOT NULL,operation TEXT NOT NULL,
 contract TEXT NOT NULL,checkpoint INTEGER NOT NULL DEFAULT 0,next_ordinal INTEGER NOT NULL DEFAULT 0,feed_index INTEGER NOT NULL DEFAULT -1,
 snapshot TEXT,closed INTEGER NOT NULL DEFAULT 0 CHECK(closed IN(0,1)),FOREIGN KEY(execution,operation) REFERENCES operations(namespace,operation_id));
CREATE TABLE code_snapshots(handle TEXT NOT NULL REFERENCES code_parents(handle),id TEXT NOT NULL,sha256 TEXT NOT NULL,size INTEGER NOT NULL,codec TEXT NOT NULL,
 next_offset INTEGER NOT NULL DEFAULT 0,complete INTEGER NOT NULL DEFAULT 0 CHECK(complete IN(0,1)),bytes BLOB,PRIMARY KEY(handle,id));
CREATE TABLE code_snapshot_chunks(handle TEXT NOT NULL,id TEXT NOT NULL,offset INTEGER NOT NULL,sha256 TEXT NOT NULL,bytes BLOB,PRIMARY KEY(handle,id,offset),FOREIGN KEY(handle,id) REFERENCES code_snapshots(handle,id));
CREATE TABLE code_children(handle TEXT NOT NULL REFERENCES code_parents(handle),ordinal INTEGER NOT NULL,execution TEXT NOT NULL,operation TEXT NOT NULL,
 feed_index INTEGER NOT NULL,engine_call_id TEXT NOT NULL,snapshot TEXT NOT NULL,checkpoint INTEGER NOT NULL,
 admitted_at INTEGER NOT NULL,entered_at INTEGER,entered_generation TEXT,receipt TEXT,fed_snapshot TEXT,fed_checkpoint INTEGER,fed_feed_index INTEGER,
 PRIMARY KEY(handle,ordinal),UNIQUE(handle,feed_index,engine_call_id),UNIQUE(execution,operation),FOREIGN KEY(execution,operation) REFERENCES operations(namespace,operation_id));
CREATE TABLE code_resume_attempts(handle TEXT NOT NULL REFERENCES code_parents(handle),generation TEXT NOT NULL,state TEXT NOT NULL CHECK(state IN('queued','dispatched','finished')),PRIMARY KEY(handle,generation));
CREATE TRIGGER erase_code_sources AFTER INSERT ON invalidated_sources BEGIN
 UPDATE code_parents SET snapshot=NULL,closed=1 WHERE execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
 UPDATE code_snapshots SET bytes=NULL WHERE handle IN(SELECT handle FROM code_parents WHERE execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source));
 DELETE FROM code_snapshot_chunks WHERE handle IN(SELECT handle FROM code_parents WHERE execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source));
 UPDATE code_children SET receipt=NULL WHERE handle IN(SELECT handle FROM code_parents WHERE execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source));
END;
