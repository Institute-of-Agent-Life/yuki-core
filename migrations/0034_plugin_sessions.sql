-- Reader 34. Plugin-owned model sessions do not require a main Conversation.
PRAGMA defer_foreign_keys=ON;
PRAGMA legacy_alter_table=ON;
CREATE TABLE session_capable_origins (
    ref TEXT PRIMARY KEY NOT NULL CHECK(length(ref)>0),
    kind TEXT NOT NULL CHECK(kind IN ('plugin','self','schedule','work','child','memory_extract','embedding')),
    conversation TEXT REFERENCES entries(id), parent TEXT REFERENCES executions(id),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN(0,1)),
    CHECK(conversation IS NOT NULL OR (kind='plugin' AND ref LIKE 'plugin-aux:%'))
);
INSERT INTO session_capable_origins SELECT * FROM main_origins;
DROP TABLE main_origins;
ALTER TABLE session_capable_origins RENAME TO main_origins;
PRAGMA legacy_alter_table=OFF;
CREATE TEMP TABLE validated_session_origin_refs(count INTEGER CHECK(count=0));
INSERT INTO validated_session_origin_refs SELECT count(*) FROM pragma_foreign_key_check;
DROP TABLE validated_session_origin_refs;
PRAGMA defer_foreign_keys=OFF;

CREATE TABLE host_session_policies(
    plugin TEXT NOT NULL, revision TEXT NOT NULL,body TEXT NOT NULL,
    PRIMARY KEY(plugin,revision)
);
CREATE TABLE plugin_sessions(
    plugin TEXT NOT NULL,id TEXT NOT NULL,created_key TEXT NOT NULL,created_hash TEXT NOT NULL,
    revision TEXT NOT NULL,scope_kind TEXT NOT NULL CHECK(scope_kind IN('user','group','plugin')),
    person TEXT REFERENCES persons(id),entry TEXT REFERENCES entries(id),space TEXT,
    name TEXT,instructions TEXT,route TEXT NOT NULL,purpose TEXT NOT NULL,
    persistence TEXT NOT NULL CHECK(persistence IN('durable','ephemeral')),
    boot TEXT NOT NULL,context_profile TEXT NOT NULL CHECK(context_profile IN('none','current_user','current_group')),
    state TEXT NOT NULL DEFAULT 'active' CHECK(state IN('active','closed','expired','erased')),
    epoch INTEGER NOT NULL DEFAULT 0,next_sequence INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT(unixepoch()),updated_at INTEGER NOT NULL DEFAULT(unixepoch()),
    PRIMARY KEY(plugin,id),UNIQUE(plugin,created_key),
    CHECK(scope_kind!='user' OR (person IS NOT NULL AND space IS NULL)),
    CHECK(scope_kind!='plugin' OR (person IS NULL AND space IS NULL)),
    CHECK(scope_kind!='group' OR (entry IS NOT NULL AND space IS NOT NULL))
);
CREATE TABLE plugin_session_messages(
    plugin TEXT NOT NULL,session TEXT NOT NULL,sequence INTEGER NOT NULL,epoch INTEGER NOT NULL,
    role TEXT NOT NULL CHECK(role IN('user','assistant','tool')),sender TEXT REFERENCES persons(id),
    body TEXT,digest TEXT NOT NULL,turn TEXT,
    PRIMARY KEY(plugin,session,sequence),FOREIGN KEY(plugin,session) REFERENCES plugin_sessions(plugin,id)
);
CREATE TABLE plugin_session_operations(
    plugin TEXT NOT NULL,session TEXT NOT NULL,original_key TEXT NOT NULL,method TEXT NOT NULL,
    digest TEXT NOT NULL,result TEXT NOT NULL,
    PRIMARY KEY(plugin,session,original_key),FOREIGN KEY(plugin,session) REFERENCES plugin_sessions(plugin,id)
);
CREATE TABLE plugin_session_turns(
    id TEXT PRIMARY KEY,plugin TEXT NOT NULL,session TEXT NOT NULL,epoch INTEGER NOT NULL,
    original_key TEXT NOT NULL,digest TEXT NOT NULL,authority TEXT NOT NULL,model_params TEXT,
    parent TEXT REFERENCES executions(id),event INTEGER REFERENCES events(id),entry TEXT REFERENCES entries(id),
    request TEXT REFERENCES host_aux_requests(id),user_sequence INTEGER NOT NULL,assistant_sequence INTEGER,
    state TEXT NOT NULL CHECK(state IN('prepared','confirmed','unknown','failed','redacted')),
    UNIQUE(plugin,session,original_key),FOREIGN KEY(plugin,session) REFERENCES plugin_sessions(plugin,id)
);
CREATE UNIQUE INDEX one_pending_session_turn ON plugin_session_turns(plugin,session) WHERE state='prepared';
CREATE TABLE plugin_session_sources(
    plugin TEXT NOT NULL,session TEXT NOT NULL,source INTEGER NOT NULL REFERENCES events(id),
    PRIMARY KEY(plugin,session,source),FOREIGN KEY(plugin,session) REFERENCES plugin_sessions(plugin,id)
);
CREATE TABLE plugin_session_redactions(execution TEXT PRIMARY KEY REFERENCES executions(id),reason TEXT NOT NULL);
CREATE TRIGGER erase_plugin_session_source AFTER INSERT ON invalidated_sources BEGIN
    UPDATE plugin_sessions SET state='erased',name=NULL,instructions=NULL WHERE (plugin,id) IN
        (SELECT plugin,session FROM plugin_session_sources WHERE source=NEW.source);
    UPDATE plugin_session_messages SET body=NULL WHERE (plugin,session) IN
        (SELECT plugin,session FROM plugin_session_sources WHERE source=NEW.source);
    UPDATE plugin_session_turns SET state='redacted',model_params=NULL WHERE (plugin,session) IN
        (SELECT plugin,session FROM plugin_session_sources WHERE source=NEW.source);
END;
CREATE TABLE plugin_session_boot(singleton INTEGER PRIMARY KEY CHECK(singleton=1),generation TEXT NOT NULL);

CREATE TRIGGER erase_plugin_session_person AFTER INSERT ON forgotten_people BEGIN
    UPDATE plugin_sessions SET state='erased',name=NULL,instructions=NULL WHERE person=NEW.person
      OR (plugin,id) IN(SELECT plugin,session FROM plugin_session_messages WHERE sender=NEW.person);
END;
