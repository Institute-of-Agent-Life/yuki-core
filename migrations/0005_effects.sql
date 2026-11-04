-- Reader 5. Operations belong to the recorded execution namespace.
CREATE TABLE operations (
    namespace TEXT NOT NULL REFERENCES executions(id),
    operation_id TEXT NOT NULL CHECK(length(operation_id) BETWEEN 1 AND 256),
    action TEXT NOT NULL CHECK(action IN ('get','put','external')),
    target TEXT NOT NULL CHECK(length(target)>0 AND target=trim(target)),
    payload TEXT NOT NULL,
    parent TEXT,
    ordinal INTEGER,
    state TEXT NOT NULL CHECK(state IN ('intent','dispatched','confirmed','failed','unknown')),
    result TEXT,
    PRIMARY KEY(namespace,operation_id),
    FOREIGN KEY(namespace,parent) REFERENCES operations(namespace,operation_id),
    UNIQUE(namespace,parent,ordinal),
    CHECK((parent IS NULL AND ordinal IS NULL) OR
          (parent IS NOT NULL AND parent<>operation_id AND ordinal IS NOT NULL AND ordinal>=0)),
    CHECK(state NOT IN ('confirmed','failed') OR result IS NOT NULL)
);
CREATE TABLE local_effect_values (
    original_actor TEXT NOT NULL REFERENCES persons(id),
    target TEXT NOT NULL,
    value TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>0),
    PRIMARY KEY(original_actor,target)
);
