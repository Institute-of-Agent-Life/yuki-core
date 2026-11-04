"""Capture one v4 run from an isolated, pinned checkout using its own Python environment."""
import argparse
import base64
import contextlib
import datetime
import gzip
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import random
import secrets
import shutil
import sqlite3
import stat
import subprocess
import sys
import uuid
import pytest
from unittest.mock import patch
import xml.etree.ElementTree as ET

CORE = Path(__file__).resolve().parents[2]
SPEC = json.loads(Path(__file__).with_name("scenarios.json").read_text())
TABLES = {
    "runtime_work": "id, parent_work_id, conversation_id, generation, source_key, goal, revision, state, model_requests, tool_calls, sent_messages",
    "runtime_work_inputs": "id, conversation_id, generation, source_key, event_id, work_id, kind, state, ready",
    "runtime_work_effects": "effect_key, work_id, kind, state, receipt_json",
    "chat_events": "id, canonical_conversation_id, direction, origin, content, author_person_id",
    "social_operation_receipts": "id, source_turn_id, tool_call_id, source_conversation_id, action, status, event_id, target_id, original_event_id, platform_reference, error_category",
}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args]).decode().strip()


def offline():
    path = CORE / "tests/harness/network.py"
    spec = importlib.util.spec_from_file_location("oracle_network", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.install()


def rows(database):
    path = Path(database.engine.url.database).resolve()
    with contextlib.closing(sqlite3.connect(path.as_uri() + "?mode=ro", uri=True)) as db:
        db.row_factory = sqlite3.Row
        return {table: [dict(row) for row in db.execute(
            f"SELECT {fields} FROM {table} ORDER BY {fields.split(',')[0]}")] for table, fields in TABLES.items()}


class Capture:
    def __init__(self, source):
        self.source = source
        self.records = {}
        self.current = None

    @pytest.hookimpl(wrapper=True)
    def pytest_runtest_protocol(self, item):
        import time_machine
        # The live App issues expiring device challenges against SQLite's native clock.
        # Exercise its real leases with the OS clock and retain the original wire.
        if item.nodeid == SPEC["cases"]["app_work"]:
            return (yield)
        with time_machine.travel(datetime.datetime.fromisoformat(SPEC["utc"]), tick=False):
            return (yield)

    def pytest_runtest_setup(self, item):
        name = next(k for k, v in SPEC["cases"].items() if item.nodeid.endswith(v.split("::")[-1]))
        self.current = self.records[name] = {"nodeid": item.nodeid, "http": [], "frames": []}

    def observe(self, original, name, *, deployment=False):
        async def call(*args, **kwargs):
            env = await original(*args, **kwargs)
            self.records[name]["gateway_calls"] = (env[1] if deployment else env).bot.calls
            return env
        return call

    @pytest.hookimpl(wrapper=True)
    def pytest_runtest_call(self, item):
        with contextlib.ExitStack() as patches:
            if item.nodeid == SPEC["cases"]["partial"]:
                patches.enter_context(patch.object(item.module, "social_env", self.observe(item.module.social_env, "partial")))
            if item.nodeid == SPEC["cases"]["app_work"]:
                patches.enter_context(patch.object(item.module, "_deployment",
                    self.observe(item.module._deployment, "app_work", deployment=True)))
            result = yield
        if (database := item.funcargs.get("database")) is not None:
            self.current["tables"] = rows(database)
        return result


def run(args):
    import httpx
    import pytest
    from websockets.asyncio.server import ServerConnection

    source = args.source.resolve()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    assert git(source, "rev-parse", "HEAD") == SPEC["source_commit"]
    assert git(source, "rev-parse", "HEAD^{tree}") == SPEC["source_tree"]
    subprocess.run(["git", "-C", str(source), "diff", "--quiet", "HEAD", "--"], check=True)
    os.chdir(source)
    sys.path[:0] = [str(source), str(source / "src"), str(source / "packages/yuki-agent-connector/src")]
    offline()
    collector = Capture(source)
    send, recv = ServerConnection.send, ServerConnection.recv
    transport = httpx.MockTransport.handle_async_request
    transmit = httpx.AsyncClient.send

    async def on_client(client, request, **kw):
        response = await transmit(client, request, **kw)
        if request.url.host in {"127.0.0.1", "localhost"}:
            collector.current.setdefault("loopback", []).append({
                "method": request.method, "path": request.url.path, "status": response.status_code})
        return response

    async def on_send(peer, message, *rest, **kw):
        result = await send(peer, message, *rest, **kw)
        collector.current["frames"].append({"direction": "to_connector", "body": json.loads(message)})
        return result

    async def on_recv(peer, *rest, **kw):
        message = await recv(peer, *rest, **kw)
        collector.current["frames"].append({"direction": "from_connector", "body": json.loads(message)})
        return message

    async def http(mock, request):
        body = await request.aread()
        response = await transport(mock, request)
        collector.current["http"].append({"method": request.method, "path": request.url.path,
            "body_bytes": base64.b64encode(body).decode(), "status": response.status_code})
        return response

    directory = source / "test-results/compact"
    before = set(directory.glob("*/*/evidence.json"))
    rng = random.Random(SPEC["seed"])
    def guid():
        return uuid.UUID(int=rng.getrandbits(128), version=4)
    def nonce(size=None):
        return rng.randbytes(32 if size is None else size)
    with contextlib.ExitStack() as stack:
        for switch in [patch.object(uuid, "uuid4", guid), patch.object(secrets, "token_bytes", nonce),
                       patch.object(ServerConnection, "send", on_send), patch.object(ServerConnection, "recv", on_recv),
                       patch.object(httpx.MockTransport, "handle_async_request", http),
                       patch.object(httpx.AsyncClient, "send", on_client)]:
            stack.enter_context(switch)
        status = pytest.main(["-vv", "--maxfail=1", "-o", "faulthandler_timeout=25", "-c", str(source / "pyproject.toml"), "--rootdir", str(source),
            "--basetemp", str(args.scratch.resolve()), "--junitxml", str(out / "junit.xml"),
            *SPEC["cases"].values()], plugins=[collector])
    if status:
        raise RuntimeError(f"v4 pytest exit {status}")
    for path in sorted(set(directory.glob("*/*/evidence.json")) - before):
        original = json.loads(path.read_text(encoding="utf-8"))
        key = next(k for k, v in SPEC["cases"].items() if v == original["nodeid"])
        assert original["source_revision"] == SPEC["source_commit"]
        for file in original["source_files"]:
            assert sha((source / file["path"]).read_bytes()) == file["sha256"]
        entry = collector.records[key]
        entry["verified_sources"] = [{k: f[k] for k in ("path", "sha256")} for f in original["source_files"]]
        for field in ("wire_requests", "gateway_calls", "original_result", "maintenance"):
            if field in original:
                entry[field] = original[field]
    for key in ("next_main", "signature"):
        assert collector.records[key]["wire_requests"] and collector.records[key]["http"]
    assert len(ET.parse(out / "junit.xml").findall(".//testcase")) == len(SPEC["cases"])
    assert collector.records["app_work"]["gateway_calls"] and collector.records["app_work"]["loopback"]
    # All data is synthetic; request headers and device credentials are not part of the transcript.
    raw = json.dumps(collector.records, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode()
    (out / "transcript.json.gz").write_bytes(gzip.compress(raw, mtime=0))
    lib = next(p for p in (source / "src/yuki/_native").glob("vec1.*") if p.suffix in (".dll", ".dylib", ".so"))
    with contextlib.closing(sqlite3.connect(":memory:")) as db:
        db.enable_load_extension(True)
        db.load_extension(str(lib))
        info = db.execute("SELECT vec1_info()").fetchone()[0]
        options = sorted(v for v, in db.execute("PRAGMA compile_options"))
    from yuki.client_api.openapi import export_openapi as client
    from yuki.admin_api.openapi import export_openapi as admin
    client(out / "client-openapi.json")
    admin(out / "admin-openapi.json")
    index = {"source_commit": SPEC["source_commit"], "tree": SPEC["source_tree"], "python": sys.version.split()[0],
             "sqlite": sqlite3.sqlite_version, "compile_options": options, "vec1": info, "vec1_sha256": sha(lib.read_bytes()),
             "uv_lock_sha256": sha((source / "uv.lock").read_bytes()), "raw_sha256": sha(raw),
             "nodeids": SPEC["cases"], "status": "verified"}
    (out / "run.json").write_text(json.dumps(index, indent=2, ensure_ascii=False), encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    for name in ("source", "out", "scratch"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    scratch = args.scratch.resolve()
    scratch.mkdir(parents=True, exist_ok=False)
    args.scratch = scratch / "pytest"
    try:
        run(args)
    finally:
        # v4 marks its template DB read-only. Clear that attribute only on our
        # exclusively created scratch tree, after pytest has closed its handles.
        def remove_readonly(operation, name, error):
            if os.name != "nt" or not isinstance(error, PermissionError) or not Path(name).is_file():
                raise error
            os.chmod(name, stat.S_IWRITE | stat.S_IREAD)
            operation(name)
        shutil.rmtree(scratch, onexc=remove_readonly)
