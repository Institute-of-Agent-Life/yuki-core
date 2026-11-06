CREATE TABLE main_processor_bindings(
 execution TEXT PRIMARY KEY REFERENCES executions(id), plugin TEXT NOT NULL REFERENCES host_installations(plugin),
 revision TEXT NOT NULL, deployment TEXT NOT NULL);
CREATE TABLE main_processor_calls(
 handle TEXT PRIMARY KEY REFERENCES host_calls(handle),event INTEGER NOT NULL REFERENCES events(id),slot INTEGER NOT NULL,
 FOREIGN KEY(event,slot) REFERENCES media_handles(event,slot));
