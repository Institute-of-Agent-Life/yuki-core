-- Additive Admin revision/name projections. Original assignment owners remain immutable.
ALTER TABLE assignments ADD COLUMN display_name TEXT NOT NULL DEFAULT '';
ALTER TABLE assignments ADD COLUMN revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>=1);
CREATE TRIGGER assignment_control_revision AFTER UPDATE OF status,display_name ON assignments
WHEN NEW.status<>OLD.status OR NEW.display_name<>OLD.display_name
BEGIN
  UPDATE assignments SET revision=OLD.revision+1 WHERE id=NEW.id;
END;
CREATE TABLE person_names (
    person TEXT NOT NULL REFERENCES persons(id),
    name TEXT NOT NULL CHECK(length(name)>0),
    PRIMARY KEY(person,name)
);
