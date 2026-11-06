-- Reader 26. Original IDs, source watermarks, cost and dedup keys survive erasure.
CREATE TABLE person_forgets(watermark INTEGER PRIMARY KEY REFERENCES source_erasure_requests(watermark),
 requested TEXT NOT NULL REFERENCES persons(id),members TEXT NOT NULL);
CREATE TABLE forgotten_people(person TEXT PRIMARY KEY REFERENCES persons(id),watermark INTEGER NOT NULL REFERENCES person_forgets(watermark));
CREATE TABLE forgotten_objects(watermark INTEGER NOT NULL REFERENCES person_forgets(watermark),digest TEXT NOT NULL,PRIMARY KEY(watermark,digest));
CREATE TABLE forgotten_transfers(watermark INTEGER NOT NULL REFERENCES person_forgets(watermark),relative TEXT NOT NULL,PRIMARY KEY(watermark,relative));
CREATE TRIGGER immutable_forgotten_person BEFORE UPDATE ON persons WHEN EXISTS(SELECT 1 FROM forgotten_people WHERE person=OLD.id)
 AND (NEW.enabled OR NEW.admin OR NEW.name!='' OR NEW.current_root IS NOT OLD.current_root)
 BEGIN SELECT RAISE(ABORT,'forgotten original Person'); END;
CREATE TRIGGER redact_erased_fact_key AFTER UPDATE OF status ON memory_facts WHEN NEW.status='erased' BEGIN
 UPDATE memory_facts SET fact_key='erased-'||NEW.number WHERE id=NEW.id;
 UPDATE extract_fact_details SET category=NULL WHERE fact=NEW.id;
END;
UPDATE memory_facts SET fact_key='erased-'||number WHERE status='erased';
UPDATE extract_fact_details SET category=NULL WHERE fact IN(SELECT id FROM memory_facts WHERE status='erased');
CREATE TABLE host_cleanup_generations(plugin TEXT NOT NULL REFERENCES host_installations(plugin),generation TEXT NOT NULL,PRIMARY KEY(plugin,generation));
