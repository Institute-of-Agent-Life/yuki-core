-- The receipt survives temporary upload expiry. The separate blob reference is
-- removed on expiry or forget; an independently authorized event can still own it.
CREATE TABLE client_uploads(
 id TEXT PRIMARY KEY, device TEXT NOT NULL REFERENCES personal_devices(id),
 owner TEXT NOT NULL REFERENCES persons(id), entry TEXT NOT NULL REFERENCES entries(id),
 digest TEXT REFERENCES media_objects(digest), expires INTEGER NOT NULL
);
CREATE INDEX client_upload_expiry ON client_uploads(expires) WHERE digest IS NOT NULL;
CREATE TABLE client_upload_parts(
 event INTEGER NOT NULL REFERENCES events(id),slot INTEGER NOT NULL, media TEXT NOT NULL REFERENCES client_uploads(id),
 name TEXT NOT NULL,PRIMARY KEY(event,slot),FOREIGN KEY(event,slot) REFERENCES media_sources(event,slot)
);
CREATE TRIGGER erase_client_attachment_name AFTER INSERT ON invalidated_sources BEGIN
 DELETE FROM client_upload_parts WHERE event=NEW.source;
END;

-- Both the polling reader and the collecting writer must agree about live ownership.
CREATE VIEW live_media_digests AS
 SELECT digest FROM media_sources WHERE digest IS NOT NULL
 UNION ALL SELECT digest FROM media_versions WHERE digest IS NOT NULL
 UNION ALL SELECT digest FROM media_reads WHERE digest IS NOT NULL
 UNION ALL SELECT digest FROM client_uploads WHERE digest IS NOT NULL;
