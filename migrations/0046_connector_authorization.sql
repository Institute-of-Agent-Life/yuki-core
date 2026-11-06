-- Reader 46. Device authorization only; native instances and journals stay with their owner.
CREATE TABLE connector_proofs(
 hash TEXT PRIMARY KEY,device TEXT NOT NULL REFERENCES personal_devices(id),
 plugin TEXT NOT NULL REFERENCES host_installations(plugin),generation TEXT NOT NULL,
 expires INTEGER NOT NULL,used INTEGER);
CREATE TABLE connector_epochs(
 plugin TEXT NOT NULL REFERENCES host_installations(plugin),device TEXT NOT NULL REFERENCES personal_devices(id),
 epoch INTEGER NOT NULL CHECK(epoch>0),PRIMARY KEY(plugin,device));
CREATE TABLE connector_connections(
 hash TEXT PRIMARY KEY,device TEXT NOT NULL REFERENCES personal_devices(id),
 plugin TEXT NOT NULL REFERENCES host_installations(plugin),generation TEXT NOT NULL,
 epoch INTEGER NOT NULL CHECK(epoch>0),expires INTEGER NOT NULL,closed INTEGER NOT NULL DEFAULT 0 CHECK(closed IN(0,1)),
 UNIQUE(plugin,device,epoch));
