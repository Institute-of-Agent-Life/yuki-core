-- Reader 35. A publisher owns an immutable external event and its original effects.
CREATE TABLE host_notification_policies(
    plugin TEXT NOT NULL,revision TEXT NOT NULL,body TEXT NOT NULL,
    PRIMARY KEY(plugin,revision)
);
CREATE TABLE notification_publishers(
    plugin TEXT PRIMARY KEY,account TEXT NOT NULL UNIQUE REFERENCES accounts(id)
);
-- A dynamic revoke overrides the operator-installed target until an administrator grants it again.
CREATE TABLE notification_target_grants(
    plugin TEXT NOT NULL,namespace TEXT NOT NULL,entry TEXT NOT NULL REFERENCES entries(id),
    receiver TEXT NOT NULL REFERENCES assignments(id),created_by TEXT NOT NULL REFERENCES persons(id),
    source_call TEXT NOT NULL REFERENCES host_calls(handle),enabled INTEGER NOT NULL CHECK(enabled IN(0,1)),
    revision INTEGER NOT NULL DEFAULT 1,PRIMARY KEY(plugin,namespace,entry)
);
CREATE TABLE notification_grant_operations(
    plugin TEXT NOT NULL,handle TEXT NOT NULL REFERENCES host_calls(handle),key TEXT NOT NULL,
    method TEXT NOT NULL,digest TEXT NOT NULL,result TEXT NOT NULL,PRIMARY KEY(plugin,handle,key)
);
CREATE VIEW notification_target_scope(plugin,entry,receiver) AS
 SELECT i.plugin,target.key,target.value FROM host_installations i
 JOIN host_notification_policies policy ON policy.plugin=i.plugin AND policy.revision=i.revision
 JOIN notification_publishers publisher ON publisher.plugin=i.plugin
 JOIN accounts account ON account.id=publisher.account AND account.namespace=i.namespace
 JOIN json_each(policy.body,'$.targets') target
 WHERE i.enabled AND NOT EXISTS(SELECT 1 FROM notification_target_grants g
     WHERE g.plugin=i.plugin AND g.namespace=i.namespace AND g.entry=target.key)
 UNION ALL
 SELECT i.plugin,g.entry,g.receiver FROM host_installations i
 JOIN host_notification_policies policy ON policy.plugin=i.plugin AND policy.revision=i.revision
 JOIN notification_publishers publisher ON publisher.plugin=i.plugin
 JOIN accounts account ON account.id=publisher.account AND account.namespace=i.namespace
 JOIN notification_target_grants g ON g.plugin=i.plugin AND g.namespace=i.namespace AND g.enabled
 JOIN persons creator ON creator.id=g.created_by AND creator.enabled
 JOIN persons root ON root.id=coalesce(creator.current_root,creator.id) AND root.enabled
 WHERE i.enabled;
CREATE TABLE notification_publications(
    id TEXT PRIMARY KEY,plugin TEXT NOT NULL,event_key TEXT NOT NULL,revision TEXT NOT NULL,
    digest TEXT NOT NULL,manifest TEXT,event INTEGER NOT NULL UNIQUE REFERENCES events(id),
    event_type TEXT NOT NULL,entry TEXT NOT NULL REFERENCES entries(id),receiver TEXT NOT NULL REFERENCES assignments(id),
    target_person TEXT REFERENCES persons(id),target_space TEXT,window INTEGER NOT NULL,
    origin TEXT NOT NULL UNIQUE REFERENCES main_origins(ref),delivery_execution TEXT NOT NULL UNIQUE REFERENCES executions(id),
    agent_execution TEXT UNIQUE REFERENCES executions(id),resume_waiting_work INTEGER NOT NULL CHECK(resume_waiting_work IN(0,1)),
    state TEXT NOT NULL DEFAULT 'published' CHECK(state IN('published','erased')),
    created_at INTEGER NOT NULL DEFAULT(unixepoch()),UNIQUE(plugin,event_key)
);
CREATE TABLE notification_deliveries(
    publication TEXT NOT NULL REFERENCES notification_publications(id),ordinal INTEGER NOT NULL,
    execution TEXT NOT NULL,operation TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN('pending','confirmed','failed','unknown','erased')),
    PRIMARY KEY(publication,ordinal),FOREIGN KEY(execution,operation) REFERENCES operations(namespace,operation_id)
);
CREATE TABLE notification_media(
    publication TEXT NOT NULL REFERENCES notification_publications(id),ordinal INTEGER NOT NULL,
    version TEXT NOT NULL REFERENCES media_versions(id),digest TEXT,
    PRIMARY KEY(publication,ordinal)
);
CREATE TABLE notification_sources(
    publication TEXT NOT NULL REFERENCES notification_publications(id),source INTEGER NOT NULL REFERENCES events(id),
    PRIMARY KEY(publication,source)
);
-- Legacy uploads keep an unknown notification lease. Their general media reader remains available.
ALTER TABLE host_media_uploads ADD COLUMN notification_expires INTEGER;
ALTER TABLE host_media_uploads ADD COLUMN notification_ttl INTEGER;
ALTER TABLE work_wait_terms ADD COLUMN external_source TEXT;
CREATE TRIGGER erase_notification_source AFTER INSERT ON invalidated_sources BEGIN
    UPDATE notification_publications SET manifest=NULL,state='erased'
    WHERE event=NEW.source OR id IN(SELECT publication FROM notification_sources WHERE source=NEW.source);
    UPDATE notification_deliveries SET state='erased'
    WHERE state='pending' AND publication IN(SELECT id FROM notification_publications WHERE state='erased');
END;

-- Keep original producers and Work readers, then add only the accepted publication executors.
DROP VIEW media_version_readers;
CREATE VIEW media_version_readers(version,reader,producer) AS
 SELECT v.id,v.execution,v.execution FROM media_versions v WHERE v.digest IS NOT NULL
 UNION ALL
 SELECT v.id,c.execution,v.execution FROM media_versions v JOIN work_runs p ON p.execution=v.execution
 JOIN work_runs c ON c.work=p.work AND c.execution!=p.execution JOIN work_nodes w ON w.id=p.work
 JOIN budget_members a ON a.execution=p.execution AND a.root=w.budget
 JOIN budget_members b ON b.execution=c.execution AND b.root=w.budget
 WHERE v.digest IS NOT NULL AND w.goal IS NOT NULL
 AND NOT EXISTS(SELECT 1 FROM work_sources s JOIN invalidated_sources i ON i.source=s.event WHERE s.work=w.id)
 UNION ALL
 SELECT v.id,reader.id,v.execution FROM notification_media m
 JOIN notification_publications p ON p.id=m.publication AND p.state='published'
 JOIN media_versions v ON v.id=m.version AND v.digest=m.digest
 JOIN executions reader ON reader.id IN(p.delivery_execution,p.agent_execution)
 JOIN notification_target_scope grant ON grant.plugin=p.plugin AND grant.entry=p.entry AND grant.receiver=p.receiver
 JOIN host_installations install ON install.plugin=p.plugin
 JOIN host_notification_policies policy ON policy.plugin=p.plugin AND policy.revision=install.revision
 JOIN entries e ON e.id=p.entry AND e.closed_at IS NULL
 JOIN assignments bot ON bot.id=p.receiver AND bot.status='active'
 LEFT JOIN persons recipient ON recipient.id=p.target_person
 LEFT JOIN persons root ON root.id=coalesce(recipient.current_root,recipient.id)
 WHERE p.window=coalesce((SELECT generation FROM main_windows WHERE conversation=p.entry),1)
 AND (reader.id=p.delivery_execution OR json_extract(policy.body,'$.agent'))
 AND (p.target_person IS NULL OR (recipient.enabled AND root.enabled))
 AND NOT EXISTS(SELECT 1 FROM notification_sources d JOIN invalidated_sources gone ON gone.source=d.source WHERE d.publication=p.id);
