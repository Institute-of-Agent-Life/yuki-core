-- Reader 18. Definitions are versioned privately, independent of immutable scheduled occurrences.
CREATE TABLE schedules(
 id INTEGER PRIMARY KEY AUTOINCREMENT, owner TEXT NOT NULL, creator_execution TEXT NOT NULL REFERENCES executions(id),
 entry TEXT NOT NULL REFERENCES entries(id), version INTEGER NOT NULL DEFAULT 1,
 status TEXT NOT NULL CHECK(status IN('active','paused','completed','cancelled','blocked','failed')),
 next_due INTEGER,run_count INTEGER NOT NULL DEFAULT 0,max_runs INTEGER CHECK(max_runs>=1),
 self_receiver TEXT REFERENCES assignments(id));
CREATE INDEX schedule_due ON schedules(next_due,id) WHERE status='active';
CREATE TABLE schedule_versions(schedule INTEGER NOT NULL REFERENCES schedules(id),version INTEGER NOT NULL,
 goal TEXT,name TEXT,trigger_json TEXT NOT NULL,zone TEXT NOT NULL,PRIMARY KEY(schedule,version));
CREATE TABLE schedule_sources(schedule INTEGER NOT NULL,version INTEGER NOT NULL,event INTEGER NOT NULL REFERENCES events(id),
 PRIMARY KEY(schedule,version,event),FOREIGN KEY(schedule,version) REFERENCES schedule_versions(schedule,version));
CREATE TABLE schedule_occurrences(
 id INTEGER PRIMARY KEY AUTOINCREMENT,schedule INTEGER NOT NULL,version INTEGER NOT NULL,planned_utc INTEGER NOT NULL,
 execution TEXT NOT NULL UNIQUE REFERENCES executions(id) DEFERRABLE INITIALLY DEFERRED,
 owner TEXT NOT NULL,entry TEXT NOT NULL REFERENCES entries(id),status TEXT NOT NULL CHECK(status IN('running','succeeded','failed','blocked','cancelled')),
 profile TEXT NOT NULL,diagnostic TEXT,work TEXT REFERENCES work_nodes(id),
 FOREIGN KEY(schedule,version) REFERENCES schedule_versions(schedule,version), UNIQUE(schedule,planned_utc));
ALTER TABLE executions ADD COLUMN source_occurrence INTEGER REFERENCES schedule_occurrences(id);
CREATE UNIQUE INDEX original_scheduled_execution ON executions(source_occurrence) WHERE source_occurrence IS NOT NULL;
CREATE TRIGGER forget_schedule_sources AFTER INSERT ON invalidated_sources BEGIN
 UPDATE schedule_versions SET goal=NULL,name=NULL WHERE (schedule,version) IN(SELECT schedule,version FROM schedule_sources WHERE event=NEW.source);
 UPDATE schedules SET status='blocked',next_due=NULL WHERE (id,version) IN(SELECT schedule,version FROM schedule_sources WHERE event=NEW.source);
 UPDATE schedule_occurrences SET status='blocked',diagnostic='source_erased' WHERE (schedule,version) IN(SELECT schedule,version FROM schedule_sources WHERE event=NEW.source) AND status='running';
END;

CREATE UNIQUE INDEX schedule_running ON schedule_occurrences(schedule) WHERE status='running';
CREATE TRIGGER count_settled_occurrence AFTER UPDATE OF status ON schedule_occurrences
WHEN OLD.status='running' AND NEW.status!='running' BEGIN
 UPDATE schedules SET run_count=run_count+1 WHERE id=NEW.schedule;
END;
