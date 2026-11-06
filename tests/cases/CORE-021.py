"""GenerateContent runner with real POST bodies, original function IDs, native events and transport faults."""

import json
import os
import sqlite3
import time
from contextlib import closing
from tests.harness.service import Service
from tests.peers.chat.recorder import Recorder


def run(ctx):
    fixture = json.loads(
        (ctx.root / "fixtures/providers/generate-content/v4-original.json").read_text()
    )
    ctx.equal(
        "fixed adapter SHA",
        fixture["source_commit"],
        "329ca7c487414d2f54ddd98ff4727446ed2446cd",
    )
    ref = fixture["wires"][0]
    paired = fixture["wires"][1]["body"]["contents"][-1]["parts"]
    ctx.equal(
        "v4 records exactly one upstream ID",
        [p["functionResponse"].get("id") for p in paired],
        ["native-1", None],
    )
    rows_seen = []
    for profile in ("mix", "no-mix", "local"):
        svc = Service(ctx, ctx.working / profile)
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
            deadline = time.monotonic() + 10
            while rows("SELECT state FROM executions WHERE id=?", run) != [(name,)]:
                if time.monotonic() > deadline:
                    raise AssertionError("execution did not reach " + name)
                time.sleep(0.03)

        actor = "00000000-0000-4000-8000-000000000021"
        with svc.running():
            if profile == "mix":
                ctx.write("sqlite-build.json", op("database-info"))
            op("person", "create", actor)
            a = op(
                "account",
                "open",
                "qq",
                "google",
                "200",
                1,
                "--person",
                actor,
                "--start",
                1,
            )["assignment"]
            b = op("account", "open", "qq", "google", "100", 1, "--start", 1)[
                "assignment"
            ]
            entry = op("entry", "private", a, b)["entry"]
        with Recorder(ref["path"], binding_status=503) as peer:
            cfg = f'\n[main]\nprotocol="gemini"\nendpoint="{peer.url}"\nbinding_endpoint="http://127.0.0.1:{peer.server.server_port}/binding"\nmodel="thinking-model"\nname="Yuki"\nkey_env="YUKI_GEMINI_FIXTURE_KEY"\nreasoning_effort="low"\nnative_web_search={str(profile != "local").lower()}\ngemini_mixed_tools={str(profile == "mix").lower()}\nmerge_ms=0\nroot_slots=4\nmax_input_units=200000\nmax_output=8192\n'
            with (svc.data / "yuki.toml").open("a") as f:
                f.write(cfg)
            with svc.running(
                environment={**os.environ, "YUKI_GEMINI_FIXTURE_KEY": "synthetic-only"}
            ):
                op("main", "register", "plugin", "source", entry)

                def trigger(key, task=None):
                    return op(
                        "main",
                        "trigger",
                        "source",
                        key,
                        json.dumps(
                            {"text": key, **({"structured": task} if task else {})}
                        ),
                    )["execution"]

                def receive():
                    raw, q = peer.receive()
                    return json.loads(raw), q

                def answer(parts, reason="STOP", extra=None):
                    content = {"role": "model", "parts": parts}
                    return {
                        "candidates": [
                            {
                                "finishReason": reason,
                                "content": content,
                                **(extra or {}),
                            }
                        ],
                        "usageMetadata": {
                            "promptTokenCount": 12,
                            "candidatesTokenCount": 5,
                            "thoughtsTokenCount": 2,
                            "totalTokenCount": 19,
                            "cachedContentTokenCount": 1,
                        },
                    }

                run = trigger("pair")
                if profile == "no-mix":
                    state(run, "held")
                    ctx.equal(
                        "profile rejects unrepresentable mix before payment",
                        len(op("main", "inspect", run)["attempts"]),
                        0,
                    )
                    ctx.check(
                        "unsupported mix made zero HTTP requests", peer.wires.empty()
                    )
                    continue
                wire, q = receive()
                ctx.equal(
                    "physical endpoint uses Google API-key shape and no bearer",
                    peer.header_shapes[-1],
                    {
                        "version": None,
                        "api_key": False,
                        "bearer": False,
                        "google_key": True,
                    },
                )
                ctx.equal(
                    "generationConfig reproduces original",
                    wire["generationConfig"],
                    ref["body"]["generationConfig"],
                )
                if profile == "mix":
                    ctx.equal(
                        "explicitly enabled mix gets validated mode",
                        wire["toolConfig"],
                        ref["body"]["toolConfig"],
                    )
                parts = [
                    {"thought": True, "text": "private thought"},
                    {
                        "functionCall": {
                            "name": "get",
                            "id": "native-1",
                            "args": {"target": "key"},
                        },
                        "thoughtSignature": "sig-one",
                    },
                    {
                        "functionCall": {
                            "name": "complete",
                            "args": {"summary": "failure"},
                        },
                        "thoughtSignature": "sig-two",
                    },
                ]
                q.put((answer(parts), 0))
                follow, q = receive()
                ctx.equal(
                    "original model parts and signatures retained exactly",
                    follow["contents"][-2],
                    {"role": "model", "parts": parts},
                )
                got = [x["functionResponse"] for x in follow["contents"][-1]["parts"]]
                ctx.equal(
                    "responses retain names and omit absent native ID",
                    [(p["name"], p.get("id")) for p in got],
                    [("get", "native-1"), ("complete", None)],
                )
                ctx.check(
                    "typed tool errors and outputs remain distinct",
                    "output" in got[0]["response"]
                    and "error" in got[1]["response"]
                    and "id" not in got[1],
                )
                local = rows(
                    "SELECT call_id FROM model_tool_intents i JOIN model_requests r ON r.id=i.logical JOIN activations a ON a.id=r.activation WHERE a.execution=? ORDER BY i.ordinal",
                    run,
                )
                ctx.check(
                    "generated key lives only in local pair rows",
                    local[1][0].startswith("gemini_")
                    and local[1][0] not in json.dumps(follow),
                )
                media = {"inlineData": {"mimeType": "image/png", "data": "iVBORw0K"}}
                final = [
                    {"thought": True, "text": "private final"},
                    {"text": "internal"},
                    media,
                ]
                grounding = {
                    "groundingMetadata": {
                        "webSearchQueries": ["weather"],
                        "groundingChunks": [
                            {
                                "web": {
                                    "uri": "https://example.test/e",
                                    "title": "fixture",
                                }
                            }
                        ],
                    }
                }
                if profile == "mix":
                    final += [
                        {
                            "toolCall": {
                                "toolType": "GOOGLE_SEARCH",
                                "id": "s1",
                                "args": {"queries": ["weather"]},
                            }
                        },
                        {"toolResponse": {"id": "s1", "response": {"complete": True}}},
                    ]
                q.put((answer(final, extra=grounding if profile == "mix" else None), 0))
                state(run, "completed")
                ins = op("main", "inspect", run)
                ctx.equal(
                    "visible text excludes private thought",
                    ins["result"]["text"],
                    "internal",
                )
                ctx.equal(
                    "original generated media kept separate",
                    ins["result"]["media"][0]["media"],
                    media["inlineData"],
                )
                ctx.equal(
                    "only two local intent operations",
                    rows("SELECT count(*) FROM operations WHERE namespace=?", run),
                    [(2,)],
                )
                if profile == "mix":
                    ctx.equal(
                        "native search and grounding retained",
                        [
                            ins["result"]["native_events"][-1]["status"],
                            ins["result"]["citations"][0]["uri"],
                        ],
                        ["completed", "https://example.test/e"],
                    )
                result_usage = json.loads(
                    rows(
                        "SELECT usage_json FROM model_attempts ORDER BY rowid LIMIT 1"
                    )[0][0]
                )
                ctx.equal(
                    "known usage includes thoughts once",
                    [
                        result_usage[k]
                        for k in ("input", "output", "reasoning", "total", "cache")
                    ],
                    [12, 7, 2, 19, 1],
                )
                rows_seen.append(
                    {
                        "profile": profile,
                        "first": wire,
                        "follow": follow,
                        "paired_keys": local,
                        "final": ins,
                    }
                )
                if profile == "mix":
                    run = trigger("server-typed-failure")
                    _, q = receive()
                    q.put(
                        (
                            answer(
                                [
                                    {
                                        "toolCall": {
                                            "id": "fail",
                                            "toolType": "GOOGLE_SEARCH",
                                            "args": {},
                                        }
                                    },
                                    {
                                        "toolResponse": {
                                            "id": "fail",
                                            "response": {"error": "quota"},
                                        }
                                    },
                                ]
                            ),
                            0,
                        )
                    )
                    state(run, "completed")
                    ctx.equal(
                        "native error has no local function effect",
                        op("main", "inspect", run)["result"]["native_events"][-1][
                            "status"
                        ],
                        "failed",
                    )
                    run = trigger("native-denial")
                    _, q = receive()
                    upstream = {
                        "error": {
                            "code": 400,
                            "message": "fixture rejects native tool combination",
                            "status": "INVALID_ARGUMENT",
                        }
                    }
                    q.put((upstream, 0, 400))
                    state(run, "held")
                    saved = rows(
                        "SELECT t.status_code,t.response FROM model_attempts t JOIN model_requests r ON r.id=t.logical JOIN activations a ON a.id=r.activation WHERE a.execution=?",
                        run,
                    )
                    ctx.equal(
                        "real 400 retains original body and no paid fallback",
                        [(k, json.loads(raw)) for k, raw in saved],
                        [(400, upstream)],
                    )
                for key, reply, missing in [
                    ("duplicate", answer([parts[1], parts[1]]), 0),
                    ("partial", json.dumps(answer(parts)).encode()[:75], 200),
                ]:
                    run = trigger(key)
                    _, q = receive()
                    q.put((reply, missing))
                    state(run, "held")
                    ctx.equal(
                        "duplicate or partial call never dispatches",
                        rows("SELECT count(*) FROM operations WHERE namespace=?", run),
                        [(0,)],
                    )
                    attempts = rows(
                        "SELECT t.state,t.partial_bytes FROM model_attempts t JOIN model_requests r ON r.id=t.logical JOIN activations a ON a.id=r.activation WHERE a.execution=?",
                        run,
                    )
                    if missing:
                        ctx.equal(
                            "truncated transport retains exact partial bytes",
                            attempts,
                            [("unknown", reply)],
                        )
                schema = {
                    "type": "object",
                    "properties": {"ok": {"type": "boolean"}},
                    "required": ["ok"],
                }
                for mode in ("function_tool", "json_schema", "text_json"):
                    run = trigger("structure-" + mode, {"mode": mode, "schema": schema})
                    wire, q = receive()
                    if mode == "function_tool":
                        ctx.equal(
                            "structured function only declares emit_result",
                            wire["toolConfig"]["functionCallingConfig"],
                            {"mode": "ANY", "allowedFunctionNames": ["emit_result"]},
                        )
                        p = {
                            "functionCall": {
                                "name": "emit_result",
                                "args": {"ok": True},
                            }
                        }
                    else:
                        ctx.equal(
                            "selected native structure MIME",
                            wire["generationConfig"]["responseMimeType"],
                            "application/json",
                        )
                        ctx.equal(
                            "JSON schema mode sends actual schema",
                            wire["generationConfig"].get("responseJsonSchema"),
                            schema if mode == "json_schema" else None,
                        )
                        p = {"text": '{"ok":true}'}
                    q.put((answer([p]), 0))
                    state(run, "completed")
    ctx.write(
        "gemini-parts.json",
        {"cases": {"H19": rows_seen}, "v4": fixture["source_commit"]},
    )
    return {
        "physical_profiles": rows_seen,
        "original_capture": fixture["source_commit"],
        "endpoint": "offline GenerateContent HTTP recorders",
    }
