-- Reader 27. Byte references belong to the original delegated call, not its file path.
CREATE TABLE host_media_selected(handle TEXT PRIMARY KEY REFERENCES host_calls(handle),version TEXT NOT NULL,
 digest TEXT NOT NULL,mime TEXT NOT NULL,purpose TEXT NOT NULL,source INTEGER REFERENCES events(id),segment INTEGER);
CREATE TABLE host_media_uploads(handle TEXT NOT NULL REFERENCES host_calls(handle),name TEXT NOT NULL,
 filename TEXT NOT NULL,kind TEXT NOT NULL,mime TEXT NOT NULL,digest TEXT NOT NULL,size INTEGER NOT NULL CHECK(size BETWEEN 0 AND 16777216),
 next_offset INTEGER NOT NULL DEFAULT 0,version TEXT REFERENCES media_versions(id),PRIMARY KEY(handle,name));
CREATE TABLE host_media_chunks(handle TEXT NOT NULL,name TEXT NOT NULL,offset INTEGER NOT NULL,bytes BLOB NOT NULL,
 PRIMARY KEY(handle,name,offset),FOREIGN KEY(handle,name) REFERENCES host_media_uploads(handle,name));
CREATE TABLE host_media_profiles(handle TEXT PRIMARY KEY REFERENCES host_calls(handle),vision INTEGER NOT NULL CHECK(vision IN(0,1)));
CREATE TABLE host_media_presentations(handle TEXT NOT NULL REFERENCES host_calls(handle),ordinal INTEGER NOT NULL,
 version TEXT REFERENCES media_versions(id),status TEXT NOT NULL,PRIMARY KEY(handle,ordinal));
CREATE TRIGGER erase_host_media_refs AFTER INSERT ON invalidated_sources BEGIN
 DELETE FROM host_media_presentations WHERE handle IN(SELECT h.handle FROM host_calls h WHERE h.execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source));
 DELETE FROM host_media_chunks WHERE handle IN(SELECT h.handle FROM host_calls h WHERE h.execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source));
 DELETE FROM host_media_uploads WHERE handle IN(SELECT h.handle FROM host_calls h WHERE h.execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source));
 DELETE FROM host_media_selected WHERE source=NEW.source OR handle IN(SELECT h.handle FROM host_calls h WHERE h.execution IN(SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source));
END;
