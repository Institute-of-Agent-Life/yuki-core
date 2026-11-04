-- Reader 10. Original operation, route, ordinal and receiving bot are immutable.
CREATE TABLE social_plans(
   namespace TEXT NOT NULL,operation_id TEXT NOT NULL,actor TEXT NOT NULL,entry TEXT NOT NULL REFERENCES entries(id),
   stage TEXT NOT NULL REFERENCES assignments(id),recipient_stage TEXT REFERENCES assignments(id),native_target TEXT NOT NULL,
   scope TEXT NOT NULL CHECK(scope IN('private','group')),window INTEGER NOT NULL,route_revision INTEGER,
   PRIMARY KEY(namespace,operation_id),FOREIGN KEY(namespace,operation_id) REFERENCES operations(namespace,operation_id)
);
CREATE TABLE social_parts(
   namespace TEXT NOT NULL,operation_id TEXT NOT NULL,ordinal INTEGER NOT NULL,child TEXT NOT NULL,wire_action TEXT NOT NULL,
   params TEXT NOT NULL,state TEXT NOT NULL CHECK(state IN('prepared','dispatched','confirmed','failed','unknown')),
   session INTEGER, locator TEXT,confirmed_event INTEGER REFERENCES events(id),error TEXT,
   PRIMARY KEY(namespace,operation_id,ordinal),FOREIGN KEY(namespace,operation_id) REFERENCES social_plans(namespace,operation_id),
   FOREIGN KEY(namespace,child) REFERENCES operations(namespace,operation_id)
);
CREATE TABLE social_image_selection(singleton INTEGER PRIMARY KEY CHECK(singleton=1),owner TEXT NOT NULL REFERENCES source_owners(id),digest TEXT NOT NULL);
CREATE TABLE social_images(
   namespace TEXT NOT NULL,operation_id TEXT NOT NULL,child TEXT NOT NULL,goal TEXT NOT NULL,emotion TEXT NOT NULL,
   actor TEXT NOT NULL,stage TEXT NOT NULL REFERENCES assignments(id),entry TEXT NOT NULL REFERENCES entries(id),
   window INTEGER NOT NULL,owner TEXT NOT NULL REFERENCES source_owners(id),digest TEXT NOT NULL,state TEXT NOT NULL DEFAULT 'pending',PRIMARY KEY(namespace,operation_id),
   FOREIGN KEY(namespace,operation_id) REFERENCES social_plans(namespace,operation_id),
   FOREIGN KEY(namespace,child) REFERENCES operations(namespace,operation_id)
);
CREATE TABLE social_outbound(
   id INTEGER PRIMARY KEY AUTOINCREMENT,stage TEXT NOT NULL REFERENCES assignments(id),entry TEXT NOT NULL REFERENCES entries(id),locator TEXT,
   namespace TEXT NOT NULL,child TEXT NOT NULL,original_actor TEXT NOT NULL,body TEXT NOT NULL,
   echo_event INTEGER REFERENCES events(id),recalled INTEGER NOT NULL DEFAULT 0,UNIQUE(stage,entry,locator),
   UNIQUE(namespace,child),FOREIGN KEY(namespace,child) REFERENCES operations(namespace,operation_id)
);
CREATE TABLE social_recalls(
   namespace TEXT NOT NULL,child TEXT NOT NULL,session INTEGER NOT NULL,state TEXT NOT NULL CHECK(state IN('dispatched','confirmed','failed','unknown')),
   PRIMARY KEY(namespace,child),FOREIGN KEY(namespace,child) REFERENCES operations(namespace,operation_id)
);
CREATE TABLE qq_social_names(entry TEXT NOT NULL REFERENCES entries(id),source INTEGER NOT NULL REFERENCES events(id),
 account TEXT NOT NULL REFERENCES accounts(id),name TEXT NOT NULL,occurred INTEGER NOT NULL,PRIMARY KEY(source,name));
CREATE INDEX qq_name_lookup ON qq_social_names(entry,name,occurred DESC,source DESC);
CREATE INDEX qq_token_lookup ON qq_social_names(entry,account,occurred DESC,source DESC);
INSERT OR IGNORE INTO qq_social_names SELECT e.entry_id,e.id,e.account_id,coalesce(j.value,''),coalesce(e.occurred_at,e.observed_at) FROM events e
 JOIN accounts a ON a.id=e.account_id JOIN json_each(json_array(json_extract(e.envelope,'$.sender.card'),json_extract(e.envelope,'$.sender.nickname'),'')) j
 WHERE a.platform='qq' AND e.entry_id IS NOT NULL AND json_extract(e.envelope,'$.kind')='message'
 AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id);
CREATE TRIGGER snapshot_qq_names AFTER INSERT ON events WHEN NEW.entry_id IS NOT NULL AND json_extract(NEW.envelope,'$.kind')='message'
 AND (SELECT platform FROM accounts WHERE id=NEW.account_id)='qq' BEGIN
 INSERT OR IGNORE INTO qq_social_names SELECT NEW.entry_id,NEW.id,NEW.account_id,coalesce(value,''),coalesce(NEW.occurred_at,NEW.observed_at)
 FROM json_each(json_array(json_extract(NEW.envelope,'$.sender.card'),json_extract(NEW.envelope,'$.sender.nickname'),''));
END;
CREATE TABLE social_media_bindings(id TEXT PRIMARY KEY NOT NULL,execution TEXT NOT NULL REFERENCES executions(id),kind TEXT NOT NULL CHECK(kind IN('file','image')),filename TEXT NOT NULL,peer_reference TEXT NOT NULL);
CREATE TRIGGER link_acknowledged_echo AFTER INSERT ON event_locators WHEN EXISTS(SELECT 1 FROM events WHERE id=NEW.event_id AND json_extract(envelope,'$.kind')='echo') BEGIN
 UPDATE social_outbound SET echo_event=NEW.event_id WHERE stage=NEW.receiver AND locator=NEW.native_id
 AND entry=(SELECT entry_id FROM events WHERE id=NEW.event_id) AND echo_event IS NULL;
 UPDATE events SET envelope='{}',mentions='[]' WHERE id=NEW.event_id AND EXISTS(
   SELECT 1 FROM social_outbound WHERE echo_event=NEW.event_id AND body='{}');
END;
CREATE TABLE social_part_receipts(
 namespace TEXT NOT NULL,child TEXT NOT NULL,state TEXT NOT NULL,locator TEXT,observed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
 PRIMARY KEY(namespace,child,state),FOREIGN KEY(namespace,child) REFERENCES operations(namespace,operation_id));
