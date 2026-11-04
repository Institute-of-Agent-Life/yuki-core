CREATE TABLE core_meta (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0)
);
