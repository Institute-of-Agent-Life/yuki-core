"""Send physical Responses requests through the same Rust runner with three distinct selected dialects."""

import json
import queue
import sqlite3
import time
from contextlib import closing
from tests.harness.service import Service
from tests.peers.chat.recorder import Recorder


def run(ctx):
    corpus = json.loads(
        (ctx.root / "fixtures/providers/responses/dialects.json").read_text()
    )
    oracle = json.loads(
        (ctx.root / "fixtures/providers/responses/v4-original.json").read_text()
    )
    ctx.equal(
        "wire capture tied to fixed baseline",
        oracle["source_commit"],
        corpus["oracle"]["source_commit"],
    )
    old = oracle["adapter_bodies"]
    ctx.check(
        "real v4 DeepSeek body omits requested tool choice",
        "tool_choice" not in old["DeepSeekResponsesProvider"]["body"],
    )
    ctx.equal(
        "original native + none exception",
        old["legacy_rejection"],
        "LLMInvalidRequestError",
    )
    cases = []
    for num, (dialect, choice, native) in enumerate(
        [
            ("openai_responses", "auto", True),
            ("deepseek_responses_legacy", "get", False),
            ("deepseek_responses", "get", False),
            ("deepseek_responses_legacy", "none", True),
        ]
    ):
        service = Service(ctx, ctx.working / f"data-{num}")
        service.command("init")

        def op(*args, ok=True):
            return service.command("operator", *map(str, args), accepted=ok)

        def rows(sql, *args):
            with closing(
                sqlite3.connect(
                    (service.data / "core.sqlite").as_uri() + "?mode=ro", uri=True
                )
            ) as db:
                return db.execute(sql, args).fetchall()

        def wait(sql, *args):
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                if r := rows(sql, *args):
                    return r
                time.sleep(0.03)
            raise AssertionError("durable state timed out: " + sql)

        actor = "00000000-0000-4000-8000-000000000019"
        with service.running():
            info = op("database-info")
            if num == 0:
                ctx.write("sqlite-build.json", info)
            op("person", "create", actor)
            a = op(
                "account",
                "open",
                "qq",
                "fixture",
                "200",
                1,
                "--person",
                actor,
                "--start",
                1,
            )["assignment"]
            b = op("account", "open", "qq", "fixture", "100", 1, "--start", 1)[
                "assignment"
            ]
            entry = op("entry", "private", a, b)["entry"]
        with Recorder(path="/v1/responses") as peer:
            with (service.data / "yuki.toml").open("a") as f:
                f.write(
                    f'\n[main]\nendpoint="{peer.url}"\nprotocol="{dialect}"\nmodel="fixture-responses"\nreasoning_effort="low"\ntool_choice="{choice}"\nnative_web_search={str(native).lower()}\nname="Yuki"\nmerge_ms=0\nroot_slots=6\nmax_input_units=250000\nmax_output=128\n'
                )
            with service.running():
                op("main", "register", "plugin", "source", entry)

                def trigger(key, payload=None):
                    return op(
                        "main",
                        "trigger",
                        "source",
                        key,
                        json.dumps(payload or {"text": key}),
                    )["execution"]

                run = trigger("original request")
                if dialect == "deepseek_responses_legacy" and native:
                    wait("SELECT id FROM executions WHERE id=? AND state='held'", run)
                    ctx.rejects(
                        "unrepresentable none + native is pre-dispatch",
                        queue.Empty,
                        lambda: peer.wires.get(timeout=0.4),
                    )
                    ctx.equal(
                        "zero physical attempts or logical reserve",
                        rows("SELECT count(*) FROM model_requests"),
                        [(0,)],
                    )
                    cases.append(
                        {"dialect": dialect, "choice": choice, "physical_attempts": 0}
                    )
                    continue
                w1, q = peer.receive()
                request = json.loads(w1)
                baseline = old[
                    "OpenAIResponsesProvider"
                    if dialect == "openai_responses"
                    else "DeepSeekResponsesProvider"
                ]["body"]
                ctx.equal(
                    "configured reasoning effort reaches physical request",
                    request["reasoning"],
                    baseline["reasoning"],
                )
                if dialect == "openai_responses":
                    ctx.equal(
                        "baseline standard include list",
                        request["include"],
                        baseline["include"],
                    )
                if dialect.endswith("legacy"):
                    ctx.equal(
                        "legacy preserves baseline storage and include omissions",
                        ("include" in request, "store" in request),
                        ("include" in baseline, "store" in baseline),
                    )
                ctx.check(
                    "explicit stateless local history",
                    "previous_response_id" not in request,
                )
                ctx.check(
                    "message body is Responses input",
                    "input" in request and "messages" not in request,
                )
                if dialect == "openai_responses":
                    ctx.check(
                        "standard disables store with encrypted evidence",
                        request["store"] is False
                        and "reasoning.encrypted_content" in request["include"],
                    )
                elif dialect.endswith("legacy"):
                    ctx.check(
                        "baseline DeepSeek omits tool_choice",
                        "tool_choice" not in request,
                    )
                else:
                    ctx.equal(
                        "documented named selection",
                        request["tool_choice"],
                        {"type": "function", "name": "get"},
                    )
                reason = {
                    "id": "rs-item",
                    "type": "reasoning",
                    "summary": [],
                    "encrypted_content": "fixture-encrypted",
                    "future_opaque": {"keep": True},
                }
                msg = {
                    "id": "m-item",
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "internal text"}],
                }
                tool = {
                    "type": "function_call",
                    "id": "item-not-call",
                    "call_id": "link-original",
                    "name": "get",
                    "arguments": '{"target":"key"}',
                    "status": "completed",
                }
                items = [reason, msg]
                if native:
                    items.append(
                        {
                            "type": "web_search_call",
                            "id": "native-event",
                            "status": "completed",
                            "action": {
                                "type": "search",
                                "query": "topic",
                                "sources": [
                                    {
                                        "type": "url",
                                        "url": "https://example.test/result",
                                    }
                                ],
                            },
                        }
                    )
                items.append(tool)
                q.put(
                    (
                        {
                            "id": "resp-secret",
                            "status": "completed",
                            "output": items,
                            "usage": {
                                "input_tokens": 20,
                                "input_tokens_details": {"cached_tokens": 8},
                                "output_tokens": 5,
                                "output_tokens_details": {"reasoning_tokens": 2},
                                "total_tokens": 25,
                            },
                        },
                        0,
                    )
                )
                w2, q = peer.receive()
                follow = json.loads(w2)["input"]
                ctx.equal(
                    "ordered original output including unknown future fields",
                    follow[-len(items) - 1 : -1],
                    items,
                )
                ctx.check(
                    "function_call_output links call_id and a true receipt",
                    follow[-1]["type"] == "function_call_output"
                    and follow[-1]["call_id"] == "link-original"
                    and json.loads(follow[-1]["output"])["state"] == "confirmed",
                )
                q.put(({"id": "fin", "status": "completed", "output": [msg]}, 0))
                terminal = "completed" if choice == "auto" else "held"
                wait("SELECT id FROM executions WHERE id=? AND state=?", run, terminal)
                saved = op("main", "inspect", run)
                ctx.equal("two real physical requests", len(saved["attempts"]), 2)
                usage = json.loads(
                    rows("SELECT usage_json FROM model_attempts ORDER BY rowid")[0][0]
                )
                ctx.equal(
                    "cache and reasoning are original known counts",
                    (usage["cache"], usage["reasoning"]),
                    (8, 2),
                )
                cases.append(
                    {
                        "dialect": dialect,
                        "wires": [w1, w2],
                        "inspection": saved,
                        "usage": usage,
                        "live_verified": False,
                    }
                )
                for key, body in (
                    (
                        "duplicate",
                        {
                            "status": "completed",
                            "output": [tool, {**tool, "id": "another-item"}],
                        },
                    ),
                    (
                        "incomplete",
                        {
                            "status": "incomplete",
                            "incomplete_details": {"reason": "max_output_tokens"},
                            "output": [tool],
                        },
                    ),
                    (
                        "sse",
                        b'event: response.output_item.added\ndata: {"type":"response.output_item.added","item":{"type":"function_call","arguments":"{}"}}\n\n',
                    ),
                ):
                    bad = trigger(key)
                    raw, q = peer.receive()
                    q.put((body, 0))
                    wait("SELECT id FROM executions WHERE id=? AND state='held'", bad)
                    ctx.equal(
                        "invalid or incomplete items produce no operation " + key,
                        rows("SELECT count(*) FROM operations WHERE namespace=?", bad),
                        [(0,)],
                    )
                    if key == "sse":
                        partial = rows(
                            "SELECT t.partial_bytes,t.diagnostic FROM model_attempts t JOIN model_requests r ON r.id=t.logical JOIN activations a ON a.id=r.activation WHERE a.execution=?",
                            bad,
                        )[0]
                        ctx.equal(
                            "partial SSE kept as bytes without tool dispatch",
                            partial,
                            (body, "incomplete_responses_stream"),
                        )
                if native:
                    failed = trigger("upstream native failure")
                    _, q = peer.receive()
                    event = {
                        "type": "web_search_call",
                        "id": "original-native",
                        "status": "failed",
                        "action": {"type": "search", "query": "missing source"},
                    }
                    q.put(({"status": "completed", "output": [event, msg]}, 0))
                    wait(
                        "SELECT id FROM executions WHERE id=? AND state='completed'",
                        failed,
                    )
                    native_result = op("main", "inspect", failed)["result"]
                    ctx.equal(
                        "HTTP 200 does not turn a failed native event into a success",
                        native_result["native_events"],
                        [event],
                    )
                    ctx.equal(
                        "native failure does not create a local effect",
                        rows(
                            "SELECT count(*) FROM operations WHERE namespace=?", failed
                        ),
                        [(0,)],
                    )
                ctx.check(
                    "physical attempt holds fixed native-tool boundary",
                    bool(
                        rows(
                            "SELECT native_tools FROM model_requests WHERE activation=? ORDER BY ordinal LIMIT 1",
                            "main:" + run,
                        )[0][0]
                    )
                    == native,
                )
                # Reuse the same structured contract with the provider's own function result representation.
                structured = trigger(
                    "extract",
                    {
                        "text": "extract",
                        "structured": {
                            "mode": "function_tool",
                            "schema": {
                                "type": "object",
                                "properties": {"value": {"type": "integer"}},
                                "required": ["value"],
                            },
                        },
                    },
                )
                raw, q = peer.receive()
                req = json.loads(raw)
                ctx.equal(
                    "single structured function in Responses",
                    req["tools"][0]["name"],
                    "emit_result",
                )
                q.put(
                    (
                        {
                            "status": "completed",
                            "output": [
                                {
                                    **tool,
                                    "name": "emit_result",
                                    "arguments": '{"value":4}',
                                }
                            ],
                        },
                        0,
                    )
                )
                wait(
                    "SELECT id FROM executions WHERE id=? AND state='completed'",
                    structured,
                )
                ctx.equal(
                    "validated native structured result",
                    op("main", "inspect", structured)["result"],
                    {"value": 4},
                )
                for mode in ("text_json", "json_schema"):
                    extract = trigger(
                        "mode-" + mode,
                        {
                            "text": "extract",
                            "structured": {
                                "mode": mode,
                                "schema": {
                                    "type": "object",
                                    "required": ["number"],
                                    "properties": {"number": {"type": "integer"}},
                                },
                            },
                        },
                    )
                    raw, q = peer.receive()
                    native_body = json.loads(raw)
                    ctx.equal(
                        "Responses native " + mode + " format",
                        native_body["text"]["format"]["type"],
                        "json_object" if mode == "text_json" else "json_schema",
                    )
                    q.put(
                        (
                            {
                                "status": "completed",
                                "output": [
                                    {
                                        "type": "message",
                                        "role": "assistant",
                                        "content": [
                                            {
                                                "type": "output_text",
                                                "text": '{"number":19}',
                                            }
                                        ],
                                    }
                                ],
                            },
                            0,
                        )
                    )
                    wait(
                        "SELECT id FROM executions WHERE id=? AND state='completed'",
                        extract,
                    )
                    ctx.equal(
                        "strict " + mode + " result",
                        op("main", "inspect", extract)["result"],
                        {"number": 19},
                    )
                    cases.append(
                        {
                            "dialect": dialect,
                            "mode": mode,
                            "wire": raw,
                            "live_verified": False,
                        }
                    )
    ctx.write("responses-dialect.json", {"cases": {"H17": cases}, "sources": corpus})
    return {
        "original_responses": cases,
        "declared_reference": corpus,
        "external_provider_requests": 0,
    }
