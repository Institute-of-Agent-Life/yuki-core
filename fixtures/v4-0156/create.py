"""Build a small quiesced multi-owner snapshot, using v4 table and column names."""
import hashlib
import json
import shutil
import sqlite3
import sys
import tempfile
from contextlib import closing
from pathlib import Path


def create(root):
    root.mkdir(parents=True, exist_ok=False)
    main = "core.db"
    with tempfile.TemporaryDirectory() as active:
        dbfile = Path(active) / main
        with closing(sqlite3.connect(dbfile)) as db:
            db.execute("PRAGMA journal_mode=WAL")
            db.execute("PRAGMA wal_autocheckpoint=0")
            db.executescript("""
                CREATE TABLE alembic_version(version_num TEXT PRIMARY KEY);
                INSERT INTO alembic_version VALUES ('0156');
                CREATE TABLE persons(id TEXT PRIMARY KEY, enabled INTEGER, revision INTEGER);
                CREATE TABLE account_assignments(id TEXT PRIMARY KEY, person_id TEXT REFERENCES persons);
                CREATE TABLE runtime_work(id TEXT PRIMARY KEY, source_key TEXT, state TEXT);
                INSERT INTO persons VALUES ('original-person', 1, 1), ('no-account', 1, 1), ('forgotten', 1, 1);
                INSERT INTO account_assignments VALUES ('assignment', 'original-person');
            """)
            db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
            db.executescript("""
                DELETE FROM persons WHERE id='forgotten';
                INSERT INTO runtime_work VALUES ('original-work', 'event:original', 'waiting_user');
            """)
            for suffix in ("", "-wal"):
                shutil.copyfile(Path(str(dbfile) + suffix), root / (main + suffix))
    workspace = root / "workspace"
    workspace.mkdir()
    data = b"synthetic immutable document"
    (workspace / "retained.blob").write_bytes(data)
    with closing(sqlite3.connect(workspace / "manifest.sqlite3")) as db:
        db.execute("CREATE TABLE artifacts(id TEXT PRIMARY KEY, blob TEXT, sha256 TEXT, size INTEGER)")
        db.executemany("INSERT INTO artifacts VALUES (?,?,?,?)", [
            ("present", "retained.blob", hashlib.sha256(data).hexdigest(), len(data)),
            ("missing", "missing.blob", hashlib.sha256(b"missing").hexdigest(), 7)])
        db.commit()
    stores = {"core": main, "workspace": "workspace/manifest.sqlite3"}
    for owner, table in (("participation", "observations"), ("plugin:fixture", "subscriptions"),
                         ("connector", "journal")):
        name = table + ".sqlite3"
        with closing(sqlite3.connect(root / name)) as db:
            db.execute(f"CREATE TABLE {table}(id TEXT PRIMARY KEY)")
            db.execute(f"INSERT INTO {table} VALUES ('old-key')")
            db.commit()
        stores[owner] = name
    roots = {"manager":"manager-home", "media":"media", "protocol":"work-protocol"}
    for directory in roots.values():
        (root / directory).mkdir()
        (root / directory / "original").write_bytes(("synthetic " + directory).encode())
    (root / "secret-store").mkdir()
    (root / "secret-store" / "value").write_text("synthetic-secret-do-not-read")
    (root / "METADATA").write_bytes(b"Metadata-Version: 2.1\nName: yuki-agent-connector\nVersion: 0.4.3\n")
    (root / "image.json").write_bytes(b'{"schemaVersion":2,"annotations":{"fixture":"synthetic"}}')
    (root / "CHANGELOG.md").write_text("数据库 head 为 **0143**。", encoding="utf-8")
    (root / "README.md").write_text("<!-- release-baseline: version=4.0.0 schema=0155 -->\n"
                                    "本机连接器 **0.4.2 / 协议 4**", encoding="utf-8")
    snapshots = {}
    for owner, name in stores.items():
        def checksum(path):
            return hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None
        snapshots[owner] = {"path": name, "db_sha256": checksum(root / name),
                            "wal_sha256": checksum(root / (name + "-wal"))}
    snapshot = {"snapshot_id":"fixture-0156", "kind":"synthetic", "capture_method":"quiesced",
                "owner_barrier":"fixture closed to all writes before copying",
                "privacy_watermark":"forget:7", "databases":snapshots, "file_roots":roots,
                "credential_namespaces":["qq-device","model-provider"],
                "connector_metadata":"METADATA", "image_manifest":"image.json",
                "documents":{"changelog":"CHANGELOG.md", "readme":"README.md"},
                "deployment_image_digest":"sha256:" + checksum(root / "image.json")}
    (root / "snapshot.json").write_text(json.dumps(snapshot, indent=2), encoding="utf-8")


if __name__ == "__main__":
    create(Path(sys.argv[1]))
