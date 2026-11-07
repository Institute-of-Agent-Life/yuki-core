-- Record only the source relation observed for newly accepted current-group claims.
ALTER TABLE memory_evidence ADD COLUMN relation TEXT CHECK(relation='group_statement');
