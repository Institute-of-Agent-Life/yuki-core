"""Use the live Rust Main runner. All physical requests terminate at a local HTTP recorder."""

import json
import queue
import sqlite3
import subprocess
import time
from contextlib import closing
from tests.harness.service import Service, previous_schema
from tests.peers.chat.recorder import Recorder


def run(ctx):
    def response(reason="stop", content="", calls=None):
        message = {
            "role": "assistant",
            "content": content,
            "reasoning_content": "private reasoning",
        }
        if calls is not None:
            message["tool_calls"] = calls
        return {
            "choices": [{"index": 0, "finish_reason": reason, "message": message}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 4, "total_tokens": 7},
        }

    def call(id, name, args):
        return {
            "type": "function",
            "id": id,
            "function": {"name": name, "arguments": json.dumps(args)},
        }

    # Exercise schema 8 with both inbound and outbound FKs to an existing execution.
    upgrade = Service(ctx, ctx.working / "legacy")
    previous_schema(ctx, upgrade.data, 8)
    with closing(sqlite3.connect(upgrade.data / "core.sqlite")) as db:
        db.executescript("""
            PRAGMA foreign_keys=ON;
            INSERT INTO persons(id) VALUES('00000000-0000-4000-8000-000000000088');
            INSERT INTO accounts VALUES('a','qq','legacy','1'),('b','qq','legacy','2');
            INSERT INTO assignments(id,account_id,original_person,kind,status,first_observed) VALUES('actor','a','00000000-0000-4000-8000-000000000088','person','active',1),('bot','b',NULL,'self','active',1);
            INSERT INTO entries(id,scope,actor_stage,bot_stage) VALUES('entry','private','actor','bot');
            INSERT INTO events(id,source_scope,source_key,entry_id,account_id,raw_actor,actor_stage,bot_stage,observed_at,envelope,mentions) VALUES(1,'scope','key','entry','a','00000000-0000-4000-8000-000000000088','actor','bot',1,'{}','[]');
            INSERT INTO accepted_sources VALUES(1);
            INSERT INTO executions(id,source,original_actor,state) VALUES('old-run',1,'00000000-0000-4000-8000-000000000088','completed');
            INSERT INTO operations(namespace,operation_id,action,target,payload,state,result) VALUES('old-run','op','external','send','{}','confirmed','{"verified":true}');
            INSERT INTO root_budgets(root,max_requests,max_units) VALUES('old-run',2,400);
            INSERT INTO budget_members VALUES('old-run','old-run');
        """)
    ctx.check(
        "populated v8 migrates to the Main schema",
        upgrade.command("migrate")["schema_version"] >= 9,
    )
    with closing(sqlite3.connect(upgrade.data / "core.sqlite")) as db:
        ctx.equal(
            "all legacy references remain intact",
            db.execute("pragma foreign_key_check").fetchall(),
            [],
        )
        ctx.equal(
            "original execution, human source and receipt preserved",
            db.execute(
                "SELECT x.source,x.origin,o.result FROM executions x JOIN operations o ON x.id=o.namespace"
            ).fetchall(),
            [(1, None, '{"verified":true}')],
        )
    service = Service(ctx, ctx.working / "data")
    service.command("init", "--memory-mode", "lexical-only")
    actor = "00000000-0000-4000-8000-000000000012"

    def op(*args, ok=True):
        return service.command("operator", *map(str, args), accepted=ok)

    def rows(sql, *args):
        with closing(
            sqlite3.connect(
                (service.data / "core.sqlite").as_uri() + "?mode=ro", uri=True
            )
        ) as db:
            return db.execute(sql, args).fetchall()

    def until(fn):
        stop = time.monotonic() + 12
        while time.monotonic() < stop:
            value = fn()
            if value:
                return value
            time.sleep(0.03)
        raise AssertionError("timed out waiting for durable Main transition")

    with service.running():
        ctx.write("sqlite-build.json", op("database-info"))
        op("person", "create", actor)
        user = op(
            "account", "open", "qq", "main", "20000", 1, "--person", actor, "--start", 1
        )
        bot = op("account", "open", "qq", "main", "10001", 1, "--start", 1)
        conversation = op("entry", "private", user["assignment"], bot["assignment"])[
            "entry"
        ]
    seq = 0

    def human(text):
        nonlocal seq
        seq += 1
        frame = {
            "source": "chat-" + str(seq),
            "account": user["account"],
            "bot_stage": bot["assignment"],
            "occurred": 100 + seq,
            "observed": 100 + seq,
            "kind": "message",
            "segments": [{"type": "text", "text": text}],
        }
        return op("ingress", "record", json.dumps(frame), "--accept")["event"]

    origin = []
    layout = []
    structured = []
    boundary = []
    interrupt = []
    with Recorder() as server:
        with (service.data / "yuki.toml").open("a") as cfg:
            cfg.write(
                f'\n[main]\nendpoint="{server.url}"\nbinding_endpoint="{server.url.removesuffix("/v1/chat/completions")}/binding"\nmodel="fixture-chat"\nname="Yuki"\nmerge_ms=450\nroot_slots=6\nmax_input_units=1000000\nmax_output=128\n'
            )
        soul = service.data / "SOUL.md"
        soul.write_text("hello {{name}}; literal {{other}}", encoding="utf-8")

        def wire():
            try:
                raw, reply = server.receive()
            except queue.Empty:
                ctx.write("main-timeout.json", {
                    table: rows("SELECT * FROM " + table)
                    for table in ("executions", "main_turns", "main_segments",
                                  "root_budgets", "model_requests", "budget_members")
                })
                raise
            return raw, json.loads(raw), reply

        with service.running():
            first = human("first human line")
            second = human("second human line")
            raw, body, q = wire()
            ctx.check(
                "two nearby human inputs batched",
                "first human line" in raw and "second human line" in raw,
            )
            ctx.check(
                "only defined name interpolation",
                "hello Yuki; literal {{other}}" in raw
                and sum(
                    m["content"].startswith("\n\n<runtime_context>")
                    for m in body["messages"]
                )
                == 1
                and body["messages"][-2]["content"]
                == '\n\n<runtime_context>\n{"state":"no_active_work","state_scope":"current_activation"}\n</runtime_context>\n\n',
            )
            program = (ctx.root / "src/context/main-program.txt").read_text(
                encoding="utf-8"
            )
            grounding = json.loads((ctx.root / "fixtures/memory/v4-grounding.json").read_text(encoding="utf-8"))
            ctx.equal("Memory guidance comes from the fixed behavior baseline",grounding["source_commit"],"329ca7c487414d2f54ddd98ff4727446ed2446cd")
            ctx.equal("Memory guidance preserves the exact original rule",(ctx.root / "src/context/memory-program.txt").read_text(encoding="utf-8").rstrip(),grounding["text"])
            ctx.equal(
                "physical Chat request preserves original identity, SOUL and program order",
                body["messages"][0],
                {
                    "role": "system",
                    "content": "【角色身份】\n当前角色的名称是 Yuki。\n\n【SOUL】\nhello Yuki; literal {{other}}\n\n"
                    + grounding["text"] + "\n\n" + program,
                },
            )
            layout.append(raw)
            q.put((response(content='{"tool":"send","message":"do not send"}'), 0))
            until(lambda: rows("SELECT id FROM executions WHERE state='completed'"))
            ctx.equal(
                "plain text creates no public effect",
                rows("SELECT count(*) FROM operations"),
                [(0,)],
            )
            first_exec = rows("SELECT id FROM executions WHERE source=?", first)[0][0]
            human_origin = op("main", "inspect", first_exec)
            ctx.check(
                "ordinary human runner requires no Work",
                human_origin["kind"] == "human"
                and human_origin["source"] == first
                and rows("SELECT work_id FROM executions WHERE id=?", first_exec)
                == [(None,)],
            )
            origin.append(human_origin)
            original_paid = rows(
                "SELECT request_json FROM model_requests ORDER BY rowid"
            )[0][0]
            soul.write_text("", encoding="utf-8")
            # External file edits activate through the shared management reload;
            # already purchased executions retain their original SOUL snapshot.
            op("control", "write", "character.soul.reload", json.dumps({
                "request_id": "00000000-0000-4000-8000-000000000012"
            }))
            count = rows("SELECT count(*) FROM events")[0][0]
            for kind in ("plugin", "self", "schedule", "work", "child"):
                ref = "fixture-" + kind
                parent = rows("SELECT id FROM executions WHERE source=?", (first))[0][0]
                args = ["--parent", parent] if kind in ("work", "child") else []
                op("main", "register", kind, ref, conversation, *args)
                run = op(
                    "main",
                    "trigger",
                    ref,
                    "occurrence",
                    json.dumps(
                        {
                            "text": "source " + kind,
                            "context": ["private material one", "private material two"],
                        }
                    ),
                )
                raw, body, q = wire()
                layout.append(raw)
                ctx.check(
                    "real host boundary before trigger " + kind,
                    "<runtime_context>" in raw
                    and body["messages"][-1]["content"] == "source " + kind,
                )
                ctx.check("empty SOUL is respected", "【SOUL】" not in raw)
                q.put((response(), 0))
                until(
                    lambda: rows(
                        "SELECT id FROM executions WHERE id=? AND state='completed'",
                        run["execution"],
                    )
                )
                receipt = op("main", "inspect", run["execution"])
                ctx.check(
                    "registered source shares original conversation " + kind,
                    receipt["source"] is None
                    and receipt["kind"] == kind
                    and receipt["conversation"] == conversation
                    and receipt["tool_contract"] == "main-chat-v1",
                )
                ctx.equal(
                    "original or delegated budget root " + kind,
                    receipt["root"],
                    first_exec if kind in ("work", "child") else run["execution"],
                )
                origin.append(receipt)
            ctx.equal(
                "internal sources created zero human events",
                rows("SELECT count(*) FROM events")[0][0],
                count,
            )
            ctx.equal(
                "paid request stayed immutable across hot SOUL change",
                rows("SELECT request_json FROM model_requests ORDER BY rowid")[0][0],
                original_paid,
            )
            schema = {
                "type": "object",
                "properties": {"value": {"type": "integer"}},
                "required": ["value"],
                "additionalProperties": False,
            }
            for mode, valid in (
                ("function_tool", True),
                ("text_json", True),
                ("json_schema", True),
                ("text_json", False),
                ("function_tool", False),
            ):
                payload = {
                    "text": "extract",
                    "structured": {"mode": mode, "schema": schema},
                }
                run = op(
                    "main",
                    "trigger",
                    "fixture-plugin",
                    "structured-" + str(len(structured)),
                    json.dumps(payload),
                )
                raw, body, q = wire()
                value = {"value": 4} if valid else {"value": "invalid"}
                q.put(
                    (
                        response("tool_calls", None, [call("F", "emit_result", value)])
                        if mode == "function_tool"
                        else response(content=json.dumps(value)),
                        0,
                    )
                )
                result = until(
                    lambda: rows(
                        "SELECT state FROM executions WHERE id=? AND state IN ('completed','held')",
                        run["execution"],
                    )
                )
                ctx.equal(
                    "strict " + mode + " validation " + str(valid),
                    result,
                    [("completed" if valid else "held",)],
                )
                structured.append(
                    {
                        "mode": mode,
                        "valid": valid,
                        "wire": raw,
                        "result": op("main", "inspect", run["execution"]),
                    }
                )
            for number, invalid in enumerate(
                (
                    response(
                        "tool_calls", None, [call("X", "emit_result", {"value": 1})]
                    ),
                    response(content='{"value":1} plus text'),
                )
            ):
                invalid_run = op(
                    "main",
                    "trigger",
                    "fixture-plugin",
                    "bad-json-" + str(number),
                    json.dumps(
                        {
                            "text": "extract JSON",
                            "structured": {"mode": "text_json", "schema": schema},
                        }
                    ),
                )["execution"]
                raw, _, q = wire()
                q.put((invalid, 0))
                until(
                    lambda: rows(
                        "SELECT id FROM executions WHERE id=? AND state='held'",
                        invalid_run,
                    )
                )
                ctx.equal(
                    "unexpected tool or trailing text produced no operation",
                    rows(
                        "SELECT count(*) FROM operations WHERE namespace=?", invalid_run
                    ),
                    [(0,)],
                )
                structured.append({"id": invalid_run, "rejected_wire": raw})
            for finish, missing in (("length", 0), ("tool_calls", 9)):
                run = op(
                    "main",
                    "trigger",
                    "fixture-plugin",
                    "interrupt-" + finish,
                    json.dumps({"text": "interrupt fixture"}),
                )
                raw, body, q = wire()
                q.put(
                    (
                        response(finish, None, [call("I", "send", {"text": "canary"})]),
                        missing,
                    )
                )
                until(
                    lambda: rows(
                        "SELECT id FROM executions WHERE id=? AND state='held'",
                        run["execution"],
                    )
                )
                ctx.equal(
                    "truncated or short HTTP body grants no effects",
                    rows(
                        "SELECT count(*) FROM operations WHERE namespace=?",
                        run["execution"],
                    ),
                    [(0,)],
                )
                physical = rows(
                    "SELECT t.state,t.response,t.status_code,t.usage_json,t.partial_bytes FROM model_attempts t JOIN model_requests r ON r.id=t.logical JOIN activations a ON a.id=r.activation WHERE a.execution=?",
                    run["execution"],
                )
                ctx.equal("one original physical attempt retained", len(physical), 1)
                state, partial, status, usage, raw_bytes = physical[0]
                if missing:
                    ctx.equal(
                        "retained exact available raw bytes",
                        raw_bytes,
                        partial.encode(),
                    )
                ctx.equal(
                    "transport status is distinct from an explicit truncation",
                    state,
                    "unknown" if missing else "response_held",
                )
                ctx.check("actual status retained", status == 200)
                if partial and json.loads(partial).get("usage"):
                    ctx.equal(
                        "reported usage kept from received bytes",
                        json.loads(usage)["total"],
                        7,
                    )
                ctx.equal(
                    "no binding delivery from interrupted response",
                    len(server.deliveries),
                    0,
                )
                interrupt.append(
                    {
                        "receipt": op("main", "inspect", run["execution"]),
                        "received": partial,
                        "usage": usage,
                    }
                )
            # The HTTP fixture receives exactly the original ordered effects and returns real receipts.
            tool_run = op(
                "main",
                "trigger",
                "fixture-plugin",
                "real-tools",
                json.dumps({"text": "apply the sequence"}),
            )["execution"]
            _, _, q = wire()
            commands = [
                call("A", "get", {"target": "x"}),
                call("B", "put", {"target": "x", "value": 5}),
                call("C", "get", {"target": "x"}),
                call("D", "complete", {"summary": "done"}),
                call("E", "send", {"text": "result one"}),
                call("F", "send", {"text": "result two"}),
                call("G", "send", {"text": "result three"}),
            ]
            q.put((response("tool_calls", None, commands), 0))
            raw, body, q = wire()
            pairs = [m for m in body["messages"] if m["role"] == "tool"]
            ctx.equal(
                "paired tool results preserve physical call ids",
                [m["tool_call_id"] for m in pairs],
                list("ABCDEFG"),
            )
            ctx.check(
                "second read sees actual written value",
                json.loads(pairs[2]["content"])["result"]["value"]["value"] == 5,
            )
            ctx.equal(
                "synthetic peer received complete then three original sends",
                [x["tool"] for x in server.deliveries],
                ["complete", "send", "send", "send"],
            )
            ctx.check(
                "retained private provider reasoning in original assistant tool message",
                "private reasoning" in raw,
            )
            q.put((response(), 0))
            until(
                lambda: rows(
                    "SELECT id FROM executions WHERE id=? AND state='completed'",
                    tool_run,
                )
            )
            interrupt.append(
                {
                    "complete_tool_wire": raw,
                    "physical_deliveries": list(server.deliveries),
                }
            )
            repair = op(
                "main",
                "trigger",
                "fixture-plugin",
                "repair",
                json.dumps(
                    {
                        "text": "repair once",
                        "structured": {
                            "mode": "text_json",
                            "schema": schema,
                            "validation_retries": 1,
                        },
                    }
                ),
            )["execution"]
            _, _, q = wire()
            q.put((response(content="not JSON"), 0))
            repaired_wire, _, q = wire()
            q.put((response(content='{"value":9}'), 0))
            until(
                lambda: rows(
                    "SELECT id FROM executions WHERE id=? AND state='completed'", repair
                )
            )
            ctx.equal(
                "explicit repair pays one new logical request",
                len(
                    rows(
                        "SELECT r.id FROM model_requests r JOIN activations a ON a.id=r.activation WHERE a.execution=?",
                        repair,
                    )
                ),
                2,
            )
            structured.append(
                {"repair": repaired_wire, "result": op("main", "inspect", repair)}
            )
            # The captured foreground loses its own HTTP task. The successor stays live.
            old_event = human("interrupt this foreground")
            old_raw, _, old_http = wire()
            old = rows("SELECT id FROM executions WHERE source=?", old_event)[0][0]
            successor = human("new private instruction")
            next_raw, _, next_http = wire()
            new = rows("SELECT execution FROM main_inputs WHERE event=?", successor)[0][
                0
            ]
            old_http.put((response(content="old late text"), 0))
            next_http.put((response(), 0))
            until(
                lambda: rows(
                    "SELECT id FROM executions WHERE id=? AND state='completed'", new
                )
            )
            ctx.equal(
                "only old task was cancelled",
                rows(
                    "SELECT id,cancelled FROM executions WHERE id IN (?,?) ORDER BY cancelled",
                    old,
                    new,
                ),
                [(new, 0), (old, 1)],
            )
            ctx.check(
                "supplement is exclusive to next paid body",
                "new private instruction" not in old_raw
                and "new private instruction" in next_raw,
            )
            boundary.append(
                {
                    "cancelled_execution": old,
                    "successor": new,
                    "first_wire": old_raw,
                    "next_wire": next_raw,
                    "original": op("main", "inspect", old),
                }
            )
            for protection in ("work", "local", "native_dispatch"):
                event = human("protected " + protection)
                before, _, q = wire()
                protected = rows("SELECT id FROM executions WHERE source=?", event)[0][
                    0
                ]
                if protection == "work":
                    op("model", "attach-work", protected, "fixture-work")
                else:
                    action = "put" if protection == "local" else "external"
                    op(
                        "effect",
                        "intent",
                        protected,
                        "boundary",
                        actor,
                        action,
                        "protected-key",
                        "{}",
                    )
                    op(
                        "effect",
                        "local" if protection == "local" else "dispatch",
                        protected,
                        "boundary",
                        actor,
                    )
                late = human("steer " + protection)
                until(
                    lambda: rows(
                        "SELECT event FROM main_inputs WHERE execution=? AND event=?",
                        protected,
                        late,
                    )
                )
                q.put((response(), 0))
                after, _, q = wire()
                ctx.check(
                    "protected steer at next normal boundary",
                    "steer " + protection in after
                    and "steer " + protection not in before,
                )
                q.put((response(), 0))
                until(
                    lambda: rows(
                        "SELECT id FROM executions WHERE id=? AND state='completed'",
                        protected,
                    )
                )
                ctx.equal(
                    "protected original task remained active",
                    rows("SELECT cancelled FROM executions WHERE id=?", protected),
                    [(0,)],
                )
                boundary.append(
                    {
                        "protected": protection,
                        "execution": protected,
                        "events": [event, late],
                        "first_wire": before,
                        "next_wire": after,
                        "original": op("main", "inspect", protected),
                    }
                )
            other = "00000000-0000-4000-8000-000000123456"
            op("person", "create", other)
            other_account = op(
                "account",
                "open",
                "qq",
                "main",
                "20001",
                1,
                "--person",
                other,
                "--start",
                1,
            )["account"]

            def group(account, key, text):
                return op(
                    "ingress",
                    "record",
                    json.dumps(
                        {
                            "source": key,
                            "account": account,
                            "bot_stage": bot["assignment"],
                            "space": "fixture-group",
                            "occurred": 200,
                            "observed": 200,
                            "kind": "message",
                            "segments": [{"type": "text", "text": text}],
                        }
                    ),
                    "--accept",
                )["event"]

            group_first = group(
                user["account"], "group-direct", "addressed group request"
            )
            before, _, q = wire()
            foreground = rows("SELECT id FROM executions WHERE source=?", group_first)[
                0
            ][0]
            op("model", "attach-work", foreground, "protected-group-work")
            bystander = group(
                other_account, "group-bystander", "independent group observation"
            )
            independent = until(
                lambda: rows(
                    "SELECT execution FROM main_inputs WHERE event=?", bystander
                )
            )[0][0]
            ctx.check(
                "group bystander has its own execution", independent != foreground
            )
            q.put((response(content="internal group final"), 0))
            after, _, q = wire()
            ctx.check(
                "group bystander cannot assume original Work",
                rows("SELECT work_id FROM executions WHERE id=?", independent)
                == [(None,)]
                and "independent group observation" in after
                and "independent group observation" not in before,
            )
            q.put((response(), 0))
            until(
                lambda: rows(
                    "SELECT id FROM executions WHERE id=? AND state='completed'",
                    independent,
                )
            )
            boundary.append(
                {
                    "group_foreground": foreground,
                    "bystander": independent,
                    "origin_wire": before,
                    "independent_wire": after,
                }
            )
            human("/ai new")
            human("/ai help")
            until(lambda: rows("SELECT COUNT(*) FROM main_controls")[0][0] >= 2)
            ctx.check(
                "new window preserved original event",
                bool(rows("SELECT id FROM events WHERE id=?", first)),
            )
            ctx.rejects(
                "help and new consume zero requests",
                queue.Empty,
                lambda: server.wires.get(timeout=0.6),
            )
            event = human("fresh-window-only")
            fresh_wire, _, q = wire()
            ctx.check(
                "new window excludes previous private inputs",
                "fresh-window-only" in fresh_wire
                and "first human line" not in fresh_wire
                and "protected work" not in fresh_wire,
            )
            q.put((response(), 0))
            until(
                lambda: rows(
                    "SELECT id FROM executions WHERE source=? AND state='completed'",
                    event,
                )
            )
        # Kill the real owner after the HTTP server received a request, before it can persist the response.
        process = subprocess.Popen(
            [*service.argv, "run"], stdout=subprocess.PIPE, text=True
        )
        try:
            ctx.equal(
                "cold owner ready",
                json.loads(process.stdout.readline())["status"],
                "ready",
            )
            crash_run = op(
                "main",
                "trigger",
                "fixture-plugin",
                "lost-body",
                json.dumps({"text": "kill at physical dispatch"}),
            )["execution"]
            physical_raw, _, stalled = wire()
            process.kill()
            process.wait(timeout=10)
            stalled.put((response(), 0))
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)
            process.stdout.close()
        with service.running():
            until(
                lambda: rows(
                    "SELECT id FROM executions WHERE id=? AND state='held'", crash_run
                )
            )
            recovered = op("main", "inspect", crash_run)
            ctx.equal(
                "cold owner holds unresolved original without retry",
                [(a[1]) for a in recovered["attempts"]],
                ["unknown"],
            )
            ctx.rejects(
                "unknown cost buys no follow-up",
                queue.Empty,
                lambda: server.wires.get(timeout=0.4),
            )
            interrupt.append(
                {"owner_killed": True, "wire": physical_raw, "recovered": recovered}
            )
            op("main", "revoke", "fixture-self")
            op(
                "main",
                "trigger",
                "fixture-self",
                "revoked",
                json.dumps({"text": "denied"}),
                ok=False,
            )
            op("person", "set-enabled", actor, "false")
            child = next(o for o in origin if o["kind"] == "child")["execution"]
            op("main", "inspect", child, ok=False)
            op("person", "set-enabled", actor, "true")
            ctx.equal(
                "parent authority restored for original child",
                op("main", "inspect", child)["root"],
                first_exec,
            )
            op("privacy", "erase", "clear-primary", actor, json.dumps([first]))
            op("main", "inspect", first_exec, ok=False)
            ctx.check(
                "derived child no longer publishes material",
                rows(
                    "SELECT count(*) FROM main_turns t JOIN executions x ON x.id=t.execution WHERE t.result IS NOT NULL AND x.origin='fixture-child'"
                )
                == [(0,)],
            )
            ctx.check(
                "independent plugin result remains readable",
                op("main", "inspect", tool_run)["state"] == "completed",
            )
    for name, key, cases in [
        ("entry-origin", "H01", origin),
        ("input-layout", "H03", layout),
        ("chat-structured-mode", "H16", structured),
        ("provider-interruption", "H20", interrupt),
        ("steer-boundary", "H02", boundary),
    ]:
        ctx.write(name + ".json", {"cases": {key: cases}})
    return {
        "human_sources": [first, second],
        "internal_origins": origin,
        "transport_interruptions": interrupt,
        "structured": structured,
    }
