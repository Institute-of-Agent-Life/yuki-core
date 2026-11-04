"""Clean A during a callback. Kill the owner before delivery of the external erase obligation."""

import json
import sqlite3
import subprocess
import sys
from contextlib import closing
from tests.harness.crash import kill_writer
from tests.harness.service import Service, previous_schema


def run(ctx):
    owner = Service(ctx, ctx.working / "data")
    previous_schema(ctx, owner.data, 6)
    ctx.check(
        "migration from schema 6", owner.command("migrate")["schema_version"] >= 7
    )
    a, b = [f"00000000-0000-4000-8000-{n:012x}" for n in (1, 2)]

    def op(*args, ok=True):
        return owner.command("operator", *map(str, args), accepted=ok)

    def sink(name, cmd, body):
        process = subprocess.run(
            [
                sys.executable,
                str(ctx.root / "tests/peers/erasure/owner.py"),
                str(ctx.working / (name + ".db")),
                cmd,
            ],
            input=json.dumps(body),
            capture_output=True,
            text=True,
            timeout=10,
            check=True,
        )
        return json.loads(process.stdout)

    def privacy(*args, ok=True):
        return op("privacy", *args, ok=ok)

    with owner.running():
        ctx.write("sqlite-build.json", op("database-info"))
        bot = op("account", "open", "qq", "privacy", "bot", 1, "--start", 1)
        frames = []
        events = []
        executions = []
        for person, secret in ((a, "private-A-inbound"), (b, "retained-B-inbound")):
            op("person", "create", person)
            sender = op(
                "account",
                "open",
                "qq",
                "privacy",
                person,
                1,
                "--person",
                person,
                "--start",
                1,
            )
            frame = {
                "source": "source-" + person,
                "native_id": person,
                "account": sender["account"],
                "bot_stage": bot["assignment"],
                "occurred": 2,
                "observed": 3,
                "kind": "message",
                "segments": [{"type": "text", "text": secret}],
            }
            event = op("ingress", "record", json.dumps(frame), "--accept")["event"]
            execution = op("ingress", "claim", event)["execution"]
            frames.append(frame)
            events.append(event)
            executions.append(execution)
        for ns, event in (("one", events[0]), ("two", events[1])):
            privacy("owner", ns, "true")
            receipt = privacy("acquire", ns, "local-ref", event)
            ctx.equal(
                "actual external " + ns + " retains its source",
                sink(ns, "retain", receipt)["retained"],
                1,
            )
        privacy("owner", "no-material", "true")
        for run_id, actor, mark in zip(
            executions, (a, b), ("private-A-result", "retained-B-result")
        ):
            op(
                "effect",
                "intent",
                run_id,
                "operation",
                actor,
                "put",
                "local-note",
                json.dumps({"value": mark}),
            )
            op("effect", "local", run_id, "operation", actor)
        read_frame = dict(
            frames[0],
            source="distinct-A",
            native_id="distinct-A",
            segments=[{"type": "text", "text": "other A event"}],
        )
        read_event = op("ingress", "record", json.dumps(read_frame), "--accept")[
            "event"
        ]
        read_execution = op("ingress", "claim", read_event)["execution"]
        op("effect", "intent", read_execution, "snapshot", a, "get", "local-note", "{}")
        snapshot = op("effect", "local", read_execution, "snapshot", a)
        ctx.check(
            "derived receipt contains value from earlier event",
            "private-A-result" in json.dumps(snapshot),
        )
        op("effect", "intent", executions[0], "external", a, "external", "notify", "{}")
        op("effect", "dispatch", executions[0], "external", a)
        op("model", "root", executions[0], 1, 100)
        op("model", "activate", executions[0], "callback", "synthetic")
        op(
            "model",
            "reserve",
            "callback",
            "logical-A",
            10,
            40,
            "--request",
            json.dumps({"prompt": "private-A-model"}),
        )
        op("model", "dispatch", "logical-A", "attempt-A")
        privacy("erase", "wrong-owner", b, json.dumps([events[0]]), ok=False)
    erase = {
        "method": "erase",
        "key": "forget-A",
        "person": a,
        "sources": json.dumps([events[0]]),
    }
    kill_writer(
        ctx,
        owner.data,
        "privacy_crash_child",
        "after",
        {"YUKI_ERASE_TEST": json.dumps(erase)},
    )
    with owner.running():
        pending = privacy("pending", "one")["obligations"]
        ctx.equal("one original pending owner", len(pending), 1)
        obligation = pending[0]
        ctx.equal(
            "obligation retains erase key and source",
            (obligation["key"], obligation["sources"]),
            ("forget-A", [events[0]]),
        )
        ctx.equal(
            "unrelated owner gets no erasure",
            privacy("pending", "two")["obligations"],
            [],
        )
        ctx.equal(
            "installed owner without retained data has no obligation",
            privacy("pending", "no-material")["obligations"],
            [],
        )
        privacy("read", "one", "local-ref", events[0], ok=False)
        privacy("acquire", "two", "late-A", events[0], ok=False)
        op("effect", "read", executions[0], "operation", a, ok=False)
        op("ingress", "claim", events[0], ok=False)
        snapshot = op("effect", "read", read_execution, "snapshot", a)
        ctx.check(
            "unrelated execution cannot republish an erased snapshot",
            "private-A" not in json.dumps(snapshot),
        )
        op(
            "effect",
            "settle",
            executions[0],
            "external",
            "confirmed",
            json.dumps({"body": "private-A-late-effect"}),
        )
        op(
            "effect",
            "settle",
            executions[0],
            "external",
            "confirmed",
            json.dumps({"body": "private-A-late-effect"}),
        )
        privacy("erase", "additional-erase-key", a, json.dumps([events[0]]))
        ctx.equal(
            "overlap keeps original owner watermark",
            privacy("pending", "one")["obligations"],
            pending,
        )
        late = op(
            "model",
            "response",
            "attempt-A",
            json.dumps({"answer": "private-A-late"}),
            json.dumps({"total": 7, "debug": "private-A-usage"}),
            "[]",
        )
        ctx.check(
            "late physical receipt has no published body",
            late["state"] == "response_held",
        )
        replay = op("ingress", "record", json.dumps(frames[0]), "--accept")
        ctx.check(
            "redelivery never republishes erased input",
            replay["erased"]
            and replay["event"] == events[0]
            and not replay["accepted"],
        )
        ctx.check(
            "unrelated owner still reads its B source",
            "retained-B-inbound"
            in json.dumps(privacy("read", "two", "local-ref", events[1])),
        )
        ctx.check(
            "unrelated local effect remains available",
            "retained-B-result"
            in json.dumps(op("effect", "read", executions[1], "operation", b)),
        )
        privacy("erase", "forget-A", a, json.dumps([events[0]]))
        privacy("erase", "forget-A", b, json.dumps([events[1]]), ok=False)
        privacy("owner", "one", "false")
        ctx.equal(
            "disabled owner stays pending",
            privacy("pending", "one")["obligations"],
            pending,
        )
        privacy("ack", "one", "forget-A", obligation["watermark"], "fixture", ok=False)
    with owner.running():
        privacy("owner", "one", "true")
        privacy("acquire", "one", "new-B", events[1], ok=False)
        ctx.equal(
            "owner physically held content until its own cleanup",
            sink("one", "count", {})["retained"],
            1,
        )
        proof = sink("one", "erase", obligation)
        ctx.equal("external owner verifies logical deletion", proof["retained"], 0)
        ack = privacy("ack", "one", "forget-A", obligation["watermark"], proof["proof"])
        ctx.equal(
            "same-key acknowledgment idempotent",
            privacy("ack", "one", "forget-A", obligation["watermark"], proof["proof"]),
            ack,
        )
        privacy(
            "ack", "one", "forget-A", obligation["watermark"], "different", ok=False
        )
        ctx.equal(
            "original obligation completed",
            privacy("pending", "one")["obligations"],
            [],
        )
        with closing(
            sqlite3.connect(
                (owner.data / "core.sqlite").as_uri() + "?mode=ro", uri=True
            )
        ) as db:
            for table, fields in [
                ("events", "envelope,mentions"),
                ("operations", "payload,result"),
                ("local_effect_values", "value"),
                ("model_requests", "request_json"),
                ("model_attempts", "response,usage_json,intents_json"),
            ]:
                values = str(db.execute(f"SELECT {fields} FROM {table}").fetchall())
                ctx.check("no A body in live " + table, "private-A" not in values)
            ctx.equal(
                "late accounting metadata retained",
                json.loads(
                    db.execute(
                        "SELECT usage_json FROM model_attempts WHERE id='attempt-A'"
                    ).fetchone()[0]
                ),
                {"total": 7},
            )
    return {
        "forget_key": "forget-A",
        "affected_sources": [events[0]],
        "unrelated_source": events[1],
        "before_owner_ack": pending,
        "after_owner_ack": ack,
        "external_owner_logical_count": proof["retained"],
        "physical_storage_reclamation": "not_measured",
    }
