-- Reader 63. Own participation intent is derived from an accepted Main response.
CREATE TABLE participation_main_reports (
    logical TEXT PRIMARY KEY NOT NULL REFERENCES model_requests(id),
    attempt TEXT NOT NULL UNIQUE REFERENCES model_attempts(id),
    execution TEXT NOT NULL REFERENCES main_turns(execution),
    event INTEGER NOT NULL REFERENCES main_inputs(event),
    engage TEXT NOT NULL CHECK (engage IN ('join','stay','quiet'))
);
CREATE INDEX participation_main_reports_by_event ON participation_main_reports(event);
CREATE TRIGGER erase_participation_main_report AFTER INSERT ON invalidated_sources BEGIN
    DELETE FROM participation_main_reports WHERE event=NEW.source;
END;
CREATE TRIGGER erase_participation_main_attempt AFTER UPDATE OF response ON model_attempts
WHEN NEW.response IS NULL AND OLD.response IS NOT NULL BEGIN
    DELETE FROM participation_main_reports WHERE attempt=NEW.id;
END;
