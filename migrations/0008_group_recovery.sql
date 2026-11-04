-- Reader 8. Preserve the native control and its original receiving stage across restarts.
CREATE TABLE recovered_groups (
    namespace TEXT NOT NULL, native_group TEXT NOT NULL, stage TEXT NOT NULL REFERENCES assignments(id),
    enabled INTEGER NOT NULL CHECK(enabled IN(0,1)), revision INTEGER NOT NULL CHECK(revision>=1),
    PRIMARY KEY(namespace,native_group)
);
CREATE TABLE group_recovery_receipts (
    namespace TEXT NOT NULL,native_group TEXT NOT NULL,native_message TEXT NOT NULL,
    stage TEXT NOT NULL REFERENCES assignments(id), actor_stage TEXT NOT NULL REFERENCES assignments(id),
    person TEXT NOT NULL REFERENCES persons(id), connection INTEGER NOT NULL, before_state TEXT NOT NULL, after_state TEXT NOT NULL,
    PRIMARY KEY(namespace,native_group,stage,native_message)
);
