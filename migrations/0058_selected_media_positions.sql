-- The selected call records an original position after the readable handle is erased.
CREATE TABLE retained_main_processor_calls(
 handle TEXT PRIMARY KEY REFERENCES host_calls(handle),
 event INTEGER NOT NULL REFERENCES events(id),slot INTEGER NOT NULL CHECK(slot>=0));
INSERT INTO retained_main_processor_calls SELECT handle,event,slot FROM main_processor_calls;
DROP TABLE main_processor_calls;
ALTER TABLE retained_main_processor_calls RENAME TO main_processor_calls;
CREATE TRIGGER admit_main_processor_position BEFORE INSERT ON main_processor_calls
 WHEN NOT EXISTS(SELECT 1 FROM media_handles WHERE event=NEW.event AND slot=NEW.slot)
 BEGIN SELECT RAISE(ABORT,'selected attachment position unavailable'); END;
