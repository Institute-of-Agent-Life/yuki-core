-- Schema/reader 4. An observation may have no resolved original actor or entry.
CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_scope TEXT NOT NULL,
    source_key TEXT NOT NULL,
    entry_id TEXT REFERENCES entries(id),
    account_id TEXT NOT NULL REFERENCES accounts(id),
    raw_actor TEXT REFERENCES persons(id),
    actor_stage TEXT REFERENCES assignments(id),
    bot_stage TEXT NOT NULL REFERENCES assignments(id),
    occurred_at INTEGER,
    observed_at INTEGER NOT NULL,
    envelope TEXT NOT NULL,
    mentions TEXT NOT NULL,
    UNIQUE(source_scope,source_key)
);
CREATE TABLE event_locators (
    event_id INTEGER NOT NULL REFERENCES events(id),
    receiver TEXT NOT NULL REFERENCES assignments(id),
    native_id TEXT NOT NULL,
    PRIMARY KEY(event_id,receiver)
);
CREATE TABLE accepted_sources (
    event_id INTEGER PRIMARY KEY REFERENCES events(id)
);
CREATE TABLE executions (
    id TEXT PRIMARY KEY NOT NULL,
    source INTEGER NOT NULL UNIQUE REFERENCES accepted_sources(event_id),
    original_actor TEXT NOT NULL REFERENCES persons(id),
    state TEXT NOT NULL CHECK(state IN ('pending','running','completed','held'))
);
