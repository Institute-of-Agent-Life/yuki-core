-- Reader 13. Installation authority, a particular process generation and original effects are distinct.
CREATE TABLE host_installations(
 plugin TEXT PRIMARY KEY,namespace TEXT NOT NULL UNIQUE,platform TEXT,bot_stage TEXT,revision TEXT NOT NULL,
 grants TEXT NOT NULL,tools TEXT NOT NULL,enabled INTEGER NOT NULL CHECK(enabled IN(0,1)),generation TEXT,online INTEGER NOT NULL DEFAULT 0);
CREATE TABLE host_commands(path TEXT PRIMARY KEY,plugin TEXT NOT NULL REFERENCES host_installations(plugin));
CREATE TABLE host_command_events(event INTEGER PRIMARY KEY REFERENCES events(id),plugin TEXT NOT NULL REFERENCES host_installations(plugin),
 path TEXT NOT NULL,execution TEXT REFERENCES executions(id),generation TEXT,state TEXT NOT NULL CHECK(state IN('queued','unavailable','complete','unknown')));
CREATE TABLE host_ingress(plugin TEXT NOT NULL REFERENCES host_installations(plugin),source TEXT NOT NULL,event INTEGER NOT NULL REFERENCES events(id),
 accepted INTEGER NOT NULL,PRIMARY KEY(plugin,source));
CREATE TABLE host_calls(handle TEXT PRIMARY KEY,plugin TEXT NOT NULL REFERENCES host_installations(plugin),revision TEXT NOT NULL,generation TEXT NOT NULL,
 execution TEXT NOT NULL REFERENCES executions(id),operation TEXT NOT NULL,method TEXT NOT NULL,args TEXT NOT NULL,state TEXT NOT NULL
 CHECK(state IN('queued','not_dispatched','dispatched','confirmed','failed','unknown')),result TEXT,
 UNIQUE(execution,operation),FOREIGN KEY(execution,operation) REFERENCES operations(namespace,operation_id));
CREATE INDEX host_pending_calls ON host_calls(plugin,generation,state);
CREATE TRIGGER erase_host_material AFTER INSERT ON invalidated_sources BEGIN
 UPDATE host_calls SET args='{}',result=NULL WHERE execution IN
   (SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
END;
CREATE TABLE host_observations(plugin TEXT NOT NULL,generation TEXT NOT NULL,event INTEGER NOT NULL REFERENCES events(id),PRIMARY KEY(plugin,generation,event));
CREATE TRIGGER hint_plugin_observers AFTER INSERT ON events
WHEN EXISTS(SELECT 1 FROM host_installations i,json_each(i.grants) g WHERE i.online AND i.enabled AND g.value='observe.metadata') BEGIN
 INSERT INTO host_observations SELECT i.plugin,i.generation,NEW.id FROM host_installations i,json_each(i.grants) g WHERE i.online AND i.enabled AND g.value='observe.metadata';
 DELETE FROM host_observations WHERE event<=NEW.id-64;
END;
-- A late response from the original authenticated generation can be reconciled
-- without replacing the earlier "unknown" fact or publishing revoked material.
CREATE TABLE host_late_receipts(handle TEXT PRIMARY KEY REFERENCES host_calls(handle),
 observed_state TEXT NOT NULL,body TEXT);
CREATE TRIGGER redact_late_host_receipts AFTER INSERT ON invalidated_sources BEGIN
 UPDATE host_late_receipts SET body=NULL WHERE handle IN (SELECT handle FROM host_calls WHERE execution IN
 (SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source));
END;
ALTER TABLE host_command_events ADD COLUMN reply_state TEXT;
CREATE TRIGGER private_command_history AFTER INSERT ON host_command_events BEGIN
 UPDATE history_items SET visible=0,revision=revision+1 WHERE source_event=NEW.event;
END;
