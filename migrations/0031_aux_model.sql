-- Reader 31. Auxiliary calls share model_requests and model_attempts with Main.
CREATE TABLE host_aux_routes (
    plugin TEXT NOT NULL,
    revision TEXT NOT NULL,
    name TEXT NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY(plugin,revision,name)
);
CREATE TABLE host_aux_origins (
    plugin TEXT NOT NULL,
    original TEXT NOT NULL,
    execution TEXT NOT NULL UNIQUE REFERENCES executions(id),
    parent_execution TEXT REFERENCES executions(id),
    event INTEGER REFERENCES events(id),
    PRIMARY KEY(plugin,original)
);
CREATE TABLE host_aux_requests (
    id TEXT PRIMARY KEY,
    plugin TEXT NOT NULL,
    revision TEXT NOT NULL,
    origin TEXT NOT NULL,
    original_key TEXT NOT NULL,
    route TEXT NOT NULL,
    purpose TEXT NOT NULL,
    logical TEXT NOT NULL UNIQUE REFERENCES model_requests(id),
    request_hash TEXT NOT NULL,
    dispatched_generation TEXT,
    task TEXT,
    UNIQUE(plugin,origin,original_key),
    FOREIGN KEY(plugin,origin) REFERENCES host_aux_origins(plugin,original)
);
CREATE TRIGGER erase_aux_request_content AFTER INSERT ON invalidated_sources BEGIN
    UPDATE host_aux_requests SET task=NULL WHERE logical IN (
        SELECT r.id FROM model_requests r JOIN activations a ON a.id=r.activation
        WHERE a.execution IN (
            SELECT execution FROM main_dependencies WHERE event=NEW.source
        )
    );
END;
