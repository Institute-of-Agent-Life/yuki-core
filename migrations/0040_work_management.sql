-- Preserve unknown historical timing; new Work observations use actual writer clocks.
CREATE TABLE work_admin_metadata(work TEXT PRIMARY KEY REFERENCES work_nodes(id),created_ms INTEGER,generation INTEGER,reason TEXT);
INSERT INTO work_admin_metadata SELECT w.id,NULL,t.window,NULL FROM work_nodes w LEFT JOIN main_turns t ON t.execution=w.owner_execution;
CREATE TRIGGER work_admin_created AFTER INSERT ON work_nodes BEGIN
 INSERT INTO work_admin_metadata SELECT NEW.id,NEW.updated,t.window,NULL FROM main_turns t WHERE t.execution=NEW.owner_execution;
END;
CREATE TABLE work_run_activity(execution TEXT PRIMARY KEY REFERENCES work_runs(execution),active_ms INTEGER,running_since INTEGER);
INSERT INTO work_run_activity SELECT execution,NULL,NULL FROM work_runs;
CREATE TRIGGER work_run_started AFTER INSERT ON work_runs BEGIN
 INSERT INTO work_run_activity SELECT NEW.execution,0,CASE WHEN state='running' THEN CAST(unixepoch('subsec')*1000 AS INTEGER) END FROM executions WHERE id=NEW.execution;
END;
CREATE TRIGGER work_run_timing AFTER UPDATE OF state ON executions WHEN NEW.state<>OLD.state BEGIN
 UPDATE work_run_activity SET
 active_ms=CASE WHEN running_since IS NOT NULL THEN active_ms+max(0,CAST(unixepoch('subsec')*1000 AS INTEGER)-running_since) ELSE active_ms END,
 running_since=CASE WHEN NEW.state='running' THEN CAST(unixepoch('subsec')*1000 AS INTEGER) END WHERE execution=NEW.id;
END;
CREATE TRIGGER work_suspend_revision AFTER INSERT ON work_suspensions BEGIN
 UPDATE personal_work_revisions SET revision=revision+1 WHERE work=NEW.work;
END;
CREATE TRIGGER work_suspension_changed AFTER UPDATE OF reason ON work_suspensions WHEN NEW.reason<>OLD.reason BEGIN
 UPDATE personal_work_revisions SET revision=revision+1 WHERE work=NEW.work;
END;
ALTER TABLE work_waits ADD COLUMN delivered_at INTEGER;
CREATE TRIGGER work_wait_delivered_at AFTER UPDATE OF state ON work_waits
WHEN NEW.state<>OLD.state AND NEW.state IN('woke','expired') BEGIN
 UPDATE work_waits SET delivered_at=CAST(unixepoch('subsec')*1000 AS INTEGER) WHERE id=NEW.id;
END;
