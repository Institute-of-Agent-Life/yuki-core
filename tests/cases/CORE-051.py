"""Exercise namespace isolation and original paid turns through installed SDK peers."""

import hashlib
import importlib.util
import json
import os
import subprocess
import sys
from tests.harness.compact import Owner, reply
from tests.peers.chat.recorder import Recorder


def install(
    ctx, owner, sdk, folder, http, plugin, *, entries=None, background=True, tool=False,
    dialect="chat",
):
    folder.mkdir()
    methods = json.loads(
        (ctx.root / "protocol/plugin/sessions/methods.json").read_bytes()
    )["methods"]
    with (owner.svc.data / "yuki.toml").open("a", encoding="utf-8") as f:
        f.write(
            "\n[[plugins]]\nid="
            + json.dumps(plugin)
            + "\nnamespace="
            + json.dumps(plugin)
            + "\nprogram="
            + json.dumps(sys.executable)
            + "\nargs="
            + json.dumps(
                [
                    str(ctx.root / "tests/peers/aux-model/peer.py"),
                    str(folder),
                    str(sdk / "python-install"),
                ]
            )
            + "\ngrants="
            + json.dumps(
                [
                    "observe.metadata",
                    "forget.owner",
                    "llm.complete",
                    "llm.result",
                    *methods,
                ]
                + (["tool.call"] if tool else [])
            )
            + "\n"
        )
        f.write(
            '\n[plugins.sessions]\nroutes=["game"]\nbackground='
            + str(background).lower()
            + '\ncontext_profiles=["none","current_user","current_group"]\n'
        )
        f.write(
            '\n[plugins.aux_routes.game]\npurposes=["game"]\norigins='
            + json.dumps(["session", "event", "call"] if entries else ["session"])
            + "\nentries="
            + json.dumps(entries or [])
            + "\n[plugins.aux_routes.game.profile]\nendpoint="
            + json.dumps(http.url)
            + "\nprotocol="
            + json.dumps(dialect)
            + '\nmodel="session-fixture"\nname="Game"\nmerge_ms=0\nroot_slots=1\nmax_input_units=1000000\nmax_output=4096\n'
        )
        if tool:
            f.write(
                '\n[[plugins.tools]]\nname="session_game"\ndescription="Run an isolated game in the original Work"\ncacheable=false\nparallel_read=false\ncode_direct=false\nschema={type="object",properties={},additionalProperties=false}\n'
            )


def create(session, key, persistence="durable", **extra):
    return {
        "session": session,
        "key": key,
        "name": "Original game",
        "instructions": "Continue the game using only its own visible transcript.",
        "route": "game",
        "purpose": "game",
        "persistence": persistence,
        "context_profile": "none",
        **extra,
    }


def run(ctx):
    spec = importlib.util.spec_from_file_location(
        "aux_fixture", ctx.root / "tests/cases/CORE-050.py"
    )
    aux = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(aux)
    sdk = ctx.working / "sdk"
    built = subprocess.run(
        [
            sys.executable,
            str(ctx.root / "ci/publish-contract.py"),
            "build",
            "--out",
            str(ctx.output / "sdk-distributions"),
            "--verify-work",
            str(sdk),
        ],
        capture_output=True,
        text=True,
    )
    if built.returncode:
        ctx.write("sdk-failure.json", {"stderr": built.stderr})
    ctx.equal("installed independent SDK packages verify", built.returncode, 0)
    owner = Owner(ctx, "isolated-sessions", 33)
    session = "00000000-0000-4000-8000-000000000051"
    ephemeral = "00000000-0000-4000-8000-000000000052"
    with Recorder() as http:
        folders = {name: ctx.working / name for name in ["first-game", "second-game"]}
        for name, folder in folders.items():
            install(ctx, owner, sdk, folder, http, name)
        peers = {name: aux.Peer(owner, folder) for name, folder in folders.items()}
        a, b = peers.values()
        with owner.svc.running():
            for folder in folders.values():
                owner.until((folder / "ready.json").exists)
            negotiated = sorted(
                m
                for m in json.loads(
                    (folders["first-game"] / "ready.json").read_bytes()
                )["methods"]
                if m.startswith("session.")
            )
            expected = sorted(
                json.loads(
                    (ctx.root / "protocol/plugin/sessions/methods.json").read_bytes()
                )["methods"]
            )
            ctx.equal(
                "actual peer negotiates all session methods", negotiated, expected
            )
            alternate_uuid = "00000000-0000-7000-0000-00000000A059"
            normalized = a.call("session.create", create(alternate_uuid, "uuid-form"))
            ctx.equal(
                "opaque UUID text is canonicalized without inventing a UUID version",
                normalized["session"],
                alternate_uuid.lower(),
            )
            ctx.equal(
                "canonical UUID replay keeps the same original session",
                a.call("session.create", create(alternate_uuid.lower(), "uuid-form")),
                normalized,
            )
            a.call(
                "session.create",
                create(ephemeral, "empty-instructions", instructions="\u001c \n"),
                False,
            )
            first = a.call("session.create", create(session, "create"))
            second = b.call("session.create", create(session, "create"))
            ctx.equal(
                "same UUID exists in both authenticated namespaces",
                [first["session"], second["session"]],
                [session, session],
            )
            a.call(
                "session.create",
                create(ephemeral, "invent-user", context_profile="current_user"),
                False,
            )
            a.call(
                "session.create",
                create(ephemeral, "invent-group", context_profile="current_group"),
                False,
            )
            a.call(
                "session.create",
                {**create(ephemeral, "invent-actor"), "actor": owner.actor},
                False,
            )
            append_a = {
                "session": session,
                "epoch": 0,
                "key": "opening",
                "text": "First plugin opening",
            }
            a.call(
                "session.run",
                {
                    "session": session,
                    "epoch": 0,
                    "key": "empty-control-input",
                    "user_input": "\u001c",
                },
                False,
            )
            own_a = a.call("session.append", append_a)
            a.call(
                "session.run",
                {
                    "session": session,
                    "epoch": 0,
                    "key": "opening",
                    "user_input": "changed mutation kind",
                },
                False,
            )
            ctx.equal(
                "mutation-key conflict cannot reserve a model",
                owner.rows("SELECT count(*) FROM model_requests"),
                [(0,)],
            )
            b.call(
                "session.append", {**append_a, "text": "Other plugin private opening"}
            )
            ctx.equal(
                "same append key returns original sequence",
                a.call("session.append", append_a),
                own_a,
            )
            a.call("session.append", {**append_a, "text": "changed"}, False)
            long_text = "雪𠮷" * 4000
            large = b.call(
                "session.append",
                {"session": session, "epoch": 0, "key": "large", "text": long_text},
            )
            row = b.call("session.page", {"session": session})["items"][1]
            ctx.equal(
                "large session text has a bounded preview",
                [len(row["text"]), row["complete"]],
                [512, False],
            )
            import base64

            chunks, offset = [], 0
            while True:
                block = b.call(
                    "session.content",
                    {
                        "session": session,
                        "sequence": large["sequence"],
                        "sha256": row["sha256"],
                        "offset": offset,
                    },
                )
                chunks.append(base64.b64decode(block["bytes"]))
                if block["next_offset"] is None:
                    break
                offset = block["next_offset"]
            raw_content = b"".join(chunks)
            ctx.equal(
                "content pages retain exact UTF-8 bytes",
                raw_content.decode(),
                long_text,
            )
            ctx.equal(
                "session content pages share one digest",
                hashlib.sha256(raw_content).hexdigest(),
                row["sha256"],
            )
            a.call(
                "session.content",
                {
                    "session": session,
                    "sequence": large["sequence"],
                    "sha256": row["sha256"],
                },
                False,
            )
            own_page = a.call("session.page", {"session": session})
            ctx.equal(
                "namespace page contains only its own transcript",
                [v["text"] for v in own_page["items"]],
                ["First plugin opening"],
            )
            request = {
                "session": session,
                "epoch": 0,
                "key": "original-model-turn",
                "user_input": "\u001c Start this private game \u001f",
            }
            job = a.submit("session.run", request)
            raw, q = http.receive()
            wire = json.loads(raw)
            ctx.check(
                "isolated request has no Main tools",
                not wire.get("tools") and wire["tool_choice"] == "none",
            )
            ctx.equal(
                "original visible history reaches physical provider",
                [m["content"] for m in wire["messages"] if m["role"] == "user"],
                ["First plugin opening", "Start this private game"],
            )
            pending = a.call("session.run", request)
            ctx.equal("overlapping original run reports its in-flight turn", pending["state"], "prepared")
            ctx.equal("overlapping original run appends one user and reserves once",
                      owner.rows("SELECT user_sequence,(SELECT count(*) FROM model_requests) FROM plugin_session_turns"),
                      [(2, 1)])
            ctx.check("overlapping original run dispatches no duplicate HTTP", http.wires.empty())
            q.put(
                (
                    {
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "content": "Visible game reply",
                                    "reasoning_content": "hidden-session-canary",
                                },
                                "finish_reason": "stop",
                            }
                        ],
                        "usage": {"prompt_tokens": 10, "completion_tokens": 5},
                    },
                    0,
                )
            )
            done = a.receive(job)
            ctx.equal("pending receipt settles on the same original turn and request",
                      [done["turn"], done["request"]], [pending["turn"], pending["request"]])
            ctx.equal("visible reply recorded", done["text"], "Visible game reply")
            a.call(
                "session.reset",
                {"session": session, "epoch": 0, "key": "original-model-turn"},
                False,
            )
            a.call(
                "session.append",
                {
                    "session": session,
                    "epoch": 0,
                    "key": "original-model-turn",
                    "text": "changed operation",
                },
                False,
            )
            ctx.equal(
                "visible assistant sequence is original", done["assistant_sequence"], 3
            )
            a.call("llm.result", {"request": done["request"]}, False)
            snapshot = owner.rows("SELECT id FROM model_attempts")
            ctx.equal(
                "same run keeps exact original receipt",
                a.call("session.run", request),
                done,
            )
            ctx.equal(
                "known result replay buys no replacement",
                owner.rows("SELECT id FROM model_attempts"),
                snapshot,
            )
            ctx.check("known result replay makes no HTTP", http.wires.empty())
            a.call("session.run", {**request, "user_input": "different"}, False)
            continuation = {
                **request,
                "key": "next-model-turn",
                "user_input": "Continue from the visible reply",
            }
            job = a.submit("session.run", continuation)
            raw, q = http.receive()
            ctx.check(
                "hidden reasoning never enters the next session request",
                "hidden-session-canary" not in raw,
            )
            ctx.equal(
                "assistant role preserved on actual continuation",
                [
                    m["content"]
                    for m in json.loads(raw)["messages"]
                    if m["role"] == "assistant"
                ],
                ["Visible game reply"],
            )
            reply(q, "Second visible reply")
            a.receive(job)
            ctx.equal(
                "no session turn becomes a Main or Memory event",
                owner.rows(
                    "SELECT (SELECT count(*) FROM events),(SELECT count(*) FROM main_turns),(SELECT count(*) FROM memory_facts)"
                ),
                [(0, 0, 0)],
            )
            ctx.equal(
                "actorless model runs have no fabricated conversation",
                owner.rows(
                    "SELECT count(*) FROM main_origins WHERE conversation IS NULL AND kind='plugin'"
                ),
                [(2,)],
            )
            ctx.check(
                "private transcript excludes provider reasoning",
                all(
                    "hidden-session-canary" not in body
                    for body, in owner.rows(
                        "SELECT body FROM plugin_session_messages WHERE body IS NOT NULL"
                    )
                ),
            )
            reset_request = {"session": session, "epoch": 0, "key": "reset"}
            reset = a.call("session.reset", reset_request)
            ctx.equal("reset advances original epoch", reset["epoch"], 1)
            ctx.equal(
                "reset is idempotent", a.call("session.reset", reset_request), reset
            )
            ctx.equal(
                "reset clears visible old content",
                a.call("session.page", {"session": session})["items"],
                [],
            )
            ctx.equal(
                "old paid key remains redacted",
                a.call("session.run", request)["state"],
                "redacted",
            )
            ctx.check(
                "reset keeps original model requests with redacted bodies",
                all(
                    body == "{}"
                    for body, in owner.rows("SELECT request_json FROM model_requests")
                ),
            )
            fresh = {
                **request,
                "epoch": 1,
                "key": "after-reset",
                "user_input": "New visible game turn",
            }
            job = a.submit("session.run", fresh)
            raw, q = http.receive()
            ctx.check(
                "reset old transcript cannot enter a new model request",
                "Visible game reply" not in raw,
            )
            reply(q, "After reset reply")
            after = a.receive(job)
            ctx.equal(
                "sequence never rewinds across reset",
                [after["user_sequence"], after["assistant_sequence"]],
                [6, 7],
            )
            a.call("session.create", create(ephemeral, "ephemeral", "ephemeral"))
            a.call(
                "session.append",
                {
                    "session": ephemeral,
                    "epoch": 0,
                    "key": "temporary",
                    "text": "ephemeral-session-canary",
                },
            )
        with owner.svc.running():
            page = a.call("session.page", {"session": session})
            ctx.equal(
                "durable session resumes its own current epoch",
                [v["text"] for v in page["items"]],
                ["New visible game turn", "After reset reply"],
            )
            a.call("session.page", {"session": ephemeral}, False)
            ctx.equal(
                "ephemeral content expires on core boot",
                owner.rows(
                    "SELECT state,instructions FROM plugin_sessions WHERE plugin='first-game' AND id=?",
                    ephemeral,
                ),
                [("expired", None)],
            )
            ctx.equal(
                "other plugin remains independently readable",
                b.call("session.page", {"session": session})["items"][0]["text"],
                "Other plugin private opening",
            )
            ctx.equal(
                "all session foreign keys survive restart",
                owner.rows("PRAGMA foreign_key_check"),
                [],
            )
            a.call("session.close", {"session": session, "epoch": 1, "key": "close"})
            a.call("session.page", {"session": session}, False)
            proof = {
                "implementation_stage": "namespace, visible paid continuation and reset; kill and source interleavings pending",
                "same_uuid": session,
                "first_paid": done,
                "after_reset": after,
                "negotiated_session_methods": negotiated,
                "binary_sha256": hashlib.sha256(
                    (
                        ctx.root
                        / "target/debug"
                        / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
                    ).read_bytes()
                ).hexdigest(),
                "host_idl_sha256": hashlib.sha256(
                    (ctx.root / "sdk/rust/src/host-preview.json").read_bytes()
                ).hexdigest(),
                "session_schema_sha256": hashlib.sha256(
                    (ctx.root / "protocol/plugin/sessions/methods.json").read_bytes()
                ).hexdigest(),
            }

    proof["prior_schema"] = prior_schema(ctx)
    proof["restart_cuts"] = restart_cuts(ctx, sdk, aux)
    proof["source_erasure"] = source_erasure(ctx, sdk, aux)
    proof["late_fences"] = late_fences(ctx, sdk, aux)
    proof["parent_budget"] = parent_budget(ctx, sdk, aux)
    proof["native_wires"] = native_wires(ctx, sdk, aux)
    proof["scope_compatibility"] = scope_compatibility(ctx, sdk, aux)
    proof["implementation_stage"] = (
        "namespace, visible paid turns, kill recovery and source erasure"
    )
    return proof


def scope_compatibility(ctx, sdk, aux):
    owner = Owner(ctx, "session-scope-compatibility", 33)
    person_b = "00000000-0000-4000-8000-000000000060"
    with owner.svc.running():
        owner.op("person", "create", person_b)
        user_b = owner.op(
            "account", "open", "qq", "compact", "202", 1,
            "--person", person_b, "--start", 1,
        )
        private = owner.op(
            "entry", "private", owner.user["assignment"], owner.bot["assignment"]
        )["entry"]
        group_a = owner.op("entry", "group", "session-scope", "--topic", "a")["entry"]
        group_b = owner.op("entry", "group", "session-scope", "--topic", "b")["entry"]
        other = owner.op("entry", "group", "other-session-scope")["entry"]
    folder = ctx.working / "scope-compatibility-peer"
    with Recorder() as http:
        install(
            ctx, owner, sdk, folder, http, "scope-game",
            entries=[private, group_a, group_b, other],
        )
        peer = aux.Peer(owner, folder)
        with owner.svc.running():
            owner.until((folder / "ready.json").exists)

            def authority(account, key, **scope):
                event = owner.op(
                    "ingress", "record", json.dumps({
                        "source": key, "account": account,
                        "bot_stage": owner.bot["assignment"], "kind": "message",
                        "observed": 20, "occurred": 20,
                        "segments": [{"type": "text", "text": key}], **scope,
                    }),
                )["event"]
                owner.until((folder / ("event-" + str(event) + ".json")).exists)
                return {"event": event}

            a_private = authority(owner.user["account"], "scope-private")
            a_group = authority(
                owner.user["account"], "scope-group-a", space="session-scope", topic="a"
            )
            a_topic = authority(
                owner.user["account"], "scope-group-b", space="session-scope", topic="b"
            )
            b_group = authority(
                user_b["account"], "scope-person-b", space="session-scope", topic="b"
            )
            a_other = authority(
                owner.user["account"], "scope-other-group", space="other-session-scope"
            )
            user_session = "00000000-0000-4000-8000-000000000061"
            group_session = "00000000-0000-4000-8000-000000000062"
            plugin_session = "00000000-0000-4000-8000-000000000063"
            peer.call("session.create", create(user_session, "user", authority=a_private))
            peer.call("session.create", create(group_session, "group", authority=a_group))
            peer.call("session.create", create(plugin_session, "plugin"))
            for session, current in [(user_session, a_group), (group_session, a_topic)]:
                peer.call("session.append", {
                    "session": session, "authority": current, "key": "same-owner",
                    "epoch": 0, "text": "Original owner's continued game",
                })
                page = peer.call("session.page", {"session": session, "authority": current})
                ctx.equal("original Person can continue across permitted entries " + session,
                          page["items"][0]["text"], "Original owner's continued game")
                peer.call("session.page", {"session": session, "authority": b_group}, False)
            peer.call("session.page", {"session": group_session, "authority": a_other}, False)
            peer.call("session.page", {"session": group_session, "authority": a_private}, False)
            peer.call("session.append", {
                "session": plugin_session, "authority": a_group, "key": "real-sender",
                "epoch": 0, "text": "Shared plugin-owned game",
            })
            shared = peer.call("session.page", {"session": plugin_session, "authority": b_group})
            ctx.equal("explicit plugin scope remains shared inside one authenticated namespace",
                      shared["items"][0]["text"], "Shared plugin-owned game")
            ctx.equal("real invocation stamps sender without changing plugin ownership",
                      owner.rows("SELECT s.scope_kind,s.person,s.space,m.sender FROM plugin_sessions s JOIN plugin_session_messages m ON m.plugin=s.plugin AND m.session=s.id WHERE s.id=?", plugin_session),
                      [("plugin", None, None, owner.actor)])
            request = {"session": user_session, "authority": a_group, "key": "group-turn",
                       "epoch": 0, "user_input": "Continue my original personal session"}
            job = peer.submit("session.run", request)
            _, q = http.receive()
            reply(q, "Original personal session continued")
            result = peer.receive(job)
            ctx.equal("cross-scene run retains original visible history", result["text"],
                      "Original personal session continued")
            return {"user": user_session, "group": group_session, "plugin": plugin_session,
                    "result": result, "owners": owner.rows(
                        "SELECT id,scope_kind,person,space FROM plugin_sessions ORDER BY id")}


def prior_schema(ctx):
    import sqlite3
    from contextlib import closing
    from tests.harness.service import Service, previous_schema

    svc = Service(ctx, ctx.working / "stored-schema-33")
    previous_schema(ctx, svc.data, 33)
    with closing(sqlite3.connect(svc.data / "core.sqlite")) as db:
        db.execute("PRAGMA foreign_keys=ON")
        db.executescript(
            """
        INSERT INTO entries(id,scope,space) VALUES('prior-entry','group','prior-space');
        INSERT INTO main_origins(ref,kind,conversation) VALUES('prior-origin','plugin','prior-entry');
        INSERT INTO executions(id,origin,state) VALUES('prior-execution','prior-origin','completed');
        INSERT INTO main_turns(execution,conversation,window,ready_after,result) VALUES('prior-execution','prior-entry',1,0,'{"text":"prior result"}');
        INSERT INTO operations(namespace,operation_id,action,target,payload,state,result) VALUES('prior-execution','prior-operation','get','prior-target','{}','confirmed','{"value":"prior receipt"}');
        INSERT INTO root_budgets(root,max_requests,max_units,reserved_requests,reserved_units) VALUES('prior-execution',1,100,1,2);
        INSERT INTO budget_members VALUES('prior-execution','prior-execution');
        INSERT INTO activations VALUES('prior-activation','prior-execution','prior-profile');
        INSERT INTO model_requests(id,activation,ordinal,root,estimated_input,max_output,retry_unbilled,native_tools,request_json) VALUES('prior-logical','prior-activation',0,'prior-execution',1,1,0,0,'{"model":"prior"}');
        INSERT INTO model_attempts(id,logical,ordinal,state,response,usage_json,intents_json,status_code) VALUES('prior-attempt','prior-logical',0,'response','{"content":"prior answer"}','{"input":1}','[]',200);
        """
        )
        for kind in ["self", "schedule", "work", "child", "memory_extract", "embedding"]:
            db.execute(
                "INSERT INTO main_origins(ref,kind,conversation,parent) VALUES(?,?,'prior-entry','prior-execution')",
                ("prior-" + kind, kind),
            )
            db.execute(
                "INSERT INTO executions(id,origin,state) VALUES(?,?,'completed')",
                ("prior-execution-" + kind, "prior-" + kind),
            )
        db.commit()
        before = [
            db.execute(sql).fetchall()
            for sql in [
                "SELECT ref,kind,conversation,parent,enabled FROM main_origins",
                "SELECT id,origin,state FROM executions",
                "SELECT operation_id,state,result FROM operations",
                "SELECT id,state,response,usage_json FROM model_attempts",
            ]
        ]
        ctx.equal(
            "prior snapshot has valid original references",
            db.execute("PRAGMA foreign_key_check").fetchall(),
            [],
        )
    version = svc.command("migrate")["schema_version"]
    ctx.check("native migration upgrades populated schema 33", version >= 34)
    with closing(sqlite3.connect(svc.data / "core.sqlite")) as db:
        after = [
            db.execute(sql).fetchall()
            for sql in [
                "SELECT ref,kind,conversation,parent,enabled FROM main_origins",
                "SELECT id,origin,state FROM executions",
                "SELECT operation_id,state,result FROM operations",
                "SELECT id,state,response,usage_json FROM model_attempts",
            ]
        ]
        ctx.equal(
            "old execution and paid receipt identities survive nullable scene support",
            after,
            before,
        )
        ctx.equal(
            "migrated references remain intact",
            db.execute("PRAGMA foreign_key_check").fetchall(),
            [],
        )
    return {"schema": version, "retained": after}


def restart_cuts(ctx, sdk, aux):
    from tests.harness.crash import kill_writer
    from concurrent.futures import ThreadPoolExecutor

    proofs = []
    for phase in [
        "aux-reserved-before-dispatch",
        "aux-dispatched-before-http",
        "aux-response-before-reply",
    ]:
        owner = Owner(ctx, "sessions-" + phase, 33)
        session = "00000000-0000-4000-8000-000000000053"
        folder = ctx.working / (phase + "-session-peer")
        with Recorder() as http:
            install(ctx, owner, sdk, folder, http, "cut-game")
            peer = aux.Peer(owner, folder)
            request = {
                "session": session,
                "key": "original-paid-cut",
                "epoch": 0,
                "user_input": "Keep this original game turn",
            }
            with ThreadPoolExecutor(max_workers=1) as pool:
                killed = pool.submit(
                    kill_writer,
                    ctx,
                    owner.svc.data,
                    "work_delivery_crash_child",
                    phase,
                    {},
                )
                owner.until((folder / "ready.json").exists)
                peer.call("session.create", create(session, "cut-session"))
                peer.submit("session.run", request)
                physical = 0
                if phase == "aux-response-before-reply":
                    _, q = http.receive()
                    physical += 1
                    reply(q, "Original paid session reply")
                killed.result()
            logicals = owner.rows("SELECT id FROM model_requests")
            ctx.equal(
                "user sequence and request reserved in one durable turn " + phase,
                owner.rows(
                    "SELECT user_sequence,assistant_sequence,state FROM plugin_session_turns"
                ),
                [(1, None, "prepared")],
            )
            with owner.svc.running():
                job = peer.submit("session.run", request)
                if phase == "aux-reserved-before-dispatch":
                    _, q = http.receive()
                    physical += 1
                    reply(q, "Original paid session reply")
                result = peer.receive(job)
                expected_state = "unknown" if phase == "aux-dispatched-before-http" else "confirmed"
                ctx.write("session-cut-receipt-" + phase + ".json", {
                    "result": result,
                    "turns": owner.rows("SELECT id,state,assistant_sequence FROM plugin_session_turns"),
                    "attempts": owner.rows("SELECT id,state FROM model_attempts"),
                })
                ctx.check("concurrent recovery reports pending or original settled turn " + phase,
                          result["state"] in {"prepared", expected_state})
                # The restarted inbox also replays the pre-kill job. An overlapping
                # callback can observe its original attempt before that owner settles.
                owner.until(lambda: owner.rows(
                    "SELECT id FROM plugin_session_turns WHERE state=?", expected_state
                ))
                result = peer.call("session.run", request)
                ctx.equal(
                    "restart preserves original logical request " + phase,
                    owner.rows("SELECT id FROM model_requests"),
                    logicals,
                )
                ctx.equal(
                    "one reservation across recovery " + phase,
                    owner.rows("SELECT reserved_requests FROM root_budgets"),
                    [(1,)],
                )
                if phase == "aux-dispatched-before-http":
                    ctx.equal(
                        "unknown session request is never repurchased",
                        result["state"],
                        "unknown",
                    )
                    ctx.equal(
                        "unknown has only original user sequence",
                        owner.rows("SELECT role FROM plugin_session_messages"),
                        [("user",)],
                    )
                else:
                    ctx.equal(
                        "known paid response appends its original assistant once "
                        + phase,
                        [result["state"], result["assistant_sequence"], result["text"]],
                        ["confirmed", 2, "Original paid session reply"],
                    )
                ctx.check(
                    "no duplicate physical request after recovery " + phase,
                    http.wires.empty(),
                )
                proofs.append(
                    {
                        "phase": phase,
                        "physical_requests": physical,
                        "logical": logicals,
                        "result": result,
                        "attempts": owner.rows(
                            "SELECT id,state,ever_unknown FROM model_attempts"
                        ),
                    }
                )
    return proofs


def source_erasure(ctx, sdk, aux):
    owner = Owner(ctx, "session-source", 33)
    person_b = "00000000-0000-4000-8000-000000000054"
    with owner.svc.running():
        owner.op("person", "create", person_b)
        user_b = owner.op(
            "account",
            "open",
            "qq",
            "compact",
            "201",
            1,
            "--person",
            person_b,
            "--start",
            1,
        )
        entry = owner.op("entry", "group", "session-group")["entry"]
    folder = ctx.working / "scoped-session-peer"
    with Recorder() as http:
        install(
            ctx,
            owner,
            sdk,
            folder,
            http,
            "scoped-game",
            entries=[entry],
            background=False,
        )
        peer = aux.Peer(owner, folder)
        with owner.svc.running():
            owner.until((folder / "ready.json").exists)

            def record(account, key, text):
                event = owner.op(
                    "ingress",
                    "record",
                    json.dumps(
                        {
                            "source": key,
                            "account": account,
                            "bot_stage": owner.bot["assignment"],
                            "space": "session-group",
                            "kind": "message",
                            "observed": 20,
                            "occurred": 20,
                            "segments": [{"type": "text", "text": text}],
                        }
                    ),
                )["event"]
                owner.until((folder / ("event-" + str(event) + ".json")).exists)
                return event

            event_a = record(
                owner.user["account"], "session-a", "original-session-private-canary"
            )
            event_b = record(
                user_b["account"], "session-b", "unrelated group member source"
            )
            session_a = "00000000-0000-4000-8000-000000000055"
            session_b = "00000000-0000-4000-8000-000000000056"
            authority_a, authority_b = {"event": event_a}, {"event": event_b}
            peer.call("session.create", create(session_a, "omitted-actor"), False)
            peer.call(
                "session.create",
                create(
                    session_a,
                    "source-a",
                    authority=authority_a,
                    context_profile="current_group",
                ),
            )
            peer.call(
                "session.create",
                create(
                    session_b,
                    "source-b",
                    authority=authority_b,
                    context_profile="current_group",
                ),
            )
            peer.call(
                "session.page", {"session": session_a, "authority": authority_b}, False
            )
            peer.call(
                "session.append",
                {
                    "session": session_b,
                    "authority": authority_b,
                    "key": "b-append",
                    "epoch": 0,
                    "text": "B independently retained game text",
                },
            )
            initial_sources = peer.call(
                "session.page", {"session": session_a, "authority": authority_a}
            )["sources"]
            extra_sources = []
            for index in range(129):
                event = record(
                    owner.user["account"],
                    "session-dependency-" + str(index),
                    "original dependency " + str(index),
                )
                extra_sources.append(event)
                peer.call(
                    "session.append",
                    {
                        "session": session_a,
                        "authority": {"event": event},
                        "key": "dependency-" + str(index),
                        "epoch": 0,
                        "text": "retained game source " + str(index),
                    },
                )
            header = peer.call(
                "session.page", {"session": session_a, "authority": authority_a}
            )["sources"]
            ctx.equal(
                "large source sets stay within the frame budget",
                [header["count"], len(header["ids"]), header["complete"]],
                [130, 128, False],
            )
            peer.call(
                "session.sources",
                {
                    "session": session_a,
                    "authority": authority_a,
                    "version": initial_sources["version"],
                },
                False,
            )
            ids, after = [], 0
            while True:
                sources_page = peer.call(
                    "session.sources",
                    {
                        "session": session_a,
                        "authority": authority_a,
                        "version": header["version"],
                        "after": after,
                    },
                )
                ids.extend(sources_page["ids"])
                if sources_page["next"] is None:
                    break
                after = sources_page["next"]
            ctx.equal(
                "all retained dependencies are available for owner erasure",
                ids,
                [event_a, *extra_sources],
            )
            request = {
                "session": session_a,
                "authority": authority_a,
                "key": "a-model",
                "epoch": 0,
                "user_input": "original-session-private-canary",
            }
            job = peer.submit("session.run", request)
            raw, q = http.receive()
            ctx.check(
                "trusted group metadata uses original caller",
                owner.actor in raw and "session-group" in raw,
            )
            before = owner.rows("SELECT count(*) FROM model_requests")
            peer.call(
                "session.append",
                {
                    "session": session_a,
                    "authority": authority_a,
                    "key": "competing-append",
                    "epoch": 0,
                    "text": "must not interleave",
                },
                False,
            )
            owner.op("privacy", "forget", "session-owner-forget", owner.actor)
            q.put(
                (
                    {
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "content": "late-session-canary",
                                },
                                "finish_reason": "stop",
                            }
                        ],
                        "usage": {"prompt_tokens": 11, "completion_tokens": 3},
                    },
                    0,
                )
            )
            peer.receive(job, False)
            owner.until(
                lambda: not owner.op("privacy", "pending", "scoped-game")["obligations"]
            )
            peer.call(
                "session.page", {"session": session_a, "authority": authority_a}, False
            )
            ctx.equal(
                "late response cannot recreate erased session text",
                owner.rows(
                    "SELECT count(*) FROM plugin_session_messages WHERE session=? AND body IS NOT NULL",
                    session_a,
                ),
                [(0,)],
            )
            ctx.equal(
                "late known usage survives session erasure",
                owner.rows(
                    "SELECT state,response,json_extract(usage_json,'$.input') FROM model_attempts"
                ),
                [("response_held", None, 11)],
            )
            ctx.equal(
                "forget retains original request identities",
                owner.rows("SELECT count(*) FROM model_requests"),
                before,
            )
            ctx.equal(
                "forget clears frozen model input",
                owner.rows("SELECT request_json FROM model_requests"),
                [("{}",)],
            )
            ctx.equal(
                "unrelated same-group session remains readable",
                peer.call(
                    "session.page", {"session": session_b, "authority": authority_b}
                )["items"][0]["text"],
                "B independently retained game text",
            )
            ctx.equal(
                "isolated turns never enter Main or Memory",
                owner.rows(
                    "SELECT (SELECT count(*) FROM main_turns),(SELECT count(*) FROM memory_facts)"
                ),
                [(0, 0)],
            )
            return {
                "source": event_a,
                "other_source": event_b,
                "sessions": owner.rows(
                    "SELECT id,state FROM plugin_sessions ORDER BY id"
                ),
                "attempts": owner.rows(
                    "SELECT id,state,usage_json FROM model_attempts"
                ),
                "private_scope_denial": True,
            }


def late_fences(ctx, sdk, aux):
    proofs = []
    for action in ["reset", "close", "withdraw"]:
        owner = Owner(ctx, "session-late-" + action, 33)
        folder = ctx.working / ("session-late-peer-" + action)
        session = "00000000-0000-4000-8000-000000000057"
        with Recorder() as http:
            install(ctx, owner, sdk, folder, http, "late-game")
            peer = aux.Peer(owner, folder)
            with owner.svc.running():
                owner.until((folder / "ready.json").exists)
                peer.call("session.create", create(session, "create"))
                request = {
                    "session": session,
                    "key": "original-late",
                    "epoch": 0,
                    "user_input": "Original in-flight session input",
                }
                job = peer.submit("session.run", request)
                _, q = http.receive()
                if action == "withdraw":
                    owner.op("host", "enable", "late-game", "false")
                    owner.until(
                        lambda: owner.rows(
                            "SELECT id FROM model_attempts WHERE state='unknown'"
                        )
                    )
                else:
                    peer.call(
                        "session." + action,
                        {"session": session, "key": "during-flight", "epoch": 0},
                    )
                q.put(
                    (
                        {
                            "choices": [
                                {
                                    "message": {
                                        "role": "assistant",
                                        "content": "late-fenced-session-canary",
                                    },
                                    "finish_reason": "stop",
                                }
                            ],
                            "usage": {"prompt_tokens": 17, "completion_tokens": 3},
                        },
                        0,
                    )
                )
                if action == "withdraw":
                    ctx.equal(
                        "withdrawn plugin cannot publish an assistant row",
                        owner.rows(
                            "SELECT count(*) FROM plugin_session_messages WHERE role='assistant'"
                        ),
                        [(0,)],
                    )
                    owner.op("host", "enable", "late-game", "true")
                    result = peer.receive(job)
                    ctx.equal(
                        "reconnected original session preserves unknown",
                        result["state"],
                        "unknown",
                    )
                else:
                    if action == "reset":
                        redacted = peer.receive(job)
                        ctx.equal(
                            "late pre-reset callback returns original redacted identity",
                            redacted["state"],
                            "redacted",
                        )
                        ctx.check(
                            "redacted callback contains no text", "text" not in redacted
                        )
                    else:
                        peer.receive(job, False)
                    ctx.equal(
                        "late session usage survives " + action,
                        owner.rows(
                            "SELECT state,response,json_extract(usage_json,'$.input') FROM model_attempts"
                        ),
                        [("response_held", None, 17)],
                    )
                ctx.equal(
                    "late callback adds no assistant content " + action,
                    owner.rows(
                        "SELECT count(*) FROM plugin_session_messages WHERE role='assistant' AND body IS NOT NULL"
                    ),
                    [(0,)],
                )
                ctx.equal(
                    "late callback cannot buy a replacement " + action,
                    owner.rows("SELECT count(*) FROM model_attempts"),
                    [(1,)],
                )
                ctx.check(
                    "late fence leaves no replacement HTTP " + action,
                    http.wires.empty(),
                )
                if action == "reset":
                    ctx.equal(
                        "pre-reset original key stays redacted",
                        peer.call("session.run", request)["state"],
                        "redacted",
                    )
                proofs.append(
                    {
                        "action": action,
                        "attempts": owner.rows(
                            "SELECT id,state,usage_json FROM model_attempts"
                        ),
                        "sessions": owner.rows(
                            "SELECT id,state,epoch FROM plugin_sessions"
                        ),
                    }
                )
    return proofs


def parent_budget(ctx, sdk, aux):
    from tests.harness.compact import call

    owner = Owner(ctx, "session-original-work", 33)
    with owner.svc.running():
        entry = owner.op(
            "entry", "private", owner.user["assignment"], owner.bot["assignment"]
        )["entry"]
    folder = ctx.working / "nested-session-peer"
    session = "00000000-0000-4000-8000-000000000058"
    with Recorder() as main, Recorder() as http:
        owner.config(main)
        install(
            ctx,
            owner,
            sdk,
            folder,
            http,
            "nested-game",
            entries=[entry],
            background=False,
            tool=True,
        )
        peer = aux.Peer(owner, folder)
        with owner.svc.running():
            owner.until((folder / "ready.json").exists)
            source = owner.human("Run the isolated game inside this original Work")
            _, q = owner.wire(main)
            reply(
                q,
                calls=[
                    call(
                        "accept",
                        "task_control",
                        {"action": "accept", "goal": "original isolated game"},
                    )
                ],
            )
            _, q = owner.wire(main)
            reply(q, calls=[call("game", "session_game", {})])
            owner.until(lambda: list(folder.glob("call-*.json")))
            handle = json.loads(next(folder.glob("call-*.json")).read_bytes())[
                "params"
            ]["handle"]
            authority = {"handle": handle}
            peer.call(
                "session.create",
                create(
                    session,
                    "nested-create",
                    authority=authority,
                    context_profile="current_user",
                ),
            )
            request = {
                "session": session,
                "authority": authority,
                "key": "nested-original",
                "epoch": 0,
                "user_input": "nested isolated input",
            }
            job = peer.submit("session.run", request)
            raw, q = http.receive()
            ctx.check(
                "nested session metadata uses its real human caller", owner.actor in raw
            )
            ctx.check(
                "nested session has no Main tools", not json.loads(raw).get("tools")
            )
            reply(q, "nested isolated reply")
            answer = peer.receive(job)
            ctx.equal(
                "session requests consume original Work root",
                owner.rows("SELECT count(*),sum(reserved_requests) FROM root_budgets"),
                [(1, 3)],
            )
            parent = owner.rows("SELECT id FROM executions WHERE source=?", source)
            ctx.equal(
                "session auxiliary execution retains real parent",
                owner.rows("SELECT parent_execution FROM host_aux_origins"),
                parent,
            )
            work = owner.rows("SELECT work_id FROM executions WHERE source=?", source)
            ctx.equal(
                "session paid request records original Work",
                owner.rows(
                    "SELECT r.original_work_id FROM model_requests r JOIN host_aux_requests q ON q.logical=r.id"
                ),
                work,
            )
            (folder / ("finish-" + handle + ".json")).write_text("{}")
            wire, q = owner.wire(main)
            ctx.check(
                "isolated game transcript is absent from Main continuation",
                "nested isolated reply" not in str(wire)
                and "nested isolated input" not in str(wire),
            )
            reply(
                q,
                calls=[
                    call(
                        "complete",
                        "task_control",
                        {"action": "complete", "result": "original Work finished"},
                    )
                ],
            )
            owner.finished(source)
            ctx.equal(
                "known isolated reply survives parent completion",
                peer.call("session.run", request),
                answer,
            )
            peer.call("session.run", {**request, "key": "closed-parent-new"}, False)
            ctx.check(
                "closed parent and paid replay create no HTTP", http.wires.empty()
            )
            ctx.equal(
                "four original purchases stay on one Work",
                owner.rows("SELECT reserved_requests FROM root_budgets"),
                [(4,)],
            )
            return {
                "source": source,
                "parent": parent,
                "work": work,
                "answer": answer,
                "requests": owner.rows(
                    "SELECT id,root,original_work_id FROM model_requests"
                ),
            }


def native_wires(ctx, sdk, aux):
    proofs = []
    for dialect in [
        "openai_responses",
        "deepseek_responses_legacy",
        "deepseek_responses",
        "anthropic_messages",
        "gemini",
    ]:
        owner = Owner(ctx, "session-wire-" + dialect, 33)
        folder = ctx.working / ("session-wire-peer-" + dialect)
        path = (
            "/v1/messages"
            if dialect == "anthropic_messages"
            else (
                "/v1beta/models/session-fixture:generateContent"
                if dialect == "gemini"
                else "/v1/responses"
            )
        )
        with Recorder(path) as http:
            install(ctx, owner, sdk, folder, http, "wire-game", dialect=dialect)
            peer = aux.Peer(owner, folder)
            session = "00000000-0000-4000-8000-000000000059"
            with owner.svc.running():
                owner.until((folder / "ready.json").exists)
                peer.call("session.create", create(session, "wire-create"))
                frames = []
                for index in range(2):
                    request = {
                        "session": session,
                        "key": "wire-" + str(index),
                        "epoch": 0,
                        "user_input": "Visible input " + str(index),
                    }
                    job = peer.submit("session.run", request)
                    raw, q = http.receive()
                    wire = json.loads(raw)
                    frames.append(wire)
                    ctx.check(
                        "session native request has no declared tools " + dialect,
                        not wire.get("tools"),
                    )
                    if index:
                        ctx.check(
                            "session native continuation excludes thinking " + dialect,
                            "native-session-hidden" not in raw,
                        )
                        if dialect == "gemini":
                            previous = [
                                m["parts"][0]["text"]
                                for m in wire["contents"]
                                if m["role"] == "model"
                            ]
                        elif dialect == "anthropic_messages":
                            previous = [
                                b["text"]
                                for m in wire["messages"]
                                if m["role"] == "assistant"
                                for b in m["content"]
                                if b["type"] == "text"
                            ]
                        else:
                            previous = [
                                m["content"]
                                for m in wire["input"]
                                if m["role"] == "assistant"
                            ]
                        ctx.equal(
                            "native visible assistant history retained " + dialect,
                            previous,
                            ["Visible native reply 0"],
                        )
                    visible = "Visible native reply " + str(index)
                    if dialect == "gemini":
                        response = {
                            "candidates": [
                                {
                                    "content": {
                                        "role": "model",
                                        "parts": [
                                            {
                                                "thought": True,
                                                "text": "native-session-hidden",
                                                "thoughtSignature": "native-session-hidden",
                                            },
                                            {"text": visible},
                                        ],
                                    },
                                    "finishReason": "STOP",
                                }
                            ],
                            "usageMetadata": {
                                "promptTokenCount": 3,
                                "candidatesTokenCount": 2,
                                "totalTokenCount": 5,
                            },
                        }
                    elif dialect == "anthropic_messages":
                        response = {
                            "type": "message",
                            "role": "assistant",
                            "stop_reason": "end_turn",
                            "content": [
                                {
                                    "type": "thinking",
                                    "thinking": "native-session-hidden",
                                    "signature": "native-session-hidden",
                                },
                                {"type": "text", "text": visible},
                            ],
                            "usage": {"input_tokens": 3, "output_tokens": 2},
                        }
                    else:
                        response = {
                            "id": "original-response-" + str(index),
                            "status": "completed",
                            "output": [
                                {
                                    "type": "reasoning",
                                    "encrypted_content": "native-session-hidden",
                                },
                                {
                                    "type": "message",
                                    "role": "assistant",
                                    "content": [
                                        {"type": "output_text", "text": visible}
                                    ],
                                },
                            ],
                            "usage": {
                                "input_tokens": 3,
                                "output_tokens": 2,
                                "total_tokens": 5,
                            },
                        }
                    q.put((response, 0))
                    done = peer.receive(job)
                    ctx.equal(
                        "session persists only native visible text " + dialect,
                        done["text"],
                        visible,
                    )
                    peer.call("llm.result", {"request": done["request"]}, False)
                if dialect == "anthropic_messages":
                    paused = {
                        "session": session,
                        "key": "paused-text",
                        "epoch": 0,
                        "user_input": "Original nonterminal response",
                    }
                    job = peer.submit("session.run", paused)
                    _, q = http.receive()
                    q.put(
                        (
                            {
                                "type": "message",
                                "role": "assistant",
                                "stop_reason": "pause_turn",
                                "content": [
                                    {
                                        "type": "thinking",
                                        "thinking": "native-session-hidden",
                                        "signature": "original-signature",
                                    }
                                ],
                                "usage": {"input_tokens": 7, "output_tokens": 2},
                            },
                            0,
                        )
                    )
                    unsupported = peer.receive(job)
                    ctx.equal(
                        "nonterminal result stays outside text transcript",
                        unsupported["state"],
                        "failed",
                    )
                    ctx.equal(
                        "nonterminal replay preserves original terminal decision",
                        peer.call("session.run", paused),
                        unsupported,
                    )
                    ctx.check(
                        "nonterminal text session does not make an implicit continuation",
                        http.wires.empty(),
                    )
                ctx.equal(
                    "native sessions retain their own visible messages " + dialect,
                    owner.rows(
                        "SELECT body FROM plugin_session_messages WHERE role='assistant' ORDER BY sequence"
                    ),
                    [("Visible native reply 0",), ("Visible native reply 1",)],
                )
                proofs.append(
                    {
                        "protocol": dialect,
                        "requests": frames,
                        "attempts": owner.rows(
                            "SELECT id,state,usage_json FROM model_attempts"
                        ),
                    }
                )
    return proofs
