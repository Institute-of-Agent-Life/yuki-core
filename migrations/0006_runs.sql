-- Schema/reader 6. Unknown costs and original Work at acceptance are nullable.
ALTER TABLE executions ADD COLUMN cancelled INTEGER NOT NULL DEFAULT 0 CHECK(cancelled IN (0,1));
ALTER TABLE executions ADD COLUMN work_id TEXT;
CREATE TABLE root_budgets (
    root TEXT PRIMARY KEY REFERENCES executions(id),
    max_requests INTEGER NOT NULL CHECK(max_requests>=0), max_units INTEGER NOT NULL CHECK(max_units>=0),
    reserved_requests INTEGER NOT NULL DEFAULT 0, reserved_units INTEGER NOT NULL DEFAULT 0,
    CHECK(reserved_requests BETWEEN 0 AND max_requests), CHECK(reserved_units BETWEEN 0 AND max_units)
);
CREATE TABLE budget_members(execution TEXT PRIMARY KEY REFERENCES executions(id),root TEXT NOT NULL REFERENCES root_budgets(root));
CREATE TABLE activations (
    id TEXT PRIMARY KEY NOT NULL, execution TEXT NOT NULL REFERENCES budget_members(execution),
    profile_contract TEXT NOT NULL CHECK(length(profile_contract)>0)
);
CREATE TABLE model_requests (
    id TEXT PRIMARY KEY NOT NULL, activation TEXT NOT NULL REFERENCES activations(id), ordinal INTEGER NOT NULL,
    root TEXT NOT NULL REFERENCES root_budgets(root), original_work_id TEXT,
    estimated_input INTEGER NOT NULL CHECK(estimated_input>=0), max_output INTEGER NOT NULL CHECK(max_output>=0),
    retry_unbilled INTEGER NOT NULL CHECK(retry_unbilled IN (0,1)), native_tools INTEGER NOT NULL CHECK(native_tools IN (0,1)),
    request_json TEXT NOT NULL, UNIQUE(activation,ordinal)
);
CREATE TABLE model_attempts (
    id TEXT PRIMARY KEY NOT NULL, logical TEXT NOT NULL REFERENCES model_requests(id), ordinal INTEGER NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('dispatched','unknown','rejected','response','response_held','paired')),
    dispatched_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, received_at TEXT, status_code INTEGER, retry_proof INTEGER NOT NULL DEFAULT 0, ever_unknown INTEGER NOT NULL DEFAULT 0,
    response TEXT, usage_json TEXT, intents_json TEXT, diagnostic TEXT, UNIQUE(logical,ordinal)
);
CREATE TABLE model_tool_intents (
    logical TEXT NOT NULL REFERENCES model_requests(id), ordinal INTEGER NOT NULL, call_id TEXT NOT NULL,
    execution TEXT NOT NULL, operation_id TEXT NOT NULL, paired_receipt TEXT,
    PRIMARY KEY(logical,ordinal), UNIQUE(logical,call_id), UNIQUE(execution,operation_id),
    FOREIGN KEY(execution,operation_id) REFERENCES operations(namespace,operation_id)
);
