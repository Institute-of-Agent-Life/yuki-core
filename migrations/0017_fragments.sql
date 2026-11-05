-- Reader 17. Frozen public fragments reference concrete history versions. Private paid
-- responses remain on the original execution and public adoption copies no private tail.
CREATE TABLE compact_jobs(id TEXT PRIMARY KEY,execution TEXT NOT NULL REFERENCES executions(id),entry TEXT NOT NULL REFERENCES entries(id),
 generation INTEGER NOT NULL,prior_checkpoint TEXT REFERENCES compact_jobs(id),source_hash TEXT NOT NULL,profile TEXT NOT NULL,
 coverage INTEGER NOT NULL,last_item INTEGER NOT NULL,logical TEXT NOT NULL UNIQUE,focus TEXT,
 state TEXT NOT NULL CHECK(state IN('pending','published','rejected','invalid')),summary TEXT,diagnostic TEXT,
 UNIQUE(execution,source_hash));
CREATE TABLE compact_fragments(job TEXT NOT NULL REFERENCES compact_jobs(id),ordinal INTEGER NOT NULL,item INTEGER NOT NULL REFERENCES history_items(id),
 revision INTEGER NOT NULL,source_key TEXT NOT NULL,source_hash TEXT NOT NULL,message TEXT,PRIMARY KEY(job,ordinal),UNIQUE(job,item));
CREATE INDEX compact_fragment_source ON compact_fragments(item,job);
CREATE INDEX compact_entry_head ON compact_jobs(entry,generation,last_item);
CREATE TABLE compact_views(execution TEXT PRIMARY KEY REFERENCES executions(id),entry TEXT NOT NULL REFERENCES entries(id),generation INTEGER NOT NULL,
 checkpoint TEXT REFERENCES compact_jobs(id),static_program TEXT NOT NULL);
CREATE TABLE compact_view_items(execution TEXT NOT NULL REFERENCES compact_views(execution),ordinal INTEGER NOT NULL,item INTEGER NOT NULL REFERENCES history_items(id),
 revision INTEGER NOT NULL,source_key TEXT NOT NULL,source_hash TEXT NOT NULL,message TEXT,PRIMARY KEY(execution,ordinal));
CREATE TABLE compact_manual(event INTEGER PRIMARY KEY REFERENCES events(id),execution TEXT NOT NULL UNIQUE REFERENCES executions(id),focus TEXT);
CREATE TRIGGER erase_public_compact AFTER INSERT ON invalidated_sources BEGIN
 UPDATE compact_jobs SET state='invalid',summary=NULL,focus=NULL WHERE id IN(
 WITH RECURSIVE invalid(id) AS(
 SELECT j.id FROM compact_jobs j WHERE j.execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source)
 UNION SELECT f.job FROM compact_fragments f JOIN history_items h ON h.id=f.item LEFT JOIN social_outbound o ON o.id=h.outbound
 WHERE h.source_event=NEW.source OR o.namespace IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source)
 UNION SELECT j.id FROM compact_jobs j JOIN invalid i ON j.prior_checkpoint=i.id) SELECT id FROM invalid);
 UPDATE compact_fragments SET message=NULL WHERE job IN(SELECT id FROM compact_jobs WHERE state='invalid');
 UPDATE compact_view_items SET message=NULL WHERE item IN(SELECT h.id FROM history_items h LEFT JOIN social_outbound o ON o.id=h.outbound WHERE h.source_event=NEW.source OR o.namespace IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source));
 UPDATE compact_manual SET focus=NULL WHERE event=NEW.source;
END;
CREATE UNIQUE INDEX one_public_candidate ON compact_jobs(entry,generation,source_hash);
CREATE TRIGGER reset_compact_window AFTER UPDATE OF generation ON main_windows WHEN NEW.generation!=OLD.generation BEGIN
 UPDATE compact_jobs SET state='invalid',summary=NULL,diagnostic='window_reset' WHERE entry=NEW.conversation AND generation=OLD.generation AND state IN('pending','published');
END;

-- Close dependent checkpoints during the same writer transaction that changes a source.
CREATE TRIGGER compact_source_changed AFTER UPDATE OF revision,visible ON history_items
WHEN OLD.revision!=NEW.revision OR OLD.visible!=NEW.visible BEGIN
 UPDATE compact_jobs SET state='invalid',summary=NULL,diagnostic='source_version_changed' WHERE id IN(
  WITH RECURSIVE lost(id) AS(
    SELECT job FROM compact_fragments WHERE item=NEW.id
    UNION SELECT j.id FROM compact_jobs j JOIN lost ON j.prior_checkpoint=lost.id)
  SELECT id FROM lost);
 UPDATE compact_fragments SET message=NULL WHERE item=NEW.id;
 UPDATE compact_view_items SET message=NULL WHERE item=NEW.id;
END;
