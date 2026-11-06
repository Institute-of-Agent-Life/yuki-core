-- Attribution changes create a separate versioned fact and preserve original authors.
CREATE TABLE memory_attributions (
 fact TEXT PRIMARY KEY REFERENCES memory_facts(id), authority TEXT NOT NULL CHECK(authority IN('explicit','third_party')),
 supersedes TEXT UNIQUE REFERENCES memory_facts(id), CHECK(supersedes IS NULL OR supersedes!=fact)
);
ALTER TABLE memory_evidence ADD COLUMN attribution TEXT CHECK(attribution IN('explicit','third_party'));
DROP VIEW memory_fact_states;
CREATE VIEW memory_fact_states AS SELECT f.id,
 CASE WHEN f.status='retracted' AND c.role='alternative' THEN 'contested'
      WHEN f.status='retracted' AND (m.source IS NOT NULL OR a.supersedes IS NOT NULL) THEN 'superseded' ELSE f.status END AS status,
 CASE WHEN (c.role='original' AND f.status='active') OR (c.role='alternative' AND f.status='retracted')
      THEN 'contested' ELSE 'clear' END AS conflict_state
 FROM memory_facts f LEFT JOIN memory_equivalences m ON m.source=f.id LEFT JOIN memory_conflict_flags c ON c.fact=f.id
 LEFT JOIN memory_attributions a ON a.supersedes=f.id;
