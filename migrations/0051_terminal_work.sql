-- The Host handle is the original Manager request key. Observations never own a run.
CREATE TABLE terminal_work_calls (
 handle TEXT PRIMARY KEY REFERENCES host_calls(handle), work TEXT NOT NULL REFERENCES work_nodes(id),
 plugin TEXT NOT NULL REFERENCES host_installations(plugin), deployment TEXT NOT NULL, tool TEXT NOT NULL,
 launch TEXT REFERENCES terminal_work_calls(handle), run TEXT, UNIQUE(plugin,run),
 CHECK(run IS NULL OR tool='terminal_exec'));
CREATE TRIGGER erase_terminal_run AFTER INSERT ON invalidated_sources BEGIN
 UPDATE terminal_work_calls SET run=NULL WHERE run IS NOT NULL AND
 (work IN(SELECT work FROM work_sources WHERE event=NEW.source) OR handle IN(
   SELECT h.handle FROM host_calls h JOIN executions x ON x.id=h.execution WHERE x.source=NEW.source
   OR x.id IN(SELECT execution FROM main_dependencies WHERE event=NEW.source)));
END;
