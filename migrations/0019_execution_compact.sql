-- Reader 19. The normal paid attempt and the last paired batch remain authoritative.
CREATE TABLE exec_compacts(id TEXT PRIMARY KEY,execution TEXT NOT NULL REFERENCES executions(id),
 work TEXT REFERENCES work_nodes(id),generation INTEGER NOT NULL,profile TEXT NOT NULL,source_logical TEXT NOT NULL REFERENCES model_requests(id),
 previous TEXT REFERENCES exec_compacts(id),candidate TEXT NOT NULL,covered INTEGER NOT NULL,
 logical TEXT NOT NULL UNIQUE,base TEXT,selected TEXT,retained TEXT,summary TEXT,diagnostic TEXT,
 state TEXT NOT NULL CHECK(state IN ('pending','published','rejected','invalid')),UNIQUE(execution,candidate));
CREATE INDEX exec_compact_scan ON exec_compacts(execution,state);
CREATE TRIGGER erase_exec_compaction AFTER INSERT ON invalidated_sources BEGIN
 UPDATE exec_compacts SET base=NULL,selected=NULL,retained=NULL,summary=NULL,state='invalid',diagnostic='source_erased'
 WHERE execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source)
 OR work IN(SELECT work FROM work_sources WHERE event=NEW.source);
END;
CREATE TRIGGER reset_exec_compaction AFTER UPDATE OF generation ON main_windows WHEN OLD.generation!=NEW.generation BEGIN
 UPDATE exec_compacts SET base=NULL,selected=NULL,retained=NULL,summary=NULL,state='invalid',diagnostic='window_reset'
 WHERE execution IN(SELECT execution FROM main_turns WHERE conversation=NEW.conversation AND window=OLD.generation);
END;

-- A completely received, unusable local compact can be terminal without any
-- business tool result. An unknown or native-effectful attempt still blocks the root.
CREATE VIEW closed_local_compacts AS
 SELECT p.logical FROM model_attempts p JOIN model_requests r ON r.id=p.logical
 JOIN (SELECT logical,state FROM compact_jobs UNION ALL SELECT logical,state FROM exec_compacts) j ON j.logical=p.logical
 WHERE j.state='rejected' AND p.state='response_held' AND p.status_code=200 AND p.ever_unknown=0 AND p.partial_bytes IS NULL
 AND r.native_tools=0 AND p.diagnostic IN ('public_compact_called_tool_or_paused','private_compact_called_tool_or_paused','compact_local_response_invalid');
-- Bind appended public inputs to their original admitted events. Private runtime
-- notes and user-authored JSON cannot create a public or paid tool observation.
CREATE TABLE exec_public_tails(execution TEXT NOT NULL REFERENCES executions(id),first_event INTEGER NOT NULL REFERENCES events(id),
 last_event INTEGER NOT NULL REFERENCES events(id),message TEXT,PRIMARY KEY(execution,first_event,last_event));
CREATE TRIGGER forget_exec_public_tails AFTER INSERT ON invalidated_sources BEGIN
 UPDATE exec_public_tails SET message=NULL WHERE execution IN(SELECT execution FROM main_inputs WHERE event=NEW.source);
END;
