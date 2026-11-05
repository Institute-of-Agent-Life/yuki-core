-- Reader 25. A Work continuation can reuse only the original Work's bytes and root.
CREATE VIEW media_version_readers(version,reader,producer) AS
 SELECT v.id,v.execution,v.execution FROM media_versions v WHERE v.digest IS NOT NULL
 UNION ALL
 SELECT v.id,c.execution,v.execution FROM media_versions v JOIN work_runs p ON p.execution=v.execution
 JOIN work_runs c ON c.work=p.work AND c.execution!=p.execution JOIN work_nodes w ON w.id=p.work
 JOIN budget_members a ON a.execution=p.execution AND a.root=w.budget
 JOIN budget_members b ON b.execution=c.execution AND b.root=w.budget
 WHERE v.digest IS NOT NULL AND w.goal IS NOT NULL
 AND NOT EXISTS(SELECT 1 FROM work_sources s JOIN invalidated_sources i ON i.source=s.event WHERE s.work=w.id);

CREATE TABLE social_part_versions(namespace TEXT NOT NULL,child TEXT NOT NULL,version TEXT NOT NULL REFERENCES media_versions(id),
 PRIMARY KEY(namespace,child),FOREIGN KEY(namespace,child) REFERENCES operations(namespace,operation_id));

-- Earlier send plans saved the parent request and exact action parameters. Attach an
-- earlier part only when both identify the same immutable file/image binding.
INSERT INTO social_part_versions SELECT p.namespace,p.child,v.id FROM social_parts p
 JOIN operations o ON o.namespace=p.namespace AND o.operation_id=p.operation_id
 JOIN media_versions v ON v.id=json_extract(o.payload,'$.artifact_id') AND v.execution=p.namespace AND v.digest IS NOT NULL
 JOIN social_media_bindings b ON b.id=v.id AND b.execution=v.execution
 WHERE (p.wire_action IN('upload_group_file','upload_private_file') AND json_extract(p.params,'$.file')=b.peer_reference)
 OR (p.wire_action IN('send_group_msg','send_private_msg') AND EXISTS(SELECT 1 FROM json_each(p.params,'$.message') m
 WHERE json_extract(m.value,'$.type')='image' AND json_extract(m.value,'$.data.file')=b.peer_reference));

-- Use the original echo when available at admission. Without one, publish a local
-- delivery event from the confirmed ACK. Late echoes never renumber a published event.
CREATE TABLE personal_deliveries(outbound INTEGER PRIMARY KEY REFERENCES social_outbound(id),event INTEGER NOT NULL UNIQUE REFERENCES events(id));
INSERT INTO events(source_scope,source_key,entry_id,account_id,bot_stage,observed_at,envelope,mentions)
SELECT 'core.personal.delivery.1',cast(o.id AS TEXT),o.entry,b.account_id,b.id,o.observed_at,'{"kind":"delivery"}','[]'
FROM social_outbound o JOIN assignments b ON b.id=o.stage
WHERE o.echo_event IS NULL AND o.observed_at IS NOT NULL AND NOT o.recalled AND o.body!='{}';
INSERT INTO personal_deliveries SELECT o.id,coalesce(o.echo_event,e.id) FROM social_outbound o LEFT JOIN events e
 ON e.source_scope='core.personal.delivery.1' AND e.source_key=cast(o.id AS TEXT)
 WHERE coalesce(o.echo_event,e.id) IS NOT NULL AND NOT o.recalled AND o.body!='{}';
CREATE TRIGGER personal_delivery_ack AFTER INSERT ON social_outbound WHEN NEW.body!='{}' AND NOT NEW.recalled BEGIN
 INSERT INTO events(source_scope,source_key,entry_id,account_id,bot_stage,observed_at,envelope,mentions)
 SELECT 'core.personal.delivery.1',cast(NEW.id AS TEXT),NEW.entry,b.account_id,b.id,NEW.observed_at,'{"kind":"delivery"}','[]'
 FROM assignments b WHERE b.id=NEW.stage AND NEW.echo_event IS NULL AND NEW.observed_at IS NOT NULL;
 INSERT INTO personal_deliveries SELECT NEW.id,coalesce(NEW.echo_event,e.id) FROM (SELECT 1) LEFT JOIN events e
 ON e.source_scope='core.personal.delivery.1' AND e.source_key=cast(NEW.id AS TEXT) WHERE coalesce(NEW.echo_event,e.id) IS NOT NULL;
END;
CREATE VIEW personal_live_deliveries AS
 SELECT o.*,d.event FROM social_outbound o JOIN personal_deliveries d ON d.outbound=o.id JOIN executions x ON x.id=o.namespace
 WHERE NOT o.recalled AND o.body!='{}' AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source IN(d.event,o.echo_event,x.source)
 OR i.source IN(SELECT event FROM main_dependencies WHERE execution=x.id));

CREATE VIEW personal_artifact_events AS
 SELECT d.event,d.entry,d.observed_at,d.namespace,r.work,v.id,v.filename,v.mime,v.digest,v.size,v.kind
 FROM personal_live_deliveries d JOIN social_part_versions s ON s.namespace=d.namespace AND s.child=d.child
 JOIN media_versions v ON v.id=s.version AND v.digest IS NOT NULL LEFT JOIN work_runs r ON r.execution=d.namespace;
CREATE VIEW personal_artifact_references AS SELECT a.*,
 coalesce((SELECT CAST(j.key AS INTEGER) FROM json_each(d.body,'$.message') j
           WHERE json_extract(j.value,'$.type')='image' LIMIT 1),0) AS part_index
 FROM personal_artifact_events a JOIN personal_live_deliveries d ON d.event=a.event;
