-- Reader 43. Each model recall refers to the original sent part and native receipt.
CREATE TABLE model_recalls(
 execution TEXT NOT NULL,operation TEXT NOT NULL,event INTEGER NOT NULL REFERENCES events(id),
 original_execution TEXT NOT NULL,child TEXT NOT NULL,presence TEXT NOT NULL REFERENCES assignments(id),
 target_kind TEXT NOT NULL,target_id TEXT NOT NULL,
 PRIMARY KEY(execution,operation),
 FOREIGN KEY(execution,operation) REFERENCES operations(namespace,operation_id),
 FOREIGN KEY(original_execution,child) REFERENCES social_recalls(namespace,child));
CREATE INDEX model_recall_receipts ON model_recalls(original_execution,child);
