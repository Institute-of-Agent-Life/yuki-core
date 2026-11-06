-- Reader 32. Durable scene bindings, independent of a plugin process generation.
CREATE TABLE host_participation (
    plugin TEXT NOT NULL,
    revision TEXT NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY(plugin,revision)
);
CREATE TABLE participation_proposals (
    plugin TEXT NOT NULL,
    proposal TEXT NOT NULL,
    revision TEXT NOT NULL,
    entry TEXT NOT NULL REFERENCES entries(id),
    receiver TEXT NOT NULL REFERENCES assignments(id),
    origin TEXT NOT NULL UNIQUE REFERENCES main_origins(ref),
    digest TEXT NOT NULL,
    execution TEXT UNIQUE REFERENCES executions(id),
    PRIMARY KEY(plugin,proposal)
);
CREATE TABLE participation_promotions (
    plugin TEXT NOT NULL,
    event INTEGER NOT NULL REFERENCES events(id),
    PRIMARY KEY(plugin,event)
);
-- Current boot configuration is replaced atomically before Host callbacks start.
CREATE TABLE participation_qq_receivers(stage TEXT PRIMARY KEY REFERENCES assignments(id));
CREATE TABLE participation_qq_groups(namespace TEXT NOT NULL,native_group TEXT NOT NULL,stage TEXT NOT NULL,enabled INTEGER NOT NULL,PRIMARY KEY(namespace,native_group));
