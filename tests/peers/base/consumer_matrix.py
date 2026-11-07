"""Replay the installed Telegram package through the original public Host entry points."""

from contextlib import contextmanager
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time
import uuid

from tests.peers.base.joint import exchange, free_address, wait_until, write


def replay(host, plugin, output, work, scenario):
    output.mkdir()
    data = work / "owner"
    person = str(uuid.uuid4())
    text = "雪" * 4095 + "🌨" + "part-two" * 513
    models, native, offsets, faults = [], [], [], []
    finished = threading.Event()
    env = dict(
        os.environ,
        NO_PROXY="127.0.0.1,localhost",
        no_proxy="127.0.0.1,localhost",
        YUKI_TELEGRAM_BOT_ID="101",
        YUKI_TELEGRAM_TOKEN="101:fixture",
        YUKI_TELEGRAM_STATE=str(work / "cursor"),
    )

    class Endpoint(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def reply(self, body):
            raw = json.dumps(body).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

        def do_POST(self):
            try:
                self.dispatch()
            except ConnectionError:
                pass
            except Exception as error:
                faults.append(repr(error))
                finished.set()
                self.send_error(500)

        def dispatch(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            if self.path == "/bot101:fixture/getMe":
                return self.reply(
                    {
                        "ok": True,
                        "result": {
                            "id": 101,
                            "username": "OriginalBot",
                            "is_bot": True,
                        },
                    }
                )
            if self.path == "/bot101:fixture/getUpdates":
                offset = body.get("offset", 0)
                offsets.append(offset)
                if offset > 41:
                    time.sleep(0.08)
                event = {
                    "update_id": 41,
                    "message": {
                        "message_id": 9,
                        "date": int(time.time()),
                        "from": {"id": 8},
                        "chat": {"id": 8, "type": "private"},
                        "text": "send the original text",
                    },
                }
                return self.reply(
                    {"ok": True, "result": [] if offset > 41 else [event]}
                )
            if self.path == "/model":
                models.append(body)
                message = {"role": "assistant", "content": "finished"}
                if len(models) == 1:
                    message.update(
                        content=None,
                        tool_calls=[
                            {
                                "id": "original-send",
                                "type": "function",
                                "function": {
                                    "name": "send",
                                    "arguments": json.dumps({"text": text}),
                                },
                            }
                        ],
                    )
                self.reply(
                    {
                        "choices": [
                            {
                                "message": message,
                                "finish_reason": (
                                    "tool_calls" if len(models) == 1 else "stop"
                                ),
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 2,
                            "completion_tokens": 1,
                            "total_tokens": 3,
                        },
                    }
                )
                if len(models) == 2:
                    finished.set()
                return
            if self.path == "/bot101:fixture/sendMessage":
                assert body["chat_id"] == "8", body
                native.append(body)
                if scenario == "lost-second" and len(native) == 2:
                    self.close_connection = True
                    self.connection.shutdown(socket.SHUT_RDWR)
                    return
                return self.reply(
                    {
                        "ok": True,
                        "result": {
                            "message_id": 700 + len(native),
                            "chat": {"id": 8},
                        },
                    }
                )
            raise AssertionError("unexpected native request " + self.path)

    def cli(*args, check=True):
        result = subprocess.run(
            [str(host), "--data", str(data), *map(str, args)],
            cwd=work,
            env=env,
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=20,
        )
        if result.returncode:
            if check:
                raise RuntimeError(result.stderr)
            return None
        return json.loads(result.stdout) if result.stdout.strip() else None

    @contextmanager
    def running():
        with (output / "host.log").open("ab") as log:
            proc = subprocess.Popen(
                [str(host), "--data", str(data), "run"],
                cwd=work,
                env=env,
                stdout=log,
                stderr=log,
            )
        try:

            def ready():
                if proc.poll() is not None:
                    raise RuntimeError(
                        (output / "host.log").read_text(encoding="utf-8")
                    )
                return cli("operator", "database-info", check=False)

            wait_until(ready, 15)
            yield
        finally:
            if proc.poll() is None:
                write(
                    output / "host-status.json",
                    cli("operator", "host", "status", check=False),
                )
                cli("operator", "shutdown", check=False)
                try:
                    proc.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()

    with ThreadingHTTPServer(("127.0.0.1", 0), Endpoint) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        api = "http://127.0.0.1:" + str(server.server_port)
        env["YUKI_TELEGRAM_API"] = api
        try:
            cli("init", "--plugin-base-1", "--memory-mode", "lexical-only")
            config = data / "yuki.toml"
            top = config.read_text(encoding="utf-8").replace(
                '"127.0.0.1:0"', json.dumps(free_address())
            )
            top += (
                "\n[main]\nendpoint="
                + json.dumps(api + "/model")
                + '\nmodel="fixture"\nname="Yuki"\nmax_output=12000\nroot_slots=8\n'
                + "root_tool_calls=8\nmerge_ms=1\nmax_input_units=1200000\n"
            )
            config.write_text(top, encoding="utf-8")
            with running():
                database_info = cli("operator", "database-info")
                cli("operator", "person", "create", person)
                cli(
                    "operator",
                    "account",
                    "open",
                    "telegram",
                    "tg.compat",
                    "8",
                    1,
                    "--person",
                    person,
                    "--start",
                    1,
                )
                stages = {
                    namespace: cli(
                        "operator",
                        "account",
                        "open",
                        "telegram",
                        namespace,
                        "101",
                        1,
                        "--start",
                        1,
                    )["assignment"]
                    for namespace in ("tg.compat", "too-old", "missing-field")
                }
            for name, mutation in [
                ("healthy", "none"),
                ("too-old", "unsupported-version"),
                ("missing-field", "missing-required-field"),
            ]:
                state = work / name
                state.mkdir()
                namespace = "tg.compat" if name == "healthy" else name
                row = {
                    "id": name,
                    "namespace": namespace,
                    "program": sys.executable,
                    "args": [
                        str(Path(__file__).with_name("relay.py")),
                        str(plugin),
                        str(output / (name + ".jsonl")),
                        str(state),
                        mutation,
                    ],
                    "platform": "telegram",
                    "bot_stage": stages[namespace],
                    "grants": ["gateway.inbound", "gateway.send"],
                    "env": [k for k in env if k.startswith("YUKI_TELEGRAM_")],
                }
                top += (
                    "\n[[plugins]]\n"
                    + "\n".join(k + "=" + json.dumps(v) for k, v in row.items())
                    + "\n"
                )
            config.write_text(top, encoding="utf-8")
            with running():
                assert finished.wait(35), "Main never received its original receipt"
                assert not faults, faults
                receipt = json.loads(
                    next(
                        m["content"]
                        for m in reversed(models[-1]["messages"])
                        if m.get("tool_call_id") == "original-send"
                    )
                )
                expected = "unknown" if scenario == "lost-second" else "confirmed"
                assert receipt["state"] == expected, receipt
                wait_until(lambda: 42 in offsets)

                def isolated():
                    status = cli("operator", "host", "status")["plugins"]
                    values = {v["plugin"]: v["online"] for v in status}
                    return values == {
                        "healthy": True,
                        "too-old": False,
                        "missing-field": False,
                    }

                wait_until(isolated, 15)
            before = len(native), len(models)
            position = len(offsets)
            with running():
                assert (
                    cli(
                        "operator",
                        "effect",
                        "read",
                        receipt["namespace"],
                        receipt["id"],
                        person,
                    )
                    == receipt
                )
                wait_until(lambda: offsets[position:].count(42) >= 2)
            assert before == (
                len(native),
                len(models),
            ), "cold restart dispatched another effect"
            count = 2 if scenario == "lost-second" else 3
            assert len(native) == count
            assert [row["text"] for row in native] == [
                text[i : i + 4096] for i in range(0, len(text), 4096)
            ][:count]
            traces = {
                name: [
                    json.loads(line)
                    for line in (output / (name + ".jsonl")).read_bytes().splitlines()
                ]
                for name in ("healthy", "too-old", "missing-field")
            }
            requests = list(
                exchange(traces["healthy"], "gateway.dispatch_part", "from_host")
            )
            assert len(requests) == count
            assert [reply["result"]["state"] for _, reply in requests] == (
                ["confirmed", "unknown"]
                if scenario == "lost-second"
                else ["confirmed"] * 3
            )
            for name in ("too-old", "missing-field"):
                assert any(row["direction"] == "before_fault" for row in traces[name])
                assert not any(
                    row["frame"].get("method") == "gateway.inbound"
                    for row in traces[name]
                )
            rejected = list(exchange(traces["too-old"], "initialize", "from_plugin"))
            assert rejected and all("error" in reply for _, reply in rejected)
            altered = [
                row["frame"]
                for row in traces["missing-field"]
                if row["direction"] == "from_host"
            ]
            assert altered and all(
                "namespace" not in row.get("result", {}) for row in altered
            )
            proof = {
                "status": "PASS",
                "scenario": scenario,
                "state": expected,
                "native_parts": len(native),
                "post_restart_effects": 0,
                "receipt": receipt,
                "host_sha256": hashlib.sha256(host.read_bytes()).hexdigest(),
                "consumer_sha256": hashlib.sha256(plugin.read_bytes()).hexdigest(),
                "database_info": database_info,
                "isolated_rejections": {
                    "too-old": rejected[0][1]["error"],
                    "missing-field": "native SDK rejected missing initialize namespace",
                },
                "paid_calls": 0,
            }
            write(output / "result.json", proof)
            return proof
        finally:
            server.shutdown()
            thread.join()
            write(output / "native.json", native)
            write(output / "models.json", models)
            write(output / "faults.json", faults)
