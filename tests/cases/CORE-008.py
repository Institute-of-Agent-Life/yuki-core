"""Crash exactly before SQLite commit and after commit before the platform receives an ACK."""

import json
import sqlite3
from contextlib import closing
from tests.harness.service import Service, previous_schema
from tests.harness.crash import kill_writer


def run(ctx):
    owner = Service(ctx, ctx.working / "data")
    previous_schema(ctx, owner.data, 3)
    ctx.check("schema 3 upgraded", owner.command("migrate")["schema_version"] >= 4)
    actor = "00000000-0000-4000-8000-000000000008"

    def op(*args, **kwargs):
        return owner.command("operator", *args, **kwargs)

    with owner.running():
        ctx.write("sqlite-build.json", op("database-info"))
        op("person", "create", actor)
        person = op(
            "account",
            "open",
            "qq",
            "tenant",
            "human",
            "10",
            "--person",
            actor,
            "--start",
            "10",
        )
        bot = op("account", "open", "qq", "tenant", "bot", "10", "--start", "10")
    base = {
        "source": "before",
        "native_id": "n1",
        "account": person["account"],
        "bot_stage": bot["assignment"],
        "space": None,
        "topic": None,
        "occurred": 20,
        "observed": 21,
        "kind": "message",
        "segments": [
            {"type": "text", "text": "message @x"},
            {"type": "mention", "account": person["account"], "label": "X"},
        ],
    }

    def counts():
        with closing(
            sqlite3.connect(
                (owner.data / "core.sqlite").as_uri() + "?mode=ro", uri=True
            )
        ) as db:
            return [
                db.execute("SELECT count(*) FROM " + t).fetchone()[0]
                for t in ("events", "accepted_sources", "executions")
            ]

    def record(frame, accept=True, **kw):
        return op(
            "ingress",
            "record",
            json.dumps(frame),
            *(["--accept"] if accept else []),
            **kw,
        )

    barriers = []
    for phase, expected in [("before", 0), ("after", 1)]:
        frame = {**base, "source": phase, "native_id": "native-" + phase}
        prior = counts()
        kill_writer(
            ctx,
            owner.data,
            "ingress_crash_child",
            phase,
            {"YUKI_INGRESS_TEST_FRAME": json.dumps(frame)},
        )
        after = counts()
        ctx.equal(
            phase + " persisted atomic event and admission",
            after,
            [prior[0] + expected, prior[1] + expected, prior[2]],
        )
        with owner.running():
            result = record(frame)
            replay = record({**frame, "observed": 32})
            ctx.equal("redelivery returns original event", result, replay)
            ctx.equal(
                "one event and one accepted source",
                counts(),
                [prior[0] + 1, prior[1] + 1, prior[2]],
            )
            ctx.check(
                "accepted source discoverable after restart",
                result["event"] in op("ingress", "pending")["pending"],
            )
            claim = op("ingress", "claim", str(result["event"]))
            ctx.equal(
                "execution is unique per accepted source",
                claim,
                op("ingress", "claim", str(result["event"])),
            )
            ctx.equal(
                "normalized segments survive restart",
                claim["envelope"]["segments"],
                frame["segments"],
            )
            ctx.equal(
                "typed mention retains position, original and display",
                [(m["position"], m["original"], m["label"]) for m in claim["mentions"]],
                [(1, actor, "X")],
            )
            ctx.check(
                "claimed execution leaves pending queue",
                result["event"] not in op("ingress", "pending")["pending"],
            )
            barriers.append(
                {
                    "phase": phase,
                    "prior": prior,
                    "after_kill": after,
                    "ack": result,
                    "claim": claim,
                }
            )
    observe = {**base, "source": "later", "native_id": "later"}
    with owner.running():
        observed = record(observe, False)
        ctx.equal("observed only", observed["accepted"], False)
    with owner.running():
        op("ingress", "claim", str(observed["event"]), accepted=False)
        changed = {**observe, "segments": [{"type": "text", "text": "replaced"}]}
        record(changed, accepted=False)
        promoted = record(observe)
        ctx.equal(
            "delayed promotion retains ID",
            promoted,
            {"event": observed["event"], "accepted": True},
        )
        for n, kind in enumerate(("edit", "recall", "data", "echo", "message")):
            f = {
                **base,
                "source": "only-" + kind,
                "native_id": "only-" + kind,
                "kind": kind,
                "segments": [] if n == 4 else base["segments"],
            }
            ctx.equal("never ordinary admission: " + kind, record(f)["accepted"], False)
        before = counts()
        group = {
            **base,
            "source": "shared-lexical-source",
            "native_id": "same",
            "space": "space",
            "topic": "one",
        }
        one = record(group)
        two = record({**group, "topic": "two"})
        ctx.check("topic is part of source scope", one["event"] != two["event"])
        ctx.equal(
            "two new sources", counts(), [before[0] + 2, before[1] + 2, before[2]]
        )
        quoted = record(
            {
                **base,
                "source": "quoted",
                "segments": [{"type": "reply", "native_id": "prior-message"}],
            }
        )
        ctx.check(
            "real quoted reference is distinct from an empty envelope",
            quoted["accepted"],
        )
        op("person", "set-enabled", actor, "false")
        op("ingress", "claim", str(one["event"]), accepted=False)
        ctx.equal("revoked actor cannot consume saved material", counts()[2], before[2])
        new_owner = "00000000-0000-4000-8000-000000000009"
        op("person", "create", new_owner)
        op(
            "account",
            "transfer",
            person["assignment"],
            "40",
            "40",
            "--person",
            new_owner,
        )
        new = record({**observe, "occurred": 45, "observed": 46})
        ctx.check(
            "reused native key on a new DM stage cannot alias old event",
            new["accepted"] and new["event"] != observed["event"],
        )
        unknown = record({**observe, "occurred": None, "observed": 47})
        ctx.check(
            "ambiguous raw account creates no ordinary source", not unknown["accepted"]
        )
    return {"barriers": barriers, "promoted": promoted, "counts": counts()}
