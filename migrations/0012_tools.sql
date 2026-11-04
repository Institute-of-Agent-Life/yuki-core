-- Reader 12. Existing paid activations retain their original request bodies and declarations.
CREATE TABLE tool_manifests(revision TEXT PRIMARY KEY,body TEXT NOT NULL);
CREATE TABLE execution_tool_views(execution TEXT PRIMARY KEY REFERENCES executions(id),revision TEXT NOT NULL REFERENCES tool_manifests(revision),code_projection INTEGER NOT NULL CHECK(code_projection IN(0,1)),page_budget INTEGER NOT NULL,storage_quota INTEGER NOT NULL,timezone TEXT NOT NULL);
CREATE TABLE tool_artifacts(handle TEXT PRIMARY KEY,version TEXT NOT NULL,execution TEXT NOT NULL REFERENCES executions(id),operation TEXT NOT NULL,
    body TEXT,UNIQUE(execution,operation),FOREIGN KEY(execution,operation) REFERENCES operations(namespace,operation_id));
CREATE TABLE tool_aliases(execution TEXT NOT NULL,operation TEXT NOT NULL,representative TEXT NOT NULL,
    PRIMARY KEY(execution,operation),FOREIGN KEY(execution,operation) REFERENCES operations(namespace,operation_id),FOREIGN KEY(execution,representative) REFERENCES operations(namespace,operation_id));
CREATE TABLE short_state(slot INTEGER PRIMARY KEY CHECK(slot BETWEEN 1 AND 3),text TEXT NOT NULL,revision INTEGER NOT NULL,expires INTEGER NOT NULL,execution TEXT NOT NULL REFERENCES executions(id));
CREATE TRIGGER erase_tool_material AFTER INSERT ON invalidated_sources BEGIN
 UPDATE tool_artifacts SET body=NULL WHERE execution IN (SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
 UPDATE short_state SET text='' WHERE execution IN (SELECT id FROM executions WHERE source=NEW.source UNION SELECT execution FROM main_dependencies WHERE event=NEW.source);
END;
