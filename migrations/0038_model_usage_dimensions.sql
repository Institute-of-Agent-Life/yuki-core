-- Non-content billing labels survive source erasure; absent historical labels stay empty.
CREATE TABLE model_usage_dimensions (
    logical TEXT PRIMARY KEY REFERENCES model_requests(id),
    task TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    profile_id TEXT NOT NULL
);
INSERT INTO model_usage_dimensions
SELECT r.id,coalesce(json_extract(c.value,'$.profile_task'),''),
       coalesce(json_extract(c.value,'$.profile_binding.profile.provider'),''),
       coalesce(json_extract(c.value,'$.model'),json_extract(r.request_json,'$.model'),''),
       coalesce(json_extract(c.value,'$.profile_binding.id'),'')
FROM model_requests r JOIN activations a ON a.id=r.activation
LEFT JOIN core_meta c ON c.key='model-profile-contract:'||a.profile_contract;
CREATE TRIGGER capture_model_usage_dimensions AFTER INSERT ON model_requests BEGIN
 INSERT INTO model_usage_dimensions
 SELECT NEW.id,coalesce(json_extract(c.value,'$.profile_task'),''),
        coalesce(json_extract(c.value,'$.profile_binding.profile.provider'),''),
        coalesce(json_extract(c.value,'$.model'),json_extract(NEW.request_json,'$.model'),''),
        coalesce(json_extract(c.value,'$.profile_binding.id'),'')
 FROM activations a LEFT JOIN core_meta c ON c.key='model-profile-contract:'||a.profile_contract
 WHERE a.id=NEW.activation;
END;
-- A non-billing fact is accounting evidence only. Erasure still clears retry_proof.
CREATE TABLE model_usage_unbilled (
    attempt TEXT PRIMARY KEY REFERENCES model_attempts(id)
);
INSERT INTO model_usage_unbilled SELECT id FROM model_attempts WHERE retry_proof=1;
CREATE TRIGGER retain_nonbilling_fact AFTER UPDATE OF retry_proof ON model_attempts
WHEN NEW.retry_proof=1 BEGIN
 INSERT OR IGNORE INTO model_usage_unbilled VALUES(NEW.id);
END;
