-- Reader 62. Original mention identity belongs to the event-time assignment.
CREATE VIEW event_person_mentions AS
 SELECT e.id AS event,json_extract(m.value,'$.original') AS person
 FROM events e JOIN json_each(e.mentions) m
 WHERE json_type(m.value,'$.original')='text';

CREATE VIEW participation_erased_mentions AS
 SELECT m.event,f.watermark
 FROM event_person_mentions m JOIN forgotten_people f USING(person);

CREATE VIEW participation_held_sources AS
 SELECT owner,source FROM source_holds WHERE item GLOB 'participation:*';

CREATE TABLE participation_mention_redactions (
 execution TEXT PRIMARY KEY REFERENCES executions(id),
 watermark INTEGER NOT NULL REFERENCES person_forgets(watermark)
);
CREATE INDEX participation_redactions_watermark ON participation_mention_redactions(watermark);
