-- Reader 52: preserve the source/target relation without rewriting old fact rows.
-- Reader 51 already omits the inactive source from recall; the minimum reader
-- barrier prevents it from interpreting a superseded source as restorable.
CREATE TABLE memory_equivalences (
 source TEXT PRIMARY KEY REFERENCES memory_facts(id), target TEXT NOT NULL REFERENCES memory_facts(id),
 source_revision INTEGER NOT NULL, target_revision INTEGER NOT NULL, execution TEXT NOT NULL REFERENCES executions(id),
 operation TEXT NOT NULL, CHECK(source!=target), UNIQUE(execution,operation)
);

CREATE VIEW memory_fact_states AS
 SELECT f.id,CASE WHEN f.status='retracted' AND m.source IS NOT NULL THEN 'superseded' ELSE f.status END AS status
 FROM memory_facts f LEFT JOIN memory_equivalences m ON m.source=f.id;
