-- Reader 33. Installed maintenance scopes and immutable review/application receipts.
CREATE TABLE host_maintenance(plugin TEXT NOT NULL,revision TEXT NOT NULL,body TEXT NOT NULL,PRIMARY KEY(plugin,revision));
CREATE TABLE maintenance_previews(
    id TEXT PRIMARY KEY,plugin TEXT NOT NULL,revision TEXT NOT NULL,scope TEXT NOT NULL,purpose TEXT NOT NULL,
    original_key TEXT NOT NULL,digest TEXT NOT NULL,request TEXT NOT NULL REFERENCES host_aux_requests(id),
    execution TEXT NOT NULL REFERENCES executions(id),actions TEXT,state TEXT NOT NULL DEFAULT 'ready',
    UNIQUE(plugin,scope,purpose,original_key), UNIQUE(plugin,request)
);
CREATE TABLE maintenance_sources(preview TEXT NOT NULL REFERENCES maintenance_previews(id),event INTEGER NOT NULL REFERENCES events(id),role TEXT NOT NULL,version TEXT NOT NULL,PRIMARY KEY(preview,event));
CREATE TABLE maintenance_apply(plugin TEXT NOT NULL,operation TEXT NOT NULL,preview TEXT NOT NULL REFERENCES maintenance_previews(id),digest TEXT NOT NULL,result TEXT NOT NULL,PRIMARY KEY(plugin,operation));
CREATE TRIGGER erase_maintenance_preview AFTER INSERT ON invalidated_sources BEGIN
    UPDATE maintenance_previews SET actions=NULL,state='erased' WHERE id IN(SELECT preview FROM maintenance_sources WHERE event=NEW.source);
END;

CREATE TABLE maintenance_evidence(preview TEXT NOT NULL REFERENCES maintenance_previews(id),ordinal INTEGER NOT NULL,source TEXT,version TEXT NOT NULL,PRIMARY KEY(preview,ordinal));
CREATE TRIGGER erase_maintenance_evidence AFTER INSERT ON invalidated_sources BEGIN
    UPDATE maintenance_evidence SET source=NULL WHERE preview IN(SELECT preview FROM maintenance_sources WHERE event=NEW.source);
END;

CREATE TABLE maintenance_extraction_details(preview TEXT NOT NULL REFERENCES maintenance_previews(id),ordinal INTEGER NOT NULL,body TEXT,PRIMARY KEY(preview,ordinal));
CREATE TRIGGER erase_maintenance_extraction_details AFTER INSERT ON invalidated_sources BEGIN
    UPDATE maintenance_extraction_details SET body=NULL WHERE preview IN(SELECT preview FROM maintenance_sources WHERE event=NEW.source);
END;

CREATE TABLE maintenance_embeddings(plugin TEXT NOT NULL,operation TEXT NOT NULL,revision TEXT NOT NULL,scope TEXT NOT NULL,purpose TEXT NOT NULL,route TEXT NOT NULL,fact TEXT NOT NULL REFERENCES memory_facts(id),fact_revision INTEGER NOT NULL,digest TEXT NOT NULL,logical TEXT REFERENCES embedding_requests(logical),observed TEXT,PRIMARY KEY(plugin,operation));

CREATE TABLE maintenance_roots(execution TEXT PRIMARY KEY REFERENCES executions(id),plugin TEXT NOT NULL,revision TEXT NOT NULL,scope TEXT NOT NULL,purpose TEXT NOT NULL,method TEXT NOT NULL,operation TEXT NOT NULL,digest TEXT NOT NULL,params TEXT NOT NULL,UNIQUE(plugin,method,operation));
CREATE TABLE maintenance_audit_apply(plugin TEXT NOT NULL,operation TEXT NOT NULL,digest TEXT NOT NULL,result TEXT NOT NULL,PRIMARY KEY(plugin,operation));

CREATE TABLE maintenance_fact_inputs(preview TEXT NOT NULL REFERENCES maintenance_previews(id),fact TEXT NOT NULL REFERENCES memory_facts(id),revision INTEGER NOT NULL,source_version TEXT NOT NULL,role TEXT NOT NULL,PRIMARY KEY(preview,fact));

CREATE TABLE maintenance_fact_transitions(preview TEXT NOT NULL REFERENCES maintenance_previews(id),fact TEXT NOT NULL REFERENCES memory_facts(id),revision INTEGER NOT NULL,source_version TEXT NOT NULL,PRIMARY KEY(preview,fact));

-- Archived receipts keep their original human ancestry. A fresh installed reader
-- authorizes the exact entry before paging. Erased or disabled ancestry is excluded.
CREATE VIEW maintenance_execution_ancestry AS
WITH RECURSIVE family(execution,member) AS (
 SELECT id,id FROM executions
 UNION SELECT f.execution,o.parent FROM family f JOIN executions x ON x.id=f.member
 JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL
) SELECT * FROM family;
CREATE VIEW maintenance_execution_access AS
SELECT f.execution,e.raw_actor AS actor,e.entry_id AS entry
FROM maintenance_execution_ancestry f JOIN executions x ON x.id=f.member
JOIN events e ON e.id=x.source
WHERE NOT EXISTS (
 SELECT 1 FROM maintenance_execution_ancestry a JOIN executions y ON y.id=a.member
 LEFT JOIN main_origins o ON o.ref=y.origin
 LEFT JOIN persons p ON p.id=y.original_actor LEFT JOIN persons r ON r.id=coalesce(p.current_root,p.id)
 WHERE a.execution=f.execution AND (
 (y.origin IS NOT NULL AND NOT o.enabled) OR (y.source IS NOT NULL AND (NOT p.enabled OR NOT r.enabled))
 OR EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=y.source
   OR i.source IN(SELECT event FROM main_dependencies WHERE execution=y.id))
 ));
CREATE TABLE maintenance_tool_inputs(
 preview TEXT NOT NULL REFERENCES maintenance_previews(id),receipt INTEGER NOT NULL,
 version TEXT NOT NULL,role TEXT NOT NULL,PRIMARY KEY(preview,receipt));
