-- Schema/reader 3. started_at is a proven lower bound, not a guess from first observation.
CREATE TABLE accounts (
    id TEXT PRIMARY KEY NOT NULL,
    platform TEXT NOT NULL CHECK(length(trim(platform)) > 0 AND platform=trim(platform)),
    namespace TEXT NOT NULL CHECK(length(trim(namespace)) > 0 AND namespace=trim(namespace)),
    native TEXT NOT NULL CHECK(length(trim(native)) > 0 AND native=trim(native)),
    UNIQUE(platform,namespace,native)
);
CREATE TABLE assignments (
    id TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    original_person TEXT REFERENCES persons(id),
    kind TEXT NOT NULL CHECK(kind IN ('person','self')),
    status TEXT NOT NULL CHECK(status IN ('active','disabled','ended')),
    first_observed INTEGER NOT NULL,
    started_at INTEGER,
    ended_at INTEGER,
    previous TEXT UNIQUE REFERENCES assignments(id),
    CHECK((kind='person')=(original_person IS NOT NULL)),
    CHECK((status='ended')=(ended_at IS NOT NULL)),
    CHECK(started_at IS NULL OR started_at<=first_observed),
    CHECK(ended_at IS NULL OR ended_at>=coalesce(started_at,first_observed))
);
CREATE UNIQUE INDEX one_current_assignment ON assignments(account_id) WHERE status<>'ended';
CREATE INDEX assignment_history ON assignments(account_id,started_at,ended_at);
CREATE TABLE entries (
    id TEXT PRIMARY KEY NOT NULL,
    scope TEXT NOT NULL CHECK(scope IN ('private','group')),
    actor_stage TEXT REFERENCES assignments(id),
    bot_stage TEXT REFERENCES assignments(id),
    space TEXT,
    topic TEXT CHECK(topic IS NULL OR length(trim(topic))>0),
    closed_at INTEGER,
    CHECK((scope='private' AND actor_stage IS NOT NULL AND bot_stage IS NOT NULL AND space IS NULL)
       OR (scope='group' AND actor_stage IS NULL AND bot_stage IS NULL AND length(space)>0 AND space IS NOT NULL))
);
CREATE UNIQUE INDEX live_dm ON entries(actor_stage,bot_stage) WHERE scope='private' AND topic IS NULL AND closed_at IS NULL;
CREATE UNIQUE INDEX live_dm_topic ON entries(actor_stage,bot_stage,topic) WHERE scope='private' AND topic IS NOT NULL AND closed_at IS NULL;
CREATE UNIQUE INDEX live_group ON entries(space) WHERE scope='group' AND topic IS NULL AND closed_at IS NULL;
CREATE UNIQUE INDEX live_group_topic ON entries(space,topic) WHERE scope='group' AND topic IS NOT NULL AND closed_at IS NULL;
CREATE TABLE assignment_states (
    stage TEXT NOT NULL REFERENCES assignments(id),
    at INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('active','disabled','ended')),
    PRIMARY KEY(stage,at)
);
