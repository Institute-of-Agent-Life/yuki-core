CREATE TABLE terminal_entry_calls (
 handle TEXT PRIMARY KEY REFERENCES host_calls(handle),
 entry TEXT NOT NULL REFERENCES entries(id), actor TEXT NOT NULL REFERENCES persons(id),
 bot TEXT NOT NULL REFERENCES assignments(id),
 plugin TEXT NOT NULL REFERENCES host_installations(plugin), deployment TEXT NOT NULL,
 tool TEXT NOT NULL, launch TEXT REFERENCES terminal_entry_calls(handle), run TEXT,
 UNIQUE(plugin,run), CHECK(run IS NULL OR tool='terminal_exec'));

CREATE VIEW terminal_calls AS
 SELECT handle,work,NULL AS entry,NULL AS actor,NULL AS bot,plugin,deployment,tool,launch,run
 FROM terminal_work_calls
 UNION ALL
 SELECT handle,NULL,entry,actor,bot,plugin,deployment,tool,launch,run
 FROM terminal_entry_calls;

CREATE TRIGGER erase_terminal_entry_run AFTER INSERT ON invalidated_sources BEGIN
 UPDATE terminal_entry_calls SET run=NULL WHERE run IS NOT NULL AND handle IN(
   SELECT h.handle FROM host_calls h JOIN executions x ON x.id=h.execution WHERE
   x.source=NEW.source OR x.id IN(SELECT execution FROM main_dependencies WHERE event=NEW.source));
END;
