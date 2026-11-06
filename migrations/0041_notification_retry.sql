-- Stable local outbox identities; legacy import must preserve its own explicit mapping.
CREATE TABLE notification_outbox_control(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    publication TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 5,
    error_category TEXT,
    UNIQUE(publication,ordinal),
    FOREIGN KEY(publication,ordinal) REFERENCES notification_deliveries(publication,ordinal)
);
INSERT INTO notification_outbox_control(publication,ordinal)
 SELECT publication,ordinal FROM notification_deliveries ORDER BY publication,ordinal;
CREATE TRIGGER notification_outbox_identity AFTER INSERT ON notification_deliveries BEGIN
    INSERT INTO notification_outbox_control(publication,ordinal) VALUES(NEW.publication,NEW.ordinal);
END;
CREATE TRIGGER notification_outbox_revision AFTER UPDATE OF state ON notification_deliveries
WHEN NEW.state!=OLD.state BEGIN
    UPDATE notification_outbox_control SET revision=revision+1
    WHERE publication=NEW.publication AND ordinal=NEW.ordinal;
END;
