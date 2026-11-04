-- Identity reader version 2. A NULL current_root identifies a root.
-- Original IDs and device origins are immutable across merges.
CREATE TABLE persons (
    id TEXT PRIMARY KEY NOT NULL
        CHECK (length(id) = 36 AND substr(id, 9, 1) = '-' AND substr(id, 14, 2) = '-4'
               AND substr(id, 19, 1) = '-' AND substr(id, 20, 1) IN ('8','9','a','b')
               AND substr(id, 24, 1) = '-' AND length(replace(id, '-', '')) = 32
               AND replace(id, '-', '') NOT GLOB '*[^0-9a-f]*'),
    current_root TEXT REFERENCES persons(id) ON UPDATE RESTRICT ON DELETE RESTRICT
        CHECK (current_root IS NULL OR current_root <> id),
    name TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    admin INTEGER NOT NULL DEFAULT 0 CHECK (admin IN (0,1)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0)
);
CREATE INDEX persons_by_root ON persons(current_root);
CREATE TABLE device_origins (
    device_id TEXT PRIMARY KEY NOT NULL CHECK (length(device_id) BETWEEN 1 AND 128),
    original_person TEXT NOT NULL REFERENCES persons(id) ON UPDATE RESTRICT ON DELETE RESTRICT
);
CREATE INDEX device_origins_person ON device_origins(original_person);
