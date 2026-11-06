-- An installation key alone does not prove which application/tenant received a resource.
-- Pre-49 gateway events have no evidence for this binding and cannot be backfilled.
CREATE TABLE gateway_media_origins(
 event INTEGER PRIMARY KEY REFERENCES events(id),plugin TEXT NOT NULL REFERENCES host_installations(plugin),
 revision TEXT NOT NULL, deployment TEXT NOT NULL, namespace TEXT NOT NULL, platform TEXT NOT NULL, stage TEXT NOT NULL REFERENCES assignments(id),
 identity TEXT NOT NULL CHECK(json_valid(identity)), locator TEXT NOT NULL CHECK(json_valid(locator))
);
CREATE TRIGGER forget_gateway_media_origin AFTER INSERT ON invalidated_sources BEGIN
 DELETE FROM gateway_media_origins WHERE event=NEW.source;
END;
