-- Root message locators never replace the native topic in original admission keys.
-- Events accepted before this evidence existed remain unbound.
CREATE TABLE gateway_topic_origins(
 event INTEGER PRIMARY KEY REFERENCES events(id),
 plugin TEXT NOT NULL REFERENCES host_installations(plugin),
 namespace TEXT NOT NULL,
 platform TEXT NOT NULL,
 stage TEXT NOT NULL REFERENCES assignments(id),
 deployment TEXT NOT NULL,
 identity TEXT NOT NULL CHECK(json_valid(identity)),
 topic_root TEXT NOT NULL
);
CREATE TRIGGER forget_gateway_topic_origin AFTER INSERT ON invalidated_sources BEGIN
 DELETE FROM gateway_topic_origins WHERE event=NEW.source;
END;
