-- Reader 36. Diagnostic cursors reference original facts; they never settle effects.
CREATE TABLE control_trace_entries(
 id INTEGER PRIMARY KEY AUTOINCREMENT, execution TEXT NOT NULL REFERENCES executions(id),
 kind TEXT NOT NULL, operation TEXT, attempt TEXT, state TEXT NOT NULL,
 created INTEGER NOT NULL DEFAULT(unixepoch()), expires INTEGER NOT NULL DEFAULT(unixepoch()+2592000)
);
CREATE INDEX control_trace_execution ON control_trace_entries(execution,id);
CREATE INDEX control_trace_expiry ON control_trace_entries(expires);
CREATE TRIGGER trace_execution_created AFTER INSERT ON executions BEGIN
 INSERT INTO control_trace_entries(execution,kind,state) VALUES(NEW.id,'execution',NEW.state);
END;
CREATE TRIGGER trace_execution_transition AFTER UPDATE OF state,cancelled ON executions
WHEN NEW.state!=OLD.state OR NEW.cancelled!=OLD.cancelled BEGIN
 INSERT INTO control_trace_entries(execution,kind,state) VALUES(NEW.id,'execution',CASE WHEN NEW.cancelled THEN 'cancelled' ELSE NEW.state END);
END;
CREATE TRIGGER trace_operation_created AFTER INSERT ON operations BEGIN
 INSERT INTO control_trace_entries(execution,kind,operation,state) VALUES(NEW.namespace,'tool',NEW.operation_id,NEW.state);
END;
CREATE TRIGGER trace_operation_transition AFTER UPDATE OF state ON operations WHEN NEW.state!=OLD.state BEGIN
 INSERT INTO control_trace_entries(execution,kind,operation,state) VALUES(NEW.namespace,'tool',NEW.operation_id,NEW.state);
END;
CREATE TRIGGER trace_model_created AFTER INSERT ON model_attempts BEGIN
 INSERT INTO control_trace_entries(execution,kind,attempt,state)
 SELECT a.execution,'model',NEW.id,NEW.state FROM model_requests r JOIN activations a ON a.id=r.activation WHERE r.id=NEW.logical;
END;
CREATE TRIGGER trace_model_transition AFTER UPDATE OF state ON model_attempts WHEN NEW.state!=OLD.state BEGIN
 INSERT INTO control_trace_entries(execution,kind,attempt,state)
 SELECT a.execution,'model',NEW.id,NEW.state FROM model_requests r JOIN activations a ON a.id=r.activation WHERE r.id=NEW.logical;
END;
CREATE TRIGGER expire_control_trace AFTER INSERT ON control_trace_entries BEGIN
 DELETE FROM control_trace_entries WHERE expires<=unixepoch();
END;
CREATE TRIGGER erase_control_trace AFTER INSERT ON invalidated_sources BEGIN
 DELETE FROM control_trace_entries WHERE execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
END;
