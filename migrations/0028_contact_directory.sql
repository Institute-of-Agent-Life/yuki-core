-- Reader 28. Canonical space and transport binding IDs survive topic and process changes.
CREATE TABLE contact_spaces(id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)),2) || '-' || substr('89ab',1+(random() & 3),1) || substr(hex(randomblob(2)),2) || '-' || hex(randomblob(6)))),space_key TEXT UNIQUE NOT NULL);
CREATE TABLE contact_space_bindings(id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)),2) || '-' || substr('89ab',1+(random() & 3),1) || substr(hex(randomblob(2)),2) || '-' || hex(randomblob(6)))),subject TEXT NOT NULL REFERENCES contact_spaces(id),
 platform TEXT NOT NULL,namespace TEXT NOT NULL,native TEXT NOT NULL,UNIQUE(platform,namespace,native));
INSERT OR IGNORE INTO contact_spaces(id,space_key) SELECT DISTINCT lower(space),space FROM entries WHERE scope='group' AND length(space)=36 AND substr(space,9,1)='-' AND substr(space,14,1)='-' AND substr(space,19,1)='-' AND substr(space,24,1)='-' AND length(replace(space,'-',''))=32 AND lower(replace(space,'-','')) NOT GLOB '*[^0-9a-f]*' ORDER BY space;
INSERT OR IGNORE INTO contact_spaces(space_key) SELECT DISTINCT space FROM entries WHERE scope='group' AND NOT EXISTS(SELECT 1 FROM contact_spaces s WHERE s.id=lower(entries.space)) ORDER BY space;
CREATE TRIGGER directory_group_insert AFTER INSERT ON entries WHEN NEW.scope='group' BEGIN
 INSERT OR IGNORE INTO contact_spaces(id,space_key) SELECT lower(NEW.space),NEW.space WHERE length(NEW.space)=36 AND substr(NEW.space,9,1)='-' AND substr(NEW.space,14,1)='-' AND substr(NEW.space,19,1)='-' AND substr(NEW.space,24,1)='-' AND length(replace(NEW.space,'-',''))=32 AND lower(replace(NEW.space,'-','')) NOT GLOB '*[^0-9a-f]*';
 INSERT OR IGNORE INTO contact_spaces(space_key) SELECT NEW.space WHERE NOT EXISTS(SELECT 1 FROM contact_spaces WHERE id=lower(NEW.space));
END;
-- A binding requires an original received platform event; a guessed name or bare group key is insufficient.
CREATE VIEW observed_space_bindings AS SELECT DISTINCT s.id AS subject,a.platform,a.namespace,json_extract(d.space,'$[2]') AS native
 FROM entries d JOIN contact_spaces s ON (s.space_key=d.space OR s.id=lower(d.space)) JOIN events e ON e.entry_id=d.id JOIN accounts a ON a.id=e.account_id
 WHERE d.scope='group' AND CASE WHEN json_valid(d.space) THEN json_type(d.space)='array' AND json_array_length(d.space)=3
 AND json_extract(d.space,'$[0]')=a.platform AND json_extract(d.space,'$[1]')=a.namespace ELSE 0 END
 AND json_extract(e.envelope,'$.kind')='message' AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id);
INSERT OR IGNORE INTO contact_space_bindings(subject,platform,namespace,native) SELECT subject,platform,namespace,native FROM observed_space_bindings;
CREATE TRIGGER directory_group_observation AFTER INSERT ON events WHEN NEW.entry_id IS NOT NULL AND json_extract(NEW.envelope,'$.kind')='message' BEGIN
 INSERT OR IGNORE INTO contact_space_bindings(subject,platform,namespace,native)
 SELECT s.id,a.platform,a.namespace,json_extract(d.space,'$[2]') FROM entries d JOIN contact_spaces s ON (s.space_key=d.space OR s.id=lower(d.space)) JOIN accounts a ON a.id=NEW.account_id
 WHERE d.id=NEW.entry_id AND CASE WHEN json_valid(d.space) THEN json_type(d.space)='array' AND json_array_length(d.space)=3
 AND json_extract(d.space,'$[0]')=a.platform AND json_extract(d.space,'$[1]')=a.namespace ELSE 0 END;
END;
