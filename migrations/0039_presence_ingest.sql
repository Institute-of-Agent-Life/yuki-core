-- A receiving presence can remain routable while ingestion is explicitly paused.
ALTER TABLE assignments ADD COLUMN ingest_eligible INTEGER NOT NULL DEFAULT 1 CHECK(ingest_eligible IN (0,1));
