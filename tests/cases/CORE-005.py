"""Real binary/SQLite/OS lock/HTTP/local-control acceptance sequence."""

import hashlib
import http.client
import json
import os
import signal
import subprocess
import sys


def run(ctx):
    binary = ctx.root / "target/debug" / ("yuki-core.exe" if sys.platform == "win32" else "yuki-core")
    data = ctx.working / "data"
    observations = []

    def command(*args, success=True):
        result = subprocess.run([str(binary), "--data", str(data), *args],
                                capture_output=True, text=True, timeout=15)
        observations.append({"args": list(args), "exit_code": result.returncode,
                             "stdout": result.stdout, "stderr": result.stderr})
        ctx.write("operator-and-health.json", observations)
        ctx.equal("command outcome: " + " ".join(args), result.returncode == 0, success)
        return result

    command("init")
    original = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                for p in [data / "yuki.toml", data / "SOUL.md"]}
    command("init", success=False)
    ctx.equal("init preserves existing configuration and SOUL", original,
              {name: hashlib.sha256((data / name).read_bytes()).hexdigest() for name in original})
    command("doctor")
    command("migrate")
    with (ctx.output / "server-stderr.txt").open("w", encoding="utf-8") as errors:
        process = subprocess.Popen([str(binary), "--data", str(data), "run"],
                                   stdout=subprocess.PIPE, stderr=errors, text=True)
        try:
            boot = json.loads(process.stdout.readline())
            ctx.equal("server startup", boot["status"], "ready")
            host, port = boot["listen"].rsplit(":", 1)

            def health(path):
                connection = http.client.HTTPConnection(host, int(port), timeout=5)
                try:
                    connection.request("GET", path)
                    response = connection.getresponse()
                    value = json.loads(response.read())
                    observations.append({"path": path, "status": response.status, "body": value})
                    ctx.write("operator-and-health.json", observations)
                    ctx.equal("health status: " + path, response.status, 200)
                    return value
                finally:
                    connection.close()

            health("/livez")
            ctx.check("ready without QQ", health("/readyz")["ready"])
            before = health("/healthz")
            ctx.check("reader and writer reported separately",
                      before["reader_ok"] and before["writer_accepting"])
            database = json.loads(command("operator", "database-info").stdout)
            ctx.equal("foreign keys enabled", database["foreign_keys"], 1)
            ctx.equal("WAL enabled", database["journal_mode"], "wal")
            latest = max(int(p.name.split("_")[0]) for p in (ctx.root / "migrations").glob("[0-9]*_*.sql"))
            ctx.equal("schema matches the last embedded migration", database["schema_version"], latest)
            ctx.check("FTS5 compiled in", "ENABLE_FTS5" in database["compile_options"])
            ctx.write("sqlite-build.json", database)
            command("run", success=False)
            command("migrate", success=False)
            command("operator", "set-label", "original owner")
            label = json.loads(command("operator", "get-label").stdout)
            ctx.equal("online mutation uses original writer", label["value"], "original owner")
            for _ in range(3):
                command("doctor")
                health("/healthz")
            after = health("/healthz")
            ctx.equal("health and doctor do not write", after["commits"], before["commits"] + 1)
            if os.name == "posix":
                ctx.equal("operator socket is owner-only", (data / "operator.sock").stat().st_mode & 0o777, 0o600)
            else:
                from tests.harness.windows_acl import inspect_pipe
                policy = inspect_pipe(boot["operator"])
                ctx.check("named pipe DACL is protected", policy["protected"])
                ctx.equal("named pipe permits only current user and SYSTEM",
                          policy["trustees"], ["SYSTEM", "current_user"])
                ctx.write("operator-acl.json", policy)
            command("operator", "shutdown")
            ctx.equal("graceful shutdown", process.wait(timeout=15), 0)
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=10)
    command("migrate")
    ctx.write("operator-and-health.json", observations)
    if os.name == "posix":
        with (ctx.output / "server-stderr.txt").open("a", encoding="utf-8") as errors:
            restarted = subprocess.Popen([str(binary), "--data", str(data), "run"],
                                         stdout=subprocess.PIPE, stderr=errors, text=True)
            try:
                ctx.equal("restart preserves schema and ownership", json.loads(restarted.stdout.readline())["status"], "ready")
                restarted.send_signal(signal.SIGTERM)
                ctx.equal("SIGTERM closes writer before releasing owner", restarted.wait(timeout=15), 0)
            finally:
                if restarted.poll() is None:
                    restarted.kill()
                    restarted.wait(timeout=10)
        command("migrate")
    return {"kind": "owner-probes", "normal": "real binary + SQLite + HTTP + local IPC",
            "double_owner": "rejected", "offline_writer_while_online": "rejected",
            "configuration": "preserved", "shutdown": "owner released after close",
            "R01": {"evidence": "rust-writer-tests.txt",
                    "scope": "real SQLite with injected rollback/close boundary faults"}}
