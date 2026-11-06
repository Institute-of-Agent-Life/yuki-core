-- Reader 29. Scheduling receipts contain IDs and counters, without private content.
CREATE TABLE main_segments (
 execution TEXT NOT NULL REFERENCES executions(id), ordinal INTEGER NOT NULL CHECK(ordinal>=0),
 first_request_count INTEGER NOT NULL, request_limit INTEGER NOT NULL CHECK(request_limit>0),
 state TEXT NOT NULL CHECK(state IN('active','yielded')), request_count INTEGER,
 started_at INTEGER NOT NULL, yielded_at INTEGER, PRIMARY KEY(execution,ordinal)
);
CREATE UNIQUE INDEX one_active_main_segment ON main_segments(execution) WHERE state='active';
