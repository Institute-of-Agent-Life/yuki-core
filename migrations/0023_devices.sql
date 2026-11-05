-- Reader 23. Immutable device origins are shared with reader 2. A connector
-- never creates a conversation. Device-instance inventories remain with the connector.
CREATE TABLE personal_codes(id TEXT PRIMARY KEY,issuer TEXT NOT NULL,operation TEXT NOT NULL,owner TEXT NOT NULL REFERENCES persons(id),
 purpose TEXT NOT NULL CHECK(purpose IN('chat','connector')),source_stage TEXT REFERENCES assignments(id),code_hash TEXT UNIQUE NOT NULL,
 expires INTEGER NOT NULL,used INTEGER,UNIQUE(issuer,operation));
CREATE TABLE personal_devices(id TEXT PRIMARY KEY REFERENCES device_origins(device_id),token_hash TEXT NOT NULL UNIQUE,
 purpose TEXT NOT NULL CHECK(purpose IN('chat','connector')),label TEXT NOT NULL,created INTEGER NOT NULL,revoked INTEGER,
 entry TEXT REFERENCES entries(id),CHECK((purpose='chat')=(entry IS NOT NULL)));
CREATE TABLE personal_receipts(owner TEXT NOT NULL REFERENCES persons(id),scope TEXT NOT NULL,operation TEXT NOT NULL,
 class TEXT NOT NULL,target TEXT,digest TEXT NOT NULL,result TEXT NOT NULL,event INTEGER REFERENCES events(id),PRIMARY KEY(owner,scope,operation));
CREATE TABLE personal_work_revisions(work TEXT PRIMARY KEY REFERENCES work_nodes(id),revision INTEGER NOT NULL);
INSERT INTO personal_work_revisions SELECT id,1 FROM work_nodes;
CREATE TRIGGER personal_work_revision AFTER INSERT ON work_nodes BEGIN INSERT INTO personal_work_revisions VALUES(NEW.id,1);END;
CREATE TRIGGER personal_work_update AFTER UPDATE OF status,note,goal,summary ON work_nodes
 WHEN NEW.status!=OLD.status OR NEW.note IS NOT OLD.note OR NEW.goal IS NOT OLD.goal OR NEW.summary IS NOT OLD.summary BEGIN
 UPDATE personal_work_revisions SET revision=revision+1 WHERE work=NEW.id;END;
CREATE TABLE personal_proofs(hash TEXT PRIMARY KEY,device TEXT NOT NULL REFERENCES personal_devices(id),plugin TEXT NOT NULL REFERENCES host_installations(plugin),
 generation TEXT NOT NULL,scope TEXT NOT NULL,work TEXT REFERENCES work_nodes(id),expires INTEGER NOT NULL,used INTEGER);
CREATE TABLE personal_sessions(hash TEXT PRIMARY KEY,device TEXT NOT NULL REFERENCES personal_devices(id),plugin TEXT NOT NULL REFERENCES host_installations(plugin),
 generation TEXT NOT NULL,scope TEXT NOT NULL,work TEXT REFERENCES work_nodes(id),expires INTEGER NOT NULL);
CREATE TABLE native_receipts(device TEXT NOT NULL REFERENCES personal_devices(id),plugin TEXT NOT NULL REFERENCES host_installations(plugin),
 operation TEXT NOT NULL,event_key TEXT NOT NULL,fingerprint TEXT NOT NULL,event INTEGER REFERENCES events(id),work TEXT REFERENCES work_nodes(id),watermark INTEGER NOT NULL,
 PRIMARY KEY(device,plugin,operation,event_key));
CREATE TRIGGER forget_personal_source AFTER INSERT ON invalidated_sources BEGIN
 UPDATE personal_receipts SET result='{}' WHERE event=NEW.source;
 DELETE FROM personal_sessions WHERE work IN(SELECT work FROM work_sources WHERE event=NEW.source);
 DELETE FROM personal_proofs WHERE work IN(SELECT work FROM work_sources WHERE event=NEW.source);
END;
-- Core keeps only authority and delivery keys. The plugin owns run details.
CREATE TABLE personal_native_operations(device TEXT NOT NULL REFERENCES personal_devices(id),operation TEXT NOT NULL,work TEXT NOT NULL REFERENCES work_nodes(id),
 chat_device TEXT NOT NULL REFERENCES personal_devices(id),PRIMARY KEY(device,operation));
