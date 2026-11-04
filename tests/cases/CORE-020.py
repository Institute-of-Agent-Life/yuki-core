"""Exercise the Rust Messages runner over physical HTTP, with no remote API calls."""

import json
import os
import sqlite3
import time
from contextlib import closing
from tests.harness.service import Service
from tests.peers.chat.recorder import Recorder


def run(ctx):
    oracle = json.loads(
        (ctx.root / "fixtures/providers/messages/v4-original.json").read_text()
    )
    ctx.equal(
        "original source",
        oracle["source_commit"],
        "329ca7c487414d2f54ddd98ff4727446ed2446cd",
    )
    usage = {
        "input_tokens": 20,
        "cache_read_input_tokens": 8,
        "cache_creation_input_tokens": 3,
        "cache_creation": {"ephemeral_5m_input_tokens": 3},
        "output_tokens": 7,
        "output_tokens_details": {"thinking_tokens": 2},
    }

    def answer(blocks, stop):
        return {
            "type": "message",
            "role": "assistant",
            "id": "msg-original",
            "content": blocks,
            "stop_reason": stop,
            "usage": usage,
        }

    def call(name, id, args):
        return {"type": "tool_use", "id": id, "name": name, "input": args}

    signature = {
        "type": "thinking",
        "thinking": "synthetic private computation",
        "signature": "sig/original+==",
    }
    hidden = {"type": "redacted_thinking", "data": "opaque-payload"}
    txt = {"type": "text", "text": "internal only"}
    runs = []
    failures = []
    profiles = [
        (
            w["thinking_enabled"],
            w["requested_choice"].replace("inspect", "get"),
            w["body"],
        )
        for w in oracle["wires"]
    ] + [(True, "auto", None)]
    for num, (thinking, choice, base) in enumerate(profiles):
        svc = Service(ctx, ctx.working / f"profile-{num}")
        svc.command("init", "--memory-mode", "lexical-only")

        def op(*args):
            return svc.command("operator", *map(str, args))

        def rows(sql, *args):
            with closing(
                sqlite3.connect(
                    (svc.data / "core.sqlite").as_uri() + "?mode=ro", uri=True
                )
            ) as db:
                return db.execute(sql, args).fetchall()

        def state(run, name):
            until = time.monotonic() + 10
            while rows("SELECT state FROM executions WHERE id=?", run) != [(name,)]:
                if time.monotonic() > until:
                    raise AssertionError(f"execution did not become {name}: {run}")
                time.sleep(0.025)

        actor = "00000000-0000-4000-8000-000000000020"
        with svc.running():
            if num == 0:
                ctx.write("sqlite-build.json", op("database-info"))
            op("person", "create", actor)
            who = op(
                "account",
                "open",
                "qq",
                "test",
                "200",
                1,
                "--person",
                actor,
                "--start",
                1,
            )["assignment"]
            bot = op("account", "open", "qq", "test", "100", 1, "--start", 1)[
                "assignment"
            ]
            entry = op("entry", "private", who, bot)["entry"]
        with Recorder("/v1/messages", binding_status=503) as peer:
            extra = 'reasoning_effort="low"\n' if thinking else ""
            with (svc.data / "yuki.toml").open("a") as f:
                f.write(
                    f'\n[main]\nprotocol="anthropic_messages"\nendpoint="{peer.url}"\nbinding_endpoint="http://127.0.0.1:{peer.server.server_port}/binding"\nkey_env="YUKI_TEST_MESSAGES_KEY"\nmodel="fixture"\ntool_choice="{choice}"\n{extra}native_web_search={str(choice == "auto").lower()}\nname="Yuki"\nmerge_ms=0\nroot_slots=4\nmax_input_units=200000\nmax_output=8192\n'
                )
            with svc.running(
                environment={
                    **os.environ,
                    "YUKI_TEST_MESSAGES_KEY": "synthetic-test-only",
                }
            ):
                op("main", "register", "plugin", "source", entry)

                def trigger(key, structured=None):
                    body = {"text": key}
                    if structured:
                        body["structured"] = structured
                    return op("main", "trigger", "source", key, json.dumps(body))[
                        "execution"
                    ]

                def receive():
                    raw, q = peer.receive()
                    return json.loads(raw), q

                run = trigger("roundtrip")
                first, q = receive()
                if base:
                    expected = {**base["tool_choice"]}
                    if expected.get("name") == "inspect":
                        expected["name"] = "get"
                    ctx.equal(
                        "logical choice matches original physical adapter",
                        first["tool_choice"],
                        expected,
                    )
                    ctx.equal(
                        "thinking retains original setting",
                        first.get("thinking"),
                        base.get("thinking"),
                    )
                ctx.equal(
                    "native headers use version and API-key auth only",
                    peer.header_shapes[-1],
                    {"version": "2023-06-01", "api_key": True, "bearer": False},
                )
                blocks = [
                    signature,
                    hidden,
                    call("get", "origin-read", {"target": "k"}),
                ]
                if choice != "get":
                    blocks.append(
                        call("put", "origin-write", {"target": "k", "value": "written"})
                    )
                if choice == "auto":
                    blocks.append(
                        call(
                            "complete", "origin-failed", {"summary": "fixture rejects"}
                        )
                    )
                q.put((answer(blocks, "tool_use"), 0))
                second, q = receive()
                ctx.equal(
                    "original signed and redacted blocks replay without edits",
                    second["messages"][-2],
                    {"role": "assistant", "content": blocks},
                )
                results = second["messages"][-1]["content"]
                ctx.equal(
                    "tool results in one user turn with original IDs",
                    [r["tool_use_id"] for r in results],
                    [b["id"] for b in blocks if b["type"] == "tool_use"],
                )
                if choice == "auto":
                    ctx.equal(
                        "uncertain physical binding returns typed tool error",
                        results[-1]["is_error"],
                        True,
                    )
                q.put((answer([txt], "end_turn"), 0))
                state(run, "completed" if choice == "auto" else "held")
                raw = rows(
                    "SELECT usage_json FROM model_attempts ORDER BY rowid LIMIT 1"
                )[0][0]
                ctx.equal(
                    "original cache write and read included once in prompt total",
                    tuple(
                        json.loads(raw)[key]
                        for key in (
                            "input",
                            "cache",
                            "cache_creation",
                            "reasoning",
                            "output",
                            "total",
                        )
                    ),
                    (31, 8, 3, 2, 7, 38),
                )
                runs.append(
                    {
                        "profile": [thinking, choice],
                        "original": first,
                        "next": second,
                        "inspection": op("main", "inspect", run),
                        "usage": json.loads(raw),
                    }
                )
                task = {
                    "mode": "function_tool",
                    "schema": {
                        "type": "object",
                        "properties": {"answer": {"type": "integer"}},
                        "required": ["answer"],
                        "additionalProperties": False,
                    },
                }
                invalid = [
                    ("zero", [txt], "end_turn"),
                    ("wrong", [call("get", "x", {"target": "k"})], "tool_use"),
                    (
                        "two",
                        [
                            call("emit_result", "x", {"answer": 1}),
                            call("emit_result", "y", {"answer": 2}),
                        ],
                        "tool_use",
                    ),
                ]
                for key, blocks, stop in (
                    invalid
                    if choice == "auto"
                    else [
                        (
                            "valid",
                            [call("emit_result", "ok", {"answer": 1})],
                            "tool_use",
                        )
                    ]
                ):
                    run = trigger("structured-" + key, task)
                    w, q = receive()
                    ctx.equal(
                        "function-tool exposes only emit_result",
                        [t["name"] for t in w["tools"]],
                        ["emit_result"],
                    )
                    ctx.equal(
                        "logical named projection for structured task",
                        w["tool_choice"],
                        {"type": "auto"}
                        if thinking
                        else {"type": "tool", "name": "emit_result"},
                    )
                    q.put((answer(blocks, stop), 0))
                    state(run, "completed" if key == "valid" else "held")
                    checked = op("main", "inspect", run)
                    ctx.equal(
                        "one physical attempt with no implicit repair",
                        len(checked["attempts"]),
                        1,
                    )
                    ctx.check(
                        "original invalid response retains reported usage",
                        bool(
                            rows(
                                "SELECT usage_json FROM model_attempts t JOIN model_requests r ON r.id=t.logical JOIN activations a ON a.id=r.activation WHERE a.execution=? AND json_extract(t.usage_json,'$.output')=7",
                                run,
                            )
                        ),
                    )
                    failures.append(
                        {
                            "case": key,
                            "wire_choice": w["tool_choice"],
                            "execution": checked,
                        }
                    )
                if choice == "auto":
                    run = trigger("native-paused")
                    w, q = receive()
                    server = {
                        "type": "server_tool_use",
                        "id": "server1",
                        "name": "web_search",
                        "input": {"query": "offline"},
                    }
                    paused = [signature, server]
                    q.put((answer(paused, "pause_turn"), 0))
                    follow, q = receive()
                    ctx.equal(
                        "paused server call replays as assistant without local result",
                        follow["messages"][-1],
                        {"role": "assistant", "content": paused},
                    )
                    native_error = {
                        "type": "web_search_tool_result",
                        "tool_use_id": "server1",
                        "content": {
                            "type": "web_search_tool_result_error",
                            "error_code": "unavailable",
                        },
                    }
                    q.put((answer([native_error, txt], "end_turn"), 0))
                    state(run, "completed")
                    budget = rows(
                        "SELECT count(distinct r.id),count(distinct r.root),sum(r.native_tools) FROM model_requests r JOIN activations a ON a.id=r.activation WHERE a.execution=?",
                        run,
                    )
                    ctx.equal(
                        "common runner reserves both native attempts in same root",
                        budget,
                        [(2, 1, 2)],
                    )
                    ctx.equal(
                        "server tools never execute as local operations",
                        rows("SELECT count(*) FROM operations WHERE namespace=?", run),
                        [(0,)],
                    )
                    final = op("main", "inspect", run)
                    ctx.equal(
                        "native typed failure remains private tool event",
                        final["result"]["native_events"][0]["status"],
                        "failed",
                    )
                    runs.append({"pause": follow, "inspection": final, "root": budget})
                    for key, reply in [
                        (
                            "duplicate",
                            answer(
                                [
                                    call("put", "dup", {"target": "x", "value": 1}),
                                    call("put", "dup", {"target": "x", "value": 2}),
                                ],
                                "tool_use",
                            ),
                        ),
                        (
                            "truncated",
                            answer(
                                [call("put", "unsafe", {"target": "x", "value": 9})],
                                "max_tokens",
                            ),
                        ),
                        ("foreign-server-id", answer([native_error, txt], "end_turn")),
                    ]:
                        run = trigger(key)
                        _, q = receive()
                        q.put((reply, 0))
                        state(run, "held")
                        ctx.equal(
                            "incomplete or unpaired blocks produce no local effect",
                            rows(
                                "SELECT count(*) FROM operations WHERE namespace=?", run
                            ),
                            [(0,)],
                        )
                    run = trigger("native-schema", {**task, "mode": "json_schema"})
                    wire, q = receive()
                    ctx.equal(
                        "schema mode uses native output_config without local tools",
                        [wire["output_config"]["format"], wire.get("tools")],
                        [{"type": "json_schema", "schema": task["schema"]}, None],
                    )
                    q.put(
                        (
                            answer(
                                [{"type": "text", "text": '{"answer":3}'}], "end_turn"
                            ),
                            0,
                        )
                    )
                    state(run, "completed")
                    run = trigger(
                        "unsupported-text-json", {**task, "mode": "text_json"}
                    )
                    state(run, "held")
                    ctx.equal(
                        "unsupported wire shape costs no physical attempt",
                        len(op("main", "inspect", run)["attempts"]),
                        0,
                    )
                    run = trigger("exhausted-pause")
                    for i in range(4):
                        _, q = receive()
                        q.put(
                            (answer([{**server, "id": f"server-{i}"}], "pause_turn"), 0)
                        )
                    state(run, "held")
                    ctx.equal(
                        "repeated pauses cannot exceed original root slots",
                        len(op("main", "inspect", run)["attempts"]),
                        4,
                    )
    ctx.write(
        "messages-choice-pause.json",
        {"cases": {"H18": {"profiles": runs, "validation": failures}}},
    )
    return {
        "original_commit": oracle["source_commit"],
        "profiles": runs,
        "structured": failures,
        "endpoint": "loopback only",
    }
