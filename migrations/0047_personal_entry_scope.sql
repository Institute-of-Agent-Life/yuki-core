-- Previous handles/operations were always admitted through their original device entry.
ALTER TABLE personal_proofs ADD COLUMN selected_entry TEXT REFERENCES entries(id);
ALTER TABLE personal_sessions ADD COLUMN selected_entry TEXT REFERENCES entries(id);
ALTER TABLE personal_native_operations ADD COLUMN selected_entry TEXT REFERENCES entries(id);
CREATE INDEX personal_code_replay ON personal_codes(owner,operation);
