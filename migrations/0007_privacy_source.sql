-- Schema/reader 7. Keep erased source IDs, operation identities and accounting, not source bodies.
CREATE TABLE source_owners(id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),enabled INTEGER NOT NULL CHECK(enabled IN(0,1)));
CREATE TABLE source_holds(
    owner TEXT NOT NULL REFERENCES source_owners(id), item TEXT NOT NULL CHECK(length(item)>0),
    source INTEGER NOT NULL REFERENCES events(id), PRIMARY KEY(owner,item,source)
);
CREATE INDEX holds_by_source ON source_holds(source,owner);
CREATE TABLE source_erasure_requests(
    watermark INTEGER PRIMARY KEY AUTOINCREMENT, erase_key TEXT UNIQUE NOT NULL CHECK(length(erase_key)>0),
    subject TEXT NOT NULL REFERENCES persons(id), sources TEXT NOT NULL
);
CREATE TABLE invalidated_sources(
    source INTEGER PRIMARY KEY REFERENCES events(id), subject TEXT NOT NULL REFERENCES persons(id),
    watermark INTEGER NOT NULL REFERENCES source_erasure_requests(watermark)
);
CREATE TABLE owner_erasures(
    watermark INTEGER NOT NULL REFERENCES source_erasure_requests(watermark),
    owner TEXT NOT NULL REFERENCES source_owners(id), sources TEXT NOT NULL, proof TEXT,
    PRIMARY KEY(owner,watermark)
);
ALTER TABLE local_effect_values ADD COLUMN source INTEGER REFERENCES events(id);
-- Prior readers wrote an immutable revision in each put receipt. Only assign a unique match.
UPDATE local_effect_values AS v SET source=(
  SELECT CASE WHEN count(DISTINCT x.source)=1 THEN min(x.source) END
  FROM operations o JOIN executions x ON x.id=o.namespace
  WHERE x.original_actor=v.original_actor AND o.action='put' AND o.state='confirmed' AND o.target=v.target
    AND json_extract(o.result,'$.revision')=v.revision);

-- A read can snapshot a value from another event of the same original actor.
ALTER TABLE operations ADD COLUMN value_source INTEGER REFERENCES events(id);
UPDATE operations AS g SET value_source=(
  SELECT CASE WHEN count(DISTINCT x.source)=1 THEN min(x.source) END
  FROM operations o JOIN executions x ON x.id=o.namespace
  WHERE x.original_actor=(SELECT original_actor FROM executions WHERE id=g.namespace)
    AND o.action='put' AND o.state='confirmed' AND o.target=g.target
    AND json_extract(o.result,'$.revision')=json_extract(g.result,'$.revision'))
WHERE g.action='get' AND g.state='confirmed';
