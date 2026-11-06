"""Compare original bridge semantics with physical provider wires and durable root charges."""

import json
import os
import sqlite3
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from tests.harness.service import Service
from tests.peers.chat.recorder import Recorder

ACTOR = "00000000-0000-4000-8000-000000000024"
SENTINEL = "PRIVATE main conversation do not disclose"
URL = "https://example.com/original"
PATHS = {
    "openai_responses": "/v1/responses",
    "anthropic_messages": "/v1/messages",
    "gemini": "/v1beta/models/fixture:generateContent",
}


def rows(svc, sql, *values):
    with closing(
        sqlite3.connect((svc.data / "core.sqlite").as_uri() + "?mode=ro", uri=True)
    ) as db:
        return db.execute(sql, values).fetchall()


def owner(ctx, key):
    svc = Service(ctx, ctx.working / key)
    svc.command("init", "--memory-mode", "lexical-only")

    def op(*args, ok=True):
        return svc.command("operator", *map(str, args), accepted=ok)

    with svc.running():
        ctx.write("sqlite-build.json", op("database-info"))
        op("person", "create", ACTOR)
        user = op(
            "account", "open", "qq", "native", "200", 1, "--person", ACTOR, "--start", 1
        )
        bot = op("account", "open", "qq", "native", "100", 1, "--start", 1)

        def human(key):
            frame = {
                "source": key,
                "account": user["account"],
                "bot_stage": bot["assignment"],
                "kind": "message",
                "occurred": 2,
                "observed": 2,
                "segments": [{"type": "text", "text": SENTINEL}],
            }
            return op("ingress", "record", json.dumps(frame), "--accept")["event"]

        ex = op("ingress", "claim", human("initial"))["execution"]
        op("model", "root", ex, 24, 1000000)
    return svc, op, ex, human


def cfg(peer, dialect, mode="native", strategy="bridge", namespace="search", ttl=600, max_results=5):
    cfg = f'\n[search]\nmode="{mode}"\nstrategy="{strategy}"\nconnection_id="separate-model"\ncredential_namespace="{namespace}"\ncache_seconds={ttl}\nmax_results={max_results}\n'
    if peer:
        cfg += f'[search.connection]\nprotocol="{dialect}"\nendpoint="{peer.url}"\nkey_env="YUKI_FIXTURE_SEARCH"\nmodel="fixture"\nname="Native"\nmerge_ms=0\nroot_slots=8\nmax_input_units=400000\nmax_output=1024\nnative_web_search=true\n'
    return cfg


def source(dialect):
    if dialect == "anthropic_messages":
        return {
            "type": "message",
            "role": "assistant",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 4,
                "output_tokens": 2,
                "server_tool_use": {"web_search_requests": 1},
            },
            "content": [
                {
                    "type": "server_tool_use",
                    "id": "s",
                    "name": "web_search",
                    "input": {"query": "茶具"},
                },
                {
                    "type": "web_search_tool_result",
                    "tool_use_id": "s",
                    "content": [
                        {"type": "web_search_result", "url": URL, "title": "original"}
                    ],
                },
            ],
        }
    if dialect == "gemini":
        return {
            "candidates": [
                {
                    "finishReason": "STOP",
                    "content": {"role": "model", "parts": [{"text": "answer"}]},
                    "groundingMetadata": {
                        "groundingChunks": [{"web": {"uri": URL, "title": "original"}}]
                    },
                }
            ],
            "usageMetadata": {"promptTokenCount": 4, "candidatesTokenCount": 2},
        }
    return {
        "status": "completed",
        "output": [
            {
                "id": "search",
                "type": "web_search_call",
                "status": "completed",
                "action": {
                    "type": "search",
                    "sources": [{"url": URL, "title": "original"}],
                },
            },
            {
                "id": "message",
                "type": "message",
                "status": "completed",
                "content": [
                    {
                        "type": "output_text",
                        "text": "body https://ignored.test/no-proof",
                        "annotations": [{"type": "url_citation", "url": URL}],
                    }
                ],
            },
        ],
        "usage": {"input_tokens": 4, "output_tokens": 2},
    }


def exercise(ctx, dialect, old):
    svc, op, ex, _ = owner(ctx, dialect)
    path = svc.data / "yuki.toml"
    original = path.read_text()
    environment = {**os.environ, "YUKI_FIXTURE_SEARCH": "synthetic-search-only"}
    with Recorder(PATHS[dialect]) as peer, ThreadPoolExecutor() as pool:
        path.write_text(original + cfg(peer, dialect))
        with svc.running(environment=environment):

            def search(key, request):
                return op(
                    "search", ex, ACTOR, key, json.dumps(request, ensure_ascii=False)
                )

            query = {"query": " 茶具  ", "max_results": 2}

            def physical(key, request, body, missing=0, status=200):
                task = pool.submit(search, key, request)
                raw, reply = peer.receive()
                wire = json.loads(raw)
                ctx.check(
                    "private Main data absent from actual HTTP", SENTINEL not in raw
                )
                reply.put((body, missing, status))
                return wire, task.result()

            wire, good = physical("first", query, source(dialect))
            key = (
                "input"
                if dialect == "openai_responses"
                else "contents" if dialect == "gemini" else "messages"
            )
            msg = wire[key]
            text = (
                msg[0]["parts"][0]["text"] if dialect == "gemini" else msg[0]["content"]
            )
            if isinstance(text, list):
                text = text[0]["text"]
            ctx.equal(
                "physically emitted independent original v4 user prompt",
                text,
                old["user"],
            )
            ctx.equal(
                "complete original source",
                ([s["url"] for s in good["sources"]], good["status"]),
                (old["original_urls"], "complete"),
            )
            ctx.check(
                "only native tools on independent wire",
                all(
                    t.get("type") != "function"
                    and "functionDeclarations" not in t
                    and "input_schema" not in t
                    for t in wire["tools"]
                ),
            )
            before = op("model", "usage", ex)
            cached = search("cache", {"query": "茶具", "max_results": 2})
            ctx.check(
                "cache before root reservation",
                cached["cached"]
                and peer.wires.empty()
                and before == op("model", "usage", ex),
            )
            dated = {
                "query": "茶具",
                "time_range": "week",
                "topic": "news",
                "max_results": 2,
            }
            for n in range(2):
                _, part = physical(f"date-{n}", dated, source(dialect))
                ctx.check(
                    "unknown dates remain partial and never populate success cache",
                    part["status"] == "partial" and not part["cached"],
                )
            bad = source(dialect)
            if dialect == "anthropic_messages":
                bad["content"][1]["tool_use_id"] = "different-attempt"
                bad["content"].append({"type": "text", "text": "https://prose.test/"})
            elif dialect == "gemini":
                bad["candidates"][0]["groundingMetadata"]["groundingChunks"] = [
                    {"web": {"uri": "http://127.0.0.1/secret"}}
                ]
            else:
                bad["output"] = bad["output"][1:]
                bad["output"][0]["content"][0]["annotations"] = []
            for n in range(2):
                _, lost = physical(f"no-source-{n}", {"query": "unproven URL"}, bad)
                ctx.equal(
                    "ordinary prose, unmatched ID or private URL cannot supply sources",
                    (lost["status"], lost["sources"]),
                    ("no_search_evidence", []),
                )
            _, unknown = physical(
                "lost", {"query": "drop wire"}, source(dialect), missing=20
            )
            ctx.check(
                "broken physical response yields unknown",
                unknown["status"] == "unknown",
            )
            ledger = rows(
                svc,
                "SELECT t.id,t.state,t.usage_json,t.ever_unknown FROM model_attempts t ORDER BY t.rowid",
            )
            ctx.equal(
                "one original reserve per physical call in controlled scenario",
                len(ledger),
                6,
            )
        with svc.running(environment=environment):
            repeated = op(
                "search", ex, ACTOR, "lost", json.dumps({"query": "drop wire"})
            )
            ctx.check(
                "cold repeat of uncertain request makes no purchase",
                repeated["status"] == "unknown"
                and peer.wires.empty()
                and len(rows(svc, "SELECT id FROM model_attempts")) == 6,
            )
            same = op(
                "search",
                ex,
                ACTOR,
                "same-after-restart",
                json.dumps(query, ensure_ascii=False),
            )
            ctx.check(
                "successful original proof survives owner restart before reserving a slot",
                same["cached"] and peer.wires.empty(),
            )
        if dialect == "openai_responses":
            previous = original + cfg(peer, dialect)
            for index, (mutated, token) in enumerate(
                [
                    (
                        previous.replace(
                            'connection_id="separate-model"', 'connection_id="new"'
                        ),
                        "synthetic-search-only",
                    ),
                    (
                        previous.replace(
                            'credential_namespace="search"',
                            'credential_namespace="other"',
                        ),
                        "synthetic-search-only",
                    ),
                    (
                        previous.replace('model="fixture"', 'model="other-profile"'),
                        "synthetic-search-only",
                    ),
                    (previous, "rotated-synthetic-key"),
                ]
            ):
                path.write_text(mutated)
                with svc.running(
                    environment={**environment, "YUKI_FIXTURE_SEARCH": token}
                ):
                    response = pool.submit(
                        op,
                        "search",
                        ex,
                        ACTOR,
                        "changed-" + str(index),
                        json.dumps(query, ensure_ascii=False),
                    )
                    _, q = peer.receive()
                    q.put((source(dialect), 0))
                    ctx.check(
                        "changed connection, namespace, profile or real credential causes a physical miss",
                        not response.result()["cached"],
                    )
        if dialect == "anthropic_messages":
            ctx.equal(
                "native server tool usage stays on the original attempt",
                json.loads(ledger[0][2])["server_web_search_requests"],
                1,
            )
        return {"wire": wire, "good": good, "cold_unknown": repeated, "ledger": ledger}


def normal_main(ctx):
    from bench.native_arrivals import pack, sha

    svc, op, _, human = owner(ctx, "main-bridge")
    env = {**os.environ, "YUKI_FIXTURE_SEARCH": "synthetic-search-only"}
    with Recorder() as main, Recorder(PATHS["openai_responses"]) as native:
        with (svc.data / "yuki.toml").open("a") as f:
            f.write(cfg(native, "openai_responses", mode="both", strategy="bridge", max_results=3))
            f.write(
                f'\n[main]\nendpoint="{main.url}"\nbinding_endpoint="http://127.0.0.1:{main.server.server_port}/binding"\nmodel="main-chat"\nname="Yuki"\nmerge_ms=2\nroot_slots=5\nmax_input_units=400000\nmax_output=1024\n'
            )
        with svc.running(environment=env):
            event = human("Main uses bridge")
            text, answer = main.receive()
            msg = json.loads(text)
            ctx.check(
                "normal Main has its own private history and exactly one local search declaration",
                SENTINEL in text
                and sum(t["function"]["name"] == "web_search" for t in msg["tools"])
                == 1,
            )
            declared = next(t for t in msg["tools"] if t["function"]["name"] == "web_search")
            oracle = json.loads((ctx.root / "fixtures/oracle/v4/native-group-contract.json").read_bytes())
            ctx.equal("configured bridge presents the original E13 search tool",
                      sha(pack(declared)), oracle["original"]["tools"]["web_search"])
            calls = [
                {"id": call_id, "type": "function", "function": {
                    "name": "web_search", "arguments": json.dumps(arguments),
                }}
                for call_id, arguments in [
                    ("override", {"query": "extra", "max_results": 20}),
                    ("lookup", {"query": "tea"}),
                ]
            ]
            answer.put(({"choices": [{"finish_reason": "tool_calls", "message": {
                "role": "assistant", "content": None, "tool_calls": calls,
            }}]}, 0))
            isolated, answer = native.receive()
            ctx.check(
                "normal search goes through independent model with no Main prompt",
                SENTINEL not in isolated and json.loads(isolated)["model"] == "fixture",
            )
            body = json.loads(isolated)
            prompt = body["input"][0]["content"]
            if isinstance(prompt, list):
                prompt = prompt[0]["text"]
            provider_query = json.loads(prompt.splitlines()[-1])
            ctx.equal("only the allowed query reaches native search at the deployment limit",
                      [provider_query["query"], provider_query["max_results"]], ["tea", 3])
            answer.put((source("openai_responses"), 0))
            raw, answer = main.receive()
            receipt = json.loads(
                next(
                    m["content"]
                    for m in json.loads(raw)["messages"]
                    if m.get("tool_call_id") == "lookup"
                )
            )
            refused = json.loads(next(
                m["content"] for m in json.loads(raw)["messages"]
                if m.get("tool_call_id") == "override"
            ))
            ctx.check("model override refused without a native search attempt",
                      refused["state"] == "failed" and native.wires.empty())
            ctx.write("search-tool-policy.json", {
                "original_tool_sha256": sha(pack(declared)), "declared": declared,
                "provider_request": body, "max_results": 3, "refused_override": refused,
            })
            ctx.equal(
                "paid Main continues on the native search receipt",
                (receipt["state"], receipt["result"]["sources"][0]["url"]),
                ("confirmed", URL),
            )
            answer.put(
                (
                    {
                        "choices": [
                            {
                                "message": {"role": "assistant", "content": "done"},
                                "finish_reason": "stop",
                            }
                        ]
                    },
                    0,
                )
            )
            import time

            end = time.monotonic() + 10
            while time.monotonic() < end:
                if rows(
                    svc,
                    "SELECT id FROM executions WHERE source=? AND state='completed'",
                    event,
                ):
                    break
                time.sleep(0.02)
            root = rows(svc, "SELECT id FROM executions WHERE source=?", event)[0][0]
            charges = op("model", "usage", root)
            ctx.equal(
                "two main wires and one search wire spend the same original root",
                charges["reserved_requests"],
                3,
            )
            return {"sources": receipt["result"]["sources"], "root_usage": charges}


def budget_and_erasure(ctx):
    from time import sleep

    svc, op, first, human = owner(ctx, "search-budget")
    with Recorder(PATHS["openai_responses"]) as peer, ThreadPoolExecutor() as pool:
        with (svc.data / "yuki.toml").open("a") as f:
            f.write(cfg(peer, "openai_responses", ttl=2))
        with svc.running(
            environment={**os.environ, "YUKI_FIXTURE_SEARCH": "synthetic-only"}
        ):

            def root(key):
                event = human(key)
                ex = op("ingress", "claim", event)["execution"]
                op("model", "root", ex, 1, 20000)
                return ex, event

            second, _ = root("one request budget")
            third, _ = root("fresh after erasure")
            first_source = rows(svc, "SELECT source FROM executions WHERE id=?", first)[
                0
            ][0]
            q = json.dumps({"query": "same stable query"})

            def physical(ex, key):
                task = pool.submit(op, "search", ex, ACTOR, key, q)
                _, a = peer.receive()
                a.put((source("openai_responses"), 0))
                return task.result()

            physical(first, "original")
            hit = op("search", second, ACTOR, "consume-cached", q)
            ctx.check(
                "distinct live execution can read only without fresh reserve",
                hit["cached"]
                and op("model", "usage", second)["reserved_requests"] == 0,
            )
            sleep(2.1)
            refreshed = physical(second, "expired")
            ctx.check(
                "expired proof buys once",
                not refreshed["cached"]
                and op("model", "usage", second)["reserved_requests"] == 1,
            )
            op(
                "search",
                second,
                ACTOR,
                "over-budget",
                json.dumps({"query": "different"}),
                ok=False,
            )
            ctx.check(
                "budget exhaustion occurs before physical HTTP", peer.wires.empty()
            )
            op(
                "privacy",
                "erase",
                "forget-search-origin",
                ACTOR,
                json.dumps([first_source]),
            )
            proved = physical(third, "clean-after-erase")
            ctx.check(
                "erase invalidates original and dependent cached search receipts",
                not proved["cached"],
            )
            return {
                "original": first_source,
                "budget_refused": True,
                "fresh_after_erasure": proved["attempt"],
            }


def inline_modes(ctx):
    out = []
    modes = [
        ("disabled", "both", False, False),
        ("tavily", "both", False, False),
        ("native", "external", False, False),
        ("native", "bridge", False, True),
        ("native", "native", True, False),
        ("both", "both", True, False),
    ]
    for n, (mode, strategy, inline, local) in enumerate(modes):
        svc, op, _, human = owner(ctx, "policy-" + str(n))
        env = {**os.environ, "YUKI_FIXTURE_SEARCH": "synthetic-only"}
        with Recorder(PATHS["anthropic_messages"]) as main, Recorder(
            PATHS["openai_responses"]
        ) as search:
            with (svc.data / "yuki.toml").open("a") as f:
                f.write(cfg(search, "openai_responses", mode, strategy))
                f.write(
                    f'\n[main]\nprotocol="anthropic_messages"\nnative_web_search=true\nendpoint="{main.url}"\nkey_env="YUKI_FIXTURE_SEARCH"\nmodel="fixture"\nname="Yuki"\nmerge_ms=0\nroot_slots=5\nmax_input_units=400000\nmax_output=256\n'
                )
            with svc.running(environment=env):
                human("policy")
                raw, reply = main.receive()
                visible = [
                    t for t in json.loads(raw)["tools"] if t["name"] == "web_search"
                ]
                ctx.equal(
                    "global mode and profile strategy gate actual Claude declarations without collisions",
                    ([t.get("type") == "web_search_20250305" for t in visible]),
                    [True] if inline else [False] if local else [],
                )
                reply.put(
                    (
                        {
                            "type": "message",
                            "role": "assistant",
                            "stop_reason": "end_turn",
                            "content": [{"type": "text", "text": "done"}],
                            "usage": {"input_tokens": 3, "output_tokens": 1},
                        },
                        0,
                    )
                )
                ctx.check(
                    "inspection of Main declarations consumed no auxiliary search",
                    search.wires.empty(),
                )
                out.append(
                    {"mode": mode, "strategy": strategy, "declarations": visible}
                )
    return out


def run(ctx):
    old = json.loads((ctx.root / "fixtures/search/v4-bridge.json").read_bytes())
    cases = {d: exercise(ctx, d, old) for d in PATHS}
    for mode, strategy in [
        ("disabled", "both"),
        ("tavily", "bridge"),
        ("both", "external"),
        ("native", "native"),
    ]:
        svc, op, ex, _ = owner(ctx, mode + "-" + strategy)
        with (svc.data / "yuki.toml").open("a") as f:
            f.write(cfg(None, "openai_responses", mode, strategy))
        with svc.running():
            op("search", ex, ACTOR, "denied", '{"query":"tea"}', ok=False)
            ctx.equal(
                "inactive bridge cannot spend or route to Tavily",
                rows(svc, "SELECT count(*) FROM model_requests"),
                [(0,)],
            )
    report = {
        "old_commit": old["source_commit"],
        "cases": {"H21": cases},
        "main": normal_main(ctx),
        "ttl_budget_erasure": budget_and_erasure(ctx),
        "claude_physical_declarations": inline_modes(ctx),
    }
    ctx.write("search-proof-cache.json", report)
    return report
