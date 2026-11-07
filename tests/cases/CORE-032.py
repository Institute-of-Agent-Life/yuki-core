"""Use installed SDKs and live plugin processes. All external recipients are synthetic ledgers."""

import hashlib
import json
import os
import queue
import sqlite3
import subprocess
import sys
import time
import tomllib
from contextlib import closing
from tests.harness.external_search import exercise as external_search
from tests.harness.terminal_work import exercise as terminal_work
from tests.harness.native_topic_route import run as native_topic_route
from tests.harness.service import Service, previous_schema
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer as OneBot


def run(ctx, profile="debug"):
    svc = Service(ctx, ctx.working / "owner", profile)
    previous_schema(ctx, svc.data, 12)
    (svc.data / "SOUL.md").write_bytes((ctx.root / "character/SOUL.md").read_bytes())
    ctx.check(
        "additive upgrade to host reader",
        svc.command("migrate")["schema_version"] >= 13,
    )
    dist = ctx.output / "sdk-distributions"
    installed = ctx.working / "installed"
    build = subprocess.run(
        [
            sys.executable,
            str(ctx.root / "ci/publish-contract.py"),
            "build",
            "--out",
            str(dist),
            "--verify-work",
            str(installed),
        ],
        capture_output=True,
        text=True,
    )
    if build.returncode:
        ctx.write(
            "package-error.json", {"stderr": build.stderr, "stdout": build.stdout}
        )
    ctx.equal(
        "both actual SDK distributions install and crosswire", build.returncode, 0
    )
    rust_consumer = ctx.working / "rust-consumer"
    (rust_consumer / "src").mkdir(parents=True)
    crate = next((installed / "rust-install").glob("yuki-plugin-sdk-*"))
    (rust_consumer / "Cargo.toml").write_text(
        '[package]\nname="yuki-host-consumer"\nversion="0.0.0"\nedition="2024"\n[dependencies]\nserde_json="=1.0.145"\nyuki-plugin-sdk={path='
        + json.dumps(str(crate))
        + "}\n",
        encoding="utf-8",
    )
    (rust_consumer / "src/main.rs").write_bytes(
        (ctx.root / "tests/peers/host/rust.rs").read_bytes()
    )
    for cmd in (
        ["cargo", "generate-lockfile", "--offline"],
        [
            "cargo",
            "build",
            "--locked",
            "--offline",
            "--target-dir",
            str(installed / "rust-target"),
        ],
    ):
        env = {
            **os.environ,
            "RUSTUP_TOOLCHAIN": tomllib.loads(
                (ctx.root / "rust-toolchain.toml").read_text()
            )["toolchain"]["channel"],
        }
        result = subprocess.run(
            cmd, cwd=rust_consumer, capture_output=True, text=True, env=env
        )
        if result.returncode:
            ctx.write("rust-consumer-error.json", {"stderr": result.stderr})
        ctx.equal(
            "compile independent Rust business SDK consumer", result.returncode, 0
        )
    rust_binary = (
        installed
        / "rust-target/debug"
        / ("yuki-host-consumer.exe" if os.name == "nt" else "yuki-host-consumer")
    )
    sdkdir = installed / "python-install"
    program = ctx.root / "tests/peers/host/peer.py"
    actor = "00000000-0000-4000-8000-000000000032"

    def op(*args, ok=True):
        return svc.command("operator", *map(str, args), accepted=ok)

    def rows(sql, *params):
        with closing(
            sqlite3.connect((svc.data / "core.sqlite").as_uri() + "?mode=ro", uri=True)
        ) as db:
            return db.execute(sql, params).fetchall()

    def until(fn, desc):
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            if r := fn():
                return r
            time.sleep(0.025)
        raise AssertionError("timeout waiting for " + desc)

    def log(who):
        p = areas[who] / "observed.jsonl"
        if not p.exists():
            return []
        lines = p.read_text(encoding="utf-8").splitlines()
        return [json.loads(line) for line in lines if line.endswith("}")]

    def record(who, method, params):
        nonlocal request_id
        request_id += 1
        rid = request_id
        path = areas[who] / f"rpc-{rid}.json"
        tmp = path.with_suffix(".new")
        tmp.write_text(
            json.dumps(
                {"jsonrpc": "2.0", "id": rid, "method": method, "params": params}
            ),
            encoding="utf-8",
        )
        tmp.replace(path)
        return until(
            lambda: next(
                (
                    r["frame"]
                    for r in log(who)
                    if r["kind"] == "reply" and r["frame"]["id"] == rid
                ),
                None,
            ),
            method,
        )

    def frame(key, text, accept):
        return {
            "source": key,
            "native_id": key,
            "sender": "200",
            "occurred": 10,
            "kind": "message",
            "segments": [{"type": "text", "text": text}],
            "accept": accept,
        }

    def enable(who, active):
        return op("host", "enable", who, str(active).lower())

    def reply(calls=None):
        return {
            "choices": [
                {
                    "finish_reason": "tool_calls" if calls else "stop",
                    "message": {
                        "role": "assistant",
                        "content": None if calls else "internal",
                        **({"tool_calls": calls} if calls else {}),
                    },
                }
            ]
        }

    def tool(name, id, args):
        return {
            "type": "function",
            "id": id,
            "function": {"name": name, "arguments": json.dumps(args)},
        }

    def offline(who):
        return rows(
            "SELECT generation FROM host_installations WHERE plugin=? AND NOT online",
            who,
        )

    request_id = 30
    areas = {n: ctx.working / n for n in ("tg", "observer")}
    for p in areas.values():
        p.mkdir()
    with svc.running():
        ctx.write("sqlite-build.json", op("database-info"))
        op("person", "create", actor)
        a = op(
            "account",
            "open",
            "telegram",
            "telegram.fixture",
            "200",
            1,
            "--person",
            actor,
            "--start",
            1,
        )
        bot = op(
            "account", "open", "telegram", "telegram.fixture", "100", 1, "--start", 1
        )

        op(
            "account",
            "open",
            "qq",
            "native.fixture",
            "210",
            1,
            "--person",
            actor,
            "--start",
            1,
        )
        qq_bot = op("account", "open", "qq", "native.fixture", "110", 1, "--start", 1)[
            "assignment"
        ]

    def argv(name):
        return json.dumps(
            [str(program), str(areas[name]), str(sdkdir)], ensure_ascii=False
        )

    cfg = f"""
[[plugins]]
id="tg"
namespace="telegram.fixture"
platform="telegram"
bot_stage={json.dumps(bot["assignment"])}
program={json.dumps(sys.executable)}
args={argv("tg")}
erasure_args={argv("tg")}
env=["YUKI_HOST_TEST_PUBLIC"]
grants=["gateway.inbound","gateway.send","tool.call","command.invoke","forget.owner"]
commands=["/game","/game/play"]
[[plugins.tools]]
name="host_probe"
description="Read a synthetic scoped fixture."
parallel_read=false
cacheable=false
code_direct=true
schema={{type="object",properties={{value={{type="string"}}}},required=["value"],additionalProperties=false}}
[[plugins]]
id="observer"
namespace="observer.fixture"
program={json.dumps(sys.executable)}
args={argv("observer")}
grants=["observe.metadata","tool.call"]
"""
    rust_trace = ctx.output / "rust-peer-state.txt"
    cfg += f'\n[[plugins]]\nid="rust.sample"\nnamespace="rust.fixture"\nprogram={json.dumps(str(rust_binary))}\nenv=["YUKI_HOST_TEST_RUST_STATE"]\ngrants=["tool.call"]\n[[plugins.tools]]\nname="rust_host_probe"\ndescription="Probe installed Rust SDK."\nparallel_read=false\ncacheable=false\ncode_direct=true\nschema={{type="object",properties={{value={{type="string"}}}},required=["value"],additionalProperties=false}}\n'
    environment = {
        **os.environ,
        "YUKI_HOST_QQ_TEST": "fixture-native-qq",
        "YUKI_PARENT_SENTINEL": "fixture-hidden",
        "YUKI_HOST_TEST_PUBLIC": "fixture-public",
        "YUKI_HOST_TEST_RUST_STATE": str(rust_trace),
    }
    with Recorder() as peer:
        cfg += f'\n[main]\nendpoint="{peer.url}"\nmodel="fixture-host"\nname="Yuki"\nmerge_ms=0\nroot_slots=5\nmax_input_units=200000\nmax_output=128\n'
        cfg += f'\n[qq]\nnamespace="native.fixture"\ndesignated_ingress="110"\n[[qq.bots]]\nself_id="110"\nstage="{qq_bot}"\ntoken_env="YUKI_HOST_QQ_TEST"\n'
        with (svc.data / "yuki.toml").open("a") as f:
            f.write(cfg)
        with svc.running(environment=environment):
            for who in areas:
                ready = until(
                    lambda: next((r for r in log(who) if r["kind"] == "ready"), None),
                    "ready " + who,
                )
                ctx.equal(
                    "parent sentinel omitted from child env",
                    ready["parent_secret_present"],
                    False,
                )
                ctx.equal(
                    "per-installation allowlist only",
                    ready["allowlisted"],
                    "fixture-public" if who == "tg" else None,
                )
            until(lambda: rust_trace.is_file() and "negotiated" in rust_trace.read_text().splitlines(),
                  "Rust consumer negotiated its installed SDK")
            first = frame("obs", "private source material", False)
            observation = record("tg", "gateway.inbound", first)["result"]
            same = record("tg", "gateway.inbound", first)["result"]
            ctx.equal("original pure observation replays", same, observation)
            ctx.equal(
                "gateway receipt only after actual event commit",
                rows("SELECT id FROM events WHERE id=?", observation["event"]),
                [(observation["event"],)],
            )
            external_space = json.dumps(
                ["qq", "native.fixture", "700"], separators=(",", ":")
            )
            groupframe = {
                **frame("group-id", "topic", False),
                "space": external_space,
                "topic": "thread-1",
            }
            group = record("tg", "gateway.inbound", groupframe)["result"]
            original = rows(
                "SELECT d.space,d.topic FROM events e JOIN entries d ON d.id=e.entry_id WHERE e.id=?",
                group["event"],
            )[0]
            ctx.equal(
                "gateway native ID resembling a QQ group cannot enter that group",
                [json.loads(original[0]), original[1]],
                [["telegram", "telegram.fixture", external_space], "thread-1"],
            )
            ctx.equal(
                "same chat, distinct thread gets another original event",
                record("tg", "gateway.inbound", {**groupframe, "topic": "thread-2"})[
                    "result"
                ]["event"]
                != group["event"],
                True,
            )
            target = {"source": "obs", "sender": "200"}
            ctx.equal(
                "query original gateway result",
                record("tg", "gateway.status", target)["result"],
                observation,
            )
            ctx.check(
                "forged Person rejected",
                "error" in record("tg", "gateway.inbound", {**first, "person": actor}),
            )
            ctx.check(
                "foreign plugin cannot impersonate gateway",
                "error" in record("observer", "gateway.inbound", first),
            )
            ctx.check(
                "raw arbitrary QQ action is unavailable",
                "error"
                in record("tg", "qq.call_action", {"action": "any", "person": actor}),
            )
            cmd = record("tg", "gateway.inbound", frame("cmd", "/game/play now", True))[
                "result"
            ]
            ctx.equal(
                "longest complete command route selected",
                cmd["command"]["path"],
                "/game/play",
            )
            until(
                lambda: rows(
                    "SELECT state FROM host_command_events WHERE event=? AND state='complete'",
                    cmd["event"],
                ),
                "command completion",
            )
            enable("tg", False)
            # The platform can be offline while the original QQ/private operator submits the same bound command.
            offline_event = op(
                "ingress",
                "record",
                json.dumps(
                    {
                        "source": "offline-cmd",
                        "account": a["account"],
                        "bot_stage": bot["assignment"],
                        "observed": 12,
                        "occurred": 10,
                        "kind": "message",
                        "segments": [{"type": "text", "text": "/game/play again"}],
                    }
                ),
                "--accept",
            )
            ctx.equal(
                "offline bound command consumed as unavailable",
                offline_event["command"],
                {"path": "/game/play", "state": "unavailable"},
            )
            ctx.check(
                "no model request for either command",
                peer.wires.empty()
                and rows("SELECT count(*) FROM model_attempts") == [(0,)],
            )
            native = OneBot(svc.address, "110", "fixture-native-qq")
            try:
                native.send(
                    {
                        "post_type": "message",
                        "message_type": "private",
                        "sub_type": "friend",
                        "message_id": 321,
                        "user_id": 210,
                        "self_id": 110,
                        "time": 10,
                        "message": [
                            {"type": "text", "data": {"text": "/game/play native"}}
                        ],
                    }
                )
                outbound = native.receive()
                while outbound["action"] != "send_private_msg":
                    native.send(
                        {
                            "echo": outbound["echo"],
                            "status": "ok",
                            "retcode": 0,
                            "data": {},
                        }
                    )
                    outbound = native.receive()
                ctx.check(
                    "actual QQ peer receives offline command disposition on original destination",
                    outbound["params"]["user_id"] == 210
                    and "暫時不可用"
                    in json.dumps(outbound["params"], ensure_ascii=False),
                )
                native.send(
                    {
                        "echo": outbound["echo"],
                        "status": "ok",
                        "retcode": 0,
                        "data": {"message_id": 7321},
                    }
                )
                until(
                    lambda: rows("SELECT id FROM social_outbound WHERE locator='7321'"),
                    "original QQ command delivery receipt",
                )
                ctx.check("no model call for native QQ command", peer.wires.empty())
            finally:
                native.close()
            enable("tg", True)
            until(
                lambda: rows(
                    "SELECT event FROM host_command_events WHERE event=? AND reply_state='confirmed'",
                    offline_event["event"],
                ),
                "actual unavailable reply on original source route",
            )
            ctx.check(
                "gateway performed visible command-unavailable send",
                any(
                    "暫時不可用" in v
                    for v in (areas["tg"] / "deliveries.jsonl")
                    .read_text(encoding="utf-8")
                    .splitlines()
                ),
            )
            ctx.equal(
                "consumed command body omitted from history",
                rows(
                    "SELECT visible FROM history_items WHERE source_event=?",
                    offline_event["event"],
                ),
                [(0,)],
            )
            delivery_start = len(
                (areas["tg"] / "deliveries.jsonl")
                .read_text(encoding="utf-8")
                .splitlines()
            )
            incoming = record(
                "tg", "gateway.inbound", frame("main", "ordinary chat", True)
            )["result"]
            raw, q = peer.receive()
            ctx.check(
                "real paid wire declares installed plugin tool",
                any(
                    t["function"]["name"] == "host_probe"
                    for t in json.loads(raw)["tools"]
                ),
            )
            q.put((reply([tool("host_probe", "call-1", {"value": "result"})]), 0))
            follow, q = peer.receive()
            r = json.loads(json.loads(follow)["messages"][-1]["content"])
            ctx.equal(
                "reverse status was processed during pending host call",
                r["result"]["data"]["status"]["state"],
                "dispatched",
            )
            handle = r["result"]["data"]["status"]["handle"]
            ctx.check(
                "a different plugin with same method grant cannot read call",
                "error" in record("observer", "tool.status", {"handle": handle}),
            )
            ctx.check(
                "caller-supplied superuser cannot elevate",
                "error"
                in record("tg", "tool.status", {"handle": handle, "admin": True}),
            )
            q.put((reply(), 0))
            # Queueing an HTTP reply is not a commit. Each independent SDK scenario
            # starts on a new turn, after the preceding owner has actually settled.
            until(lambda: rows("SELECT id FROM executions WHERE source=? AND state='completed'", incoming["event"]),
                  "original Python SDK turn settled")
            rust_source = record(
                "tg",
                "gateway.inbound",
                frame("rust", "second independent consumer", True),
            )["result"]
            ctx.check("original human admitted for Rust SDK", rust_source["accepted"])
            raw, q = peer.receive()
            ctx.check("Rust probe answers the second original model request",
                      "second independent consumer" in json.dumps(json.loads(raw)["messages"]))
            q.put((reply([tool("rust_host_probe", "rust-1", {"value": "rust"})]), 0))
            try:
                follow, q = peer.receive()
            except Exception:
                ctx.write("host-rust-followup-diagnostic.json", {
                    "executions": rows("SELECT id,state FROM executions"),
                    "turns": rows("SELECT execution,diagnostic FROM main_turns"),
                    "intents": rows("SELECT * FROM model_tool_intents"),
                    "budgets": rows("SELECT * FROM root_budgets"),
                    "requests": rows("SELECT id,root,estimated_input,max_output FROM model_requests"),
                    "attempts": rows("SELECT id,state,diagnostic FROM model_attempts"),
                    "calls": rows("SELECT method,state FROM host_calls"),
                })
                raise
            rust_envelope = json.loads(json.loads(follow)["messages"][-1]["content"])
            if "data" not in rust_envelope.get("result", {}):
                ctx.write("host-rust-result-diagnostic.json", {
                    "result": rust_envelope,
                    "executions": rows("SELECT id,state FROM executions"),
                    "turns": rows("SELECT execution,diagnostic FROM main_turns"),
                    "calls": rows("SELECT plugin,method,state FROM host_calls"),
                    "plugins": rows("SELECT plugin,enabled,online,generation FROM host_installations"),
                })
            rust_result = rust_envelope["result"]["data"]
            ctx.check(
                "installed Rust SDK handles interleaved opposite-direction status",
                rust_result["rust_sdk"]
                and rust_result["status"]["state"] == "dispatched"
                and rust_result["original"] == {"value": "rust"},
            )
            q.put((reply(), 0))
            until(lambda: rows("SELECT id FROM executions WHERE source=? AND state='completed'", rust_source["event"]),
                  "original Rust SDK turn settled")
            (areas["tg"] / "cut-second").touch()
            second = record("tg", "gateway.inbound", frame("send", "send test", True))[
                "result"
            ]
            raw, q = peer.receive()
            recipient, entry = rows("SELECT raw_actor,entry_id FROM events WHERE id=?", second["event"])[0]
            wrong = {"kind":"space","target_id":"ffffffff-ffff-4fff-8fff-ffffffffffff","conversation_id":entry}
            q.put((reply([tool("send","wrong-gateway-kind",{"text":"must not deliver","target":wrong})]),0))
            denied,q=peer.receive()
            refused=json.loads(next(m["content"] for m in json.loads(denied)["messages"] if m.get("tool_call_id")=="wrong-gateway-kind"))
            ctx.equal("private gateway cannot match a space through absent native IDs",refused["state"],"failed")
            ctx.equal("invalid gateway destination did not reach the plugin",len((areas["tg"]/"deliveries.jsonl").read_text().splitlines()),delivery_start)
            valid={"kind":"person","target_id":recipient,"conversation_id":entry,"binding_id":"ffffffff-ffff-4fff-8fff-ffffffffffff"}
            q.put((reply([tool("send","wire-send",{"text":"one|two|three","target":valid})]),0))
            until(lambda: offline("tg"), "lost physical receipt")
            parts = rows(
                "SELECT c.state,c.operation FROM host_calls c JOIN executions x ON x.id=c.execution WHERE x.source=? AND c.method='gateway.dispatch_part' ORDER BY c.rowid",
                second["event"],
            )
            ctx.equal(
                "confirmed first part, unknown second, third never sent",
                [p[0] for p in parts],
                ["confirmed", "unknown"],
            )
            ctx.equal(
                "physical peer received the two original part IDs",
                [
                    json.loads(v)["operation"]
                    for v in (areas["tg"] / "deliveries.jsonl")
                    .read_text()
                    .splitlines()[delivery_start:]
                ],
                [p[1] for p in parts],
            )
            # If the runner has a partial receipt, release its paid continuation.
            try:
                _, _, pending = peer.wires.get(timeout=0.4)
                pending.put((reply(), 0))
            except queue.Empty:
                pass
            (areas["tg"] / "cut-second").unlink()
            op("host", "restart", "tg")
            ctx.equal(
                "restart preserved both original outcomes",
                rows(
                    "SELECT state FROM host_calls WHERE operation IN (?,?) ORDER BY rowid",
                    *(p[1] for p in parts),
                ),
                [(p[0],) for p in parts],
            )
            ctx.equal(
                "no resend on reconnection",
                len((areas["tg"] / "deliveries.jsonl").read_text().splitlines()),
                2 + delivery_start,
            )
            (areas["observer"] / "stall-observer").touch()
            first_pid = next(r["pid"] for r in log("observer") if r["kind"] == "ready")
            for n in range(68):
                response = record(
                    "tg",
                    "gateway.inbound",
                    frame(f"slow-{n}", "observer cannot block ingress", False),
                )
                if "result" not in response:
                    raise AssertionError("slow observer stalled ingress")
            old_last = response["result"]["event"]
            ctx.check(
                "observer queue bounded despite unread peer",
                rows("SELECT count(*) FROM host_observations WHERE plugin='observer'")[
                    0
                ][0]
                <= 64,
            )
            op("host", "restart", "observer")
            (areas["observer"] / "stall-observer").unlink()
            fresh = record(
                "tg",
                "gateway.inbound",
                frame("after-restart", "private-body-marker", False),
            )["result"]["event"]
            observed = until(
                lambda: next(
                    (
                        r
                        for r in log("observer")
                        if r.get("method") == "observer.event"
                        and r["params"]["event"] == fresh
                    ),
                    None,
                ),
                "new-generation observation",
            )
            ctx.check(
                "fresh peer gets filtered metadata only",
                observed["pid"] != first_pid
                and "private-body-marker" not in json.dumps(observed)
                and set(observed["params"]) <= {"event", "kind", "entry"},
            )
            ctx.check(
                "old observer queue never crosses instance",
                all(
                    r["params"]["event"] > old_last
                    for r in log("observer")
                    if r["pid"] == observed["pid"]
                    and r.get("method") == "observer.event"
                ),
            )
            op("privacy", "acquire", "tg", "object", observation["event"])
            (areas["tg"] / "private.item").write_text(
                "private source material", encoding="utf-8"
            )
            (areas["tg"] / "drop-forget").touch()
            erased = op(
                "privacy",
                "erase",
                "forget-original",
                actor,
                json.dumps([observation["event"]]),
            )
            until(
                lambda: offline("tg") and (areas["tg"] / "erased.json").exists(),
                "forget interrupted before ACK",
            )
            ctx.check(
                "lost ACK leaves original owner obligation",
                op("privacy", "pending", "tg")["obligations"],
            )
            (areas["tg"] / "drop-forget").unlink()
            op("host", "restart", "tg")
            until(
                lambda: rows(
                    "SELECT watermark FROM owner_erasures WHERE owner='tg' AND proof IS NOT NULL"
                ),
                "original ACK",
            )
            ctx.check(
                "same owner proof recovered without source material",
                not (areas["tg"] / "private.item").exists(),
            )
            (areas["tg"] / "hold-tool").touch()
            revoked = record(
                "tg", "gateway.inbound", frame("revoke", "access ended", True)
            )["result"]
            raw, q = peer.receive()
            q.put(
                (
                    reply(
                        [
                            tool(
                                "host_probe",
                                "held-in-plugin",
                                {"value": "private-response-marker"},
                            )
                        ]
                    ),
                    0,
                )
            )
            ctx.check(
                "revocation scenario uses fresh accepted human event",
                revoked["accepted"],
            )
            held = until(
                lambda: next((v for v in log("tg") if v["kind"] == "holding"), None),
                "plugin holding result",
            )
            enable("tg", False)
            until(
                lambda: rows(
                    "SELECT state FROM host_calls WHERE handle=? AND state='unknown'",
                    held["handle"],
                ),
                "revoke after dispatch",
            )
            ctx.equal(
                "old instance cannot publish held body",
                rows(
                    "SELECT state,result FROM host_calls WHERE handle=?", held["handle"]
                ),
                [("unknown", None)],
            )
            ctx.equal(
                "confirmed partial gateway delivery survives revocation",
                rows("SELECT state FROM operations WHERE operation_id=?", parts[0][1]),
                [("confirmed",)],
            )
            (areas["tg"] / "hold-tool").unlink()
            try:
                _, _, pending = peer.wires.get(timeout=0.3)
                pending.put((reply(), 0))
            except queue.Empty:
                pass
            statuses = op("host", "status")
    return {
        "native_topic_route": native_topic_route(ctx, sdkdir, profile),
        "external_search": external_search(ctx,sdkdir,profile),
        "terminal_work":terminal_work(ctx,sdkdir,profile),
        "normal": {
            "observation": observation,
            "command": cmd,
            "ordinary": incoming,
            "reverse": r,
            "installed": statuses,
        },
        "faults": {
            "offline_command": offline_event,
            "parts": parts,
            "physical_receipts": [
                json.loads(v)
                for v in (areas["tg"] / "deliveries.jsonl").read_text().splitlines()
            ],
            "new_observer": observed,
            "forget": erased,
        },
        "binary_sha256": hashlib.sha256(svc.binary.read_bytes()).hexdigest(),
        "host_idl_sha256": hashlib.sha256(
            (ctx.root / "protocol/plugin/gateway/host-preview.json").read_bytes()
        ).hexdigest(),
        "sdk_versions": json.loads((dist/"candidate.json").read_bytes())["sdk_versions"],
        "rust_consumer": {
            "binary_sha256": hashlib.sha256(rust_binary.read_bytes()).hexdigest(),
            "response": rust_result,
        },
        "source": "synthetic external plugin child processes; no stable extension acceptance",
    }
