"""Start the real Rust owner with private local IPC and a disposable data directory."""
import hashlib
import sqlite3
from contextlib import closing
import json
import os
import subprocess
from contextlib import contextmanager

class Service:
    def __init__(self, ctx, data):
        self.ctx = ctx
        self.data = data
        self.binary = ctx.root / "target/debug" / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
        self.argv = [str(self.binary), "--data", str(data)]

    def command(self, *args, accepted=True):
        result = subprocess.run([*self.argv, *args], capture_output=True, text=True, timeout=20)
        if accepted and result.returncode:
            self.ctx.write("failed-command.json", {"args":args,"stderr":result.stderr,"exit_code":result.returncode})
        self.ctx.equal("command " + " ".join(args) + (" succeeds" if accepted else " rejects"),
                       (result.returncode == 0, result.stderr if accepted else ""), (accepted, ""))
        return json.loads(result.stdout) if accepted else result.stderr.strip()

    @contextmanager
    def running(self):
        server = subprocess.Popen([*self.argv, "run"], stdout=subprocess.PIPE, text=True)
        try:
            self.ctx.equal("owner listening", json.loads(server.stdout.readline())["status"], "ready")
            yield self
            self.command("operator", "shutdown")
            self.ctx.equal("owner shutdown", server.wait(timeout=10), 0)
        finally:
            if server.poll() is None:
                server.kill()
                server.wait(timeout=10)
            server.stdout.close()


def previous_schema(ctx, data, version):
    data.mkdir()
    (data / "yuki.toml").write_text('format_version = 1\nlisten = "127.0.0.1:0"\n')
    with closing(sqlite3.connect(data / "core.sqlite")) as db:
        db.executescript("""
            CREATE TABLE _sqlx_migrations(version BIGINT PRIMARY KEY, description TEXT NOT NULL,
                installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, success BOOLEAN NOT NULL,
                checksum BLOB NOT NULL, execution_time BIGINT NOT NULL);
        """)
        for file in sorted((ctx.root/"migrations").glob("[0-9]*_*.sql")):
            sequence, description = file.stem.split("_", 1)
            if int(sequence) > version:
                continue
            sql = file.read_bytes()
            db.executescript(sql.decode())
            db.execute("INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) VALUES(?,?,1,?,0)",
                       (int(sequence),description.replace("_"," "),hashlib.sha384(sql).digest()))
        db.commit()
