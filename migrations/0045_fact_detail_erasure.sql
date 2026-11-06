-- Reader 45. Explicit mutations and extraction share the same current projection.
-- Erasure must clear its content regardless of which writer originally created it.
CREATE TRIGGER erase_current_fact_details AFTER UPDATE OF status ON memory_facts
 WHEN NEW.status='erased' BEGIN
 UPDATE extract_fact_details SET category=NULL,kind=NULL,importance=NULL,temporal_mode=NULL,valid_from=NULL,valid_until=NULL WHERE fact=NEW.id;
END;
UPDATE extract_fact_details SET category=NULL,kind=NULL,importance=NULL,temporal_mode=NULL,valid_from=NULL,valid_until=NULL
 WHERE fact IN(SELECT id FROM memory_facts WHERE status='erased');
