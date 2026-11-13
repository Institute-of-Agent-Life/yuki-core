-- Reader 60. Retention for local audit metadata conveys no source reading grant.
CREATE TABLE maintenance_quality_dependencies(
    owner TEXT NOT NULL REFERENCES source_owners(id),
    source INTEGER NOT NULL REFERENCES events(id),
    reference TEXT NOT NULL UNIQUE,
    PRIMARY KEY(owner,source)
);
CREATE INDEX maintenance_quality_dependencies_source ON maintenance_quality_dependencies(source,owner);

CREATE TRIGGER erase_maintenance_quality_apply AFTER INSERT ON invalidated_sources BEGIN
    UPDATE maintenance_audit_apply SET result='{"erased":true}'
      WHERE EXISTS(SELECT 1 FROM maintenance_quality_dependencies d
        JOIN json_each(maintenance_audit_apply.result,'$.erasure_refs') r ON r.value=d.reference
        WHERE d.source=NEW.source AND d.owner=maintenance_audit_apply.plugin);
END;
