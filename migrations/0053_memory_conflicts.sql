-- A disputed alternative is inactive in the legacy table. Readers that understand
-- this migration distinguish it from invalidated memory and cannot restore it.
CREATE TABLE memory_conflict_flags (
 fact TEXT PRIMARY KEY REFERENCES memory_facts(id), role TEXT NOT NULL CHECK(role IN('original','alternative'))
);
CREATE TABLE memory_conflict_actions (
 execution TEXT NOT NULL REFERENCES executions(id), operation TEXT NOT NULL, original TEXT NOT NULL REFERENCES memory_facts(id),
 alternative TEXT UNIQUE REFERENCES memory_facts(id), original_revision INTEGER NOT NULL,
 CHECK(alternative IS NULL OR alternative!=original), PRIMARY KEY(execution,operation)
);
CREATE VIEW memory_contradictions AS SELECT alternative AS source,original AS target,'contradicts' AS relation_type
 FROM memory_conflict_actions WHERE alternative IS NOT NULL;
DROP VIEW memory_fact_states;
CREATE VIEW memory_fact_states AS SELECT f.id,
 CASE WHEN f.status='retracted' AND c.role='alternative' THEN 'contested'
      WHEN f.status='retracted' AND m.source IS NOT NULL THEN 'superseded' ELSE f.status END AS status,
 CASE WHEN (c.role='original' AND f.status='active') OR (c.role='alternative' AND f.status='retracted')
      THEN 'contested' ELSE 'clear' END AS conflict_state
 FROM memory_facts f LEFT JOIN memory_equivalences m ON m.source=f.id LEFT JOIN memory_conflict_flags c ON c.fact=f.id;
