-- Reader 9. Keep the original execution IDs, their children and effect receipts.
-- The deferred references are revalidated before SQLx commits this migration.
CREATE TABLE main_origins (
    ref TEXT PRIMARY KEY NOT NULL CHECK(length(ref)>0),
    kind TEXT NOT NULL CHECK(kind IN ('plugin','self','schedule','work','child')),
    conversation TEXT NOT NULL REFERENCES entries(id), parent TEXT REFERENCES executions(id),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1))
);
PRAGMA defer_foreign_keys=ON;
CREATE TABLE expanded_executions (
    id TEXT PRIMARY KEY NOT NULL, source INTEGER UNIQUE REFERENCES accepted_sources(event_id),
    original_actor TEXT REFERENCES persons(id), state TEXT NOT NULL CHECK(state IN ('pending','running','completed','held')),
    cancelled INTEGER NOT NULL DEFAULT 0 CHECK(cancelled IN(0,1)), work_id TEXT,
    origin TEXT REFERENCES main_origins(ref),
    CHECK((source IS NOT NULL AND original_actor IS NOT NULL AND origin IS NULL)
      OR (source IS NULL AND original_actor IS NULL AND origin IS NOT NULL))
);
INSERT INTO expanded_executions(id,source,original_actor,state,cancelled,work_id)
 SELECT id,source,original_actor,state,cancelled,work_id FROM executions;
DROP TABLE executions;
ALTER TABLE expanded_executions RENAME TO executions;
CREATE TEMP TABLE validated_execution_refs(count INTEGER CHECK(count=0));
INSERT INTO validated_execution_refs SELECT count(*) FROM pragma_foreign_key_check;
DROP TABLE validated_execution_refs;
PRAGMA defer_foreign_keys=OFF;
CREATE TABLE main_windows(conversation TEXT PRIMARY KEY REFERENCES entries(id),generation INTEGER NOT NULL);
CREATE TABLE main_turns(
    execution TEXT PRIMARY KEY REFERENCES executions(id),conversation TEXT NOT NULL REFERENCES entries(id),
    window INTEGER NOT NULL, origin TEXT REFERENCES main_origins(ref), occurrence TEXT, material TEXT,
    ready_after INTEGER NOT NULL, result TEXT, diagnostic TEXT, UNIQUE(origin,occurrence)
);
CREATE INDEX main_by_window ON main_turns(conversation,window);
CREATE TABLE main_inputs(event INTEGER PRIMARY KEY REFERENCES events(id),execution TEXT NOT NULL REFERENCES main_turns(execution));
CREATE TABLE main_controls(event INTEGER PRIMARY KEY REFERENCES events(id),command TEXT NOT NULL,generation INTEGER NOT NULL);
ALTER TABLE main_inputs ADD COLUMN consumed INTEGER NOT NULL DEFAULT 0 CHECK(consumed IN(0,1));
CREATE TABLE main_dependencies(execution TEXT NOT NULL REFERENCES executions(id),event INTEGER NOT NULL REFERENCES events(id),PRIMARY KEY(execution,event));
CREATE INDEX main_dependents ON main_dependencies(event,execution);
CREATE TABLE internal_values(origin TEXT NOT NULL REFERENCES main_origins(ref),target TEXT NOT NULL,value TEXT NOT NULL,revision INTEGER NOT NULL,execution TEXT NOT NULL REFERENCES executions(id),PRIMARY KEY(origin,target));
ALTER TABLE local_effect_values ADD COLUMN source_execution TEXT REFERENCES executions(id);
UPDATE local_effect_values SET source_execution=(SELECT id FROM executions WHERE executions.source=local_effect_values.source);
ALTER TABLE model_attempts ADD COLUMN partial_bytes BLOB;
CREATE TABLE main_admission_delays(event INTEGER PRIMARY KEY REFERENCES accepted_sources(event_id),retry_after INTEGER NOT NULL,failures INTEGER NOT NULL CHECK(failures>0));
