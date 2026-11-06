"""Run scheduled native inputs through one actual QQ websocket, a physical Chat endpoint and the SQLite owner."""

import argparse
import json
import os
import sqlite3
import subprocess
import sys
import time
from contextlib import closing
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from bench.compare import decide, trial_schedule, storage  # noqa: E402
from bench.native_arrivals import ASSIGNMENT_EPOCH, load, pack, sha
from bench.controls.chat import Chat  # noqa: E402
from tests.peers.onebot.peer import Peer  # noqa: E402


def run(binary, data, out, frames, frames_sha256, limit=None, warmup=0):
    out.mkdir(parents=True, exist_ok=False)

    plan = trial_schedule(limit, warmup)
    packets = load(frames, frames_sha256, plan)

    def cli(*args):
        result = subprocess.run(
            [str(binary), "--data", str(data), *map(str, args)],
            capture_output=True,
            timeout=30,
            check=True,
        )
        return json.loads(result.stdout)

    def op(*args):
        return cli("operator", *args)

    def start():
        server = subprocess.Popen(
            [str(binary), "--data", str(data), "run"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            line = server.stdout.readline()
            if not line:
                raise RuntimeError(
                    "owner startup failed: " + server.stderr.read().strip()
                )
            info = json.loads(line)
            assert info["status"] == "ready"
            return server, info["listen"]
        except Exception:
            server.kill()
            server.wait()
            raise

    def stop(p):
        if p.poll() is None:
            try:
                op("shutdown")
                p.wait(timeout=12)
            finally:
                if p.poll() is None:
                    p.kill()
                    p.wait()
        p.stdout.close()
        p.stderr.close()

    cli("init", "--memory-mode", "lexical-only")
    early, _ = start()
    actor = "00000000-0000-4000-8000-000000000040"
    bot = "80001"
    try:
        op("person", "create", actor)
        op(
            "account",
            "open",
            "qq",
            "slice",
            "10001",
            ASSIGNMENT_EPOCH * 1000,
            "--person",
            actor,
            "--start",
            ASSIGNMENT_EPOCH * 1000,
        )
        stage = op(
            "account",
            "open",
            "qq",
            "slice",
            bot,
            ASSIGNMENT_EPOCH * 1000,
            "--start",
            ASSIGNMENT_EPOCH * 1000,
        )["assignment"]
        database = op("database-info")
    finally:
        stop(early)
    rows = []
    timings = []
    expected = []
    with Chat() as http:
        cfg = data / "yuki.toml"
        cfg.write_text(
            'model_profiles_file="slice-models.toml"\n'
            + cfg.read_text(encoding="utf-8"),
            encoding="utf-8",
        )
        # A/B's actual injected Chat provider sends medium effort, a 4096-token cap
        # and automatic tool selection. Bind a real local Profile to reproduce it.
        (data / "slice-models.toml").write_text(
            f'''schema_version = 3
[profiles.main]
provider = "fixture"
protocol = "chat_completions"
base_url = "{http.url}"
api_key_env = "YUKI_SLICE_PEER"
model = "gemini-main"
timeout_seconds = 10.0
max_retries = 0
default_temperature = 0.2
default_max_output_tokens = 4096
thinking_mode = "enabled"
reasoning_effort = "medium"
capabilities = ["tools","reasoning"]
[routes]
chat_agent = "main"
memory_extraction = "main"
memory_self_reflection = "main"
memory_dream = "main"
plugin_agent_session = "main"
utility_structured = "main"
''',
            encoding="utf-8",
        )
        with (data / "yuki.toml").open("a", encoding="utf-8") as f:
            f.write(
                f'\n[qq]\nnamespace="slice"\ndesignated_ingress="{bot}"\n[qq.groups]\n"20001"="open"\n"20002"="open"\n[[qq.bots]]\nself_id="{bot}"\nstage="{stage}"\ntoken_env="YUKI_SLICE_PEER"\n[main]\nendpoint="{http.url}/chat/completions"\nmodel="gemini-main"\nname="Yuki"\nmerge_ms=0\nroot_slots=4\nmax_input_units=1000000\nmax_output=4096\n'
            )
        original = os.environ.get("YUKI_SLICE_PEER")
        os.environ["YUKI_SLICE_PEER"] = "synthetic-offline"
        boot = time.perf_counter()
        server, address = start()
        boot_elapsed = time.perf_counter() - boot
        peer = Peer(address, bot, "synthetic-offline")
        try:
            with (
                closing(
                    sqlite3.connect(
                        (data / "core.sqlite").as_uri() + "?mode=ro", uri=True
                    )
                ) as db,
                (out / "arrivals.jsonl").open("x", encoding="utf-8") as log,
            ):
                for item, frame in zip(plan, packets, strict=True):
                    key = item["input"]
                    destination = item["target"]
                    mid = frame["message_id"]
                    http.key = key
                    wire_offset = len(http.wires)
                    start_at = time.perf_counter()
                    peer.send(frame)
                    deliveries = []
                    receive_at = None
                    while not deliveries:
                        event = peer.receive()
                        action = event["action"]
                        result = {}
                        if action in ("send_group_msg", "send_private_msg"):
                            receive_at = time.perf_counter()
                            deliveries.append(event["params"])
                            result = {"message_id": mid + 100000}
                        peer.send(
                            {
                                "status": "ok",
                                "retcode": 0,
                                "echo": event["echo"],
                                "data": result,
                            }
                        )
                    deadline = time.perf_counter() + 12
                    while True:
                        seen = db.execute(
                            "SELECT x.id,CASE WHEN e.id IN(SELECT event_id FROM accepted_sources) AND x.origin IS NULL AND EXISTS(SELECT 1 FROM main_inputs WHERE execution=x.id AND event=e.id) AND EXISTS(SELECT 1 FROM model_requests r JOIN activations a ON a.id=r.activation WHERE a.execution=x.id) THEN 'chat' ELSE 'unclassified' END FROM executions x JOIN events e ON e.id=x.source JOIN event_locators l ON l.event_id=e.id WHERE l.native_id=? AND x.state='completed' ",
                            (str(mid),),
                        ).fetchone()
                        if seen:
                            break
                        if time.perf_counter() > deadline:
                            raise RuntimeError(
                                "no terminal original execution for " + key
                            )
                        time.sleep(0.002)
                    committed = [
                        r[0]
                        for r in db.execute(
                            "SELECT id FROM social_outbound WHERE namespace=? AND locator=?",
                            (seen[0], str(mid + 100000)),
                        )
                    ]
                    raw = {
                        "input": key,
                        "target": destination,
                        "packet_sha256": sha(pack(frame)),
                        "native_message_id": str(mid),
                        "reason": seen[1],
                        "warmup": key.startswith("warm-"),
                        "gateway": deliveries,
                        "event_ids": committed,
                        "seconds": time.perf_counter() - start_at,
                    }
                    rows.append(raw)
                    log.write(json.dumps(raw, ensure_ascii=False) + "\n")
                    log.flush()
                    calls = http.wires[wire_offset:]
                    timings.append(
                        {
                            "input": key,
                            "first_request_seconds": calls[0]["monotonic"] - start_at
                            if calls
                            else None,
                            "first_delivery_seconds": receive_at - start_at,
                            "attempts": len(calls),
                            "wire": calls,
                        }
                    )
                    expected.append(item)
        finally:
            (out / "storage.json").write_text(json.dumps(storage(data)) + "\n")
            peer.close()
            stop(server)
            if original is None:
                os.environ.pop("YUKI_SLICE_PEER", None)
            else:
                os.environ["YUKI_SLICE_PEER"] = original
    decision = decide({"C": rows}, expected)
    dataout = {
        "arm": "C",
        "process_start_seconds": boot_elapsed,
        "decision": decision,
        "sqlite": database,
        "wire_timings": timings,
        "wire_samples": http.samples,
        "warmup": warmup,
        "workload": "synchronous QQ slice only",
        "paid_calls": 0,
    }
    (out / "run.json").write_text(
        json.dumps(dataout, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    if not decision["external_effects_equal"]:
        raise RuntimeError("QQ effects differ from fixed oracle")
    return dataout


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--binary", type=Path, required=True)
    p.add_argument("--data", type=Path, required=True)
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--limit", type=int)
    p.add_argument("--warmup", type=int, default=0)
    p.add_argument("--frames", required=True, type=Path)
    p.add_argument("--frames-sha256", required=True)
    args = p.parse_args()
    run(
        args.binary.resolve(),
        args.data.resolve(),
        args.out.resolve(),
        args.frames,
        args.frames_sha256,
        args.limit,
        args.warmup,
    )
