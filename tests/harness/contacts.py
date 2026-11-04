import json, sqlite3, time, uuid
from contextlib import closing
from tests.harness.service import Service, previous_schema
from tests.peers.chat.recorder import Recorder


def run(ctx):
    legacy = Service(ctx, ctx.working / "legacy-spaces")
    previous_schema(ctx, legacy.data, 10)
    canonical = str(uuid.uuid4())
    with closing(sqlite3.connect(legacy.data / "core.sqlite")) as db:
        for raw in (canonical, canonical.upper()):
            db.execute(
                "INSERT INTO entries(id,scope,space) VALUES(?,'group',?)",
                (str(uuid.uuid4()), raw),
            )
        db.commit()
    legacy.command("migrate")
    with closing(sqlite3.connect(legacy.data / "core.sqlite")) as db:
        ctx.equal(
            "upgrade preserves an original canonical Space UUID through spelling aliases",
            db.execute("SELECT id FROM contact_spaces").fetchall(),
            [(canonical,)],
        )
    svc = Service(ctx, ctx.working / "real-contacts")
    svc.command("init")

    def op(*args):
        return svc.command("operator", *map(str, args))

    def rows(sql, *vals):
        with closing(
            sqlite3.connect((svc.data / "core.sqlite").as_uri() + "?mode=ro", uri=True)
        ) as db:
            return db.execute(sql, vals).fetchall()

    a, b, c = (str(uuid.uuid4()) for _ in range(3))
    with svc.running():
        for person, name in [(a, "same"), (b, "same"), (c, "private stranger")]:
            op("person", "create", person, "--name", name)
        aa = op(
            "account", "open", "qq", "contact", "200", 1, "--person", a, "--start", 1
        )
        bb = op(
            "account", "open", "qq", "contact", "201", 1, "--person", b, "--start", 1
        )
        cc = op(
            "account", "open", "qq", "contact", "202", 1, "--person", c, "--start", 1
        )
        alternate = op(
            "account", "open", "test", "contact", "301", 1, "--person", a, "--start", 1
        )
        bot = op("account", "open", "qq", "contact", "100", 1, "--start", 1)

        def record(account, key, space=None, text="observation", accept=False):
            f = {
                "source": key,
                "account": account["account"],
                "bot_stage": bot["assignment"],
                "kind": "message",
                "occurred": 10,
                "observed": 10,
                "segments": [{"type": "text", "text": text}],
            }
            if space:
                f["space"] = json.dumps(["qq", "contact", space])
            return op(
                "ingress", "record", json.dumps(f), *(["--accept"] if accept else [])
            )["event"]

        record(aa, "a-group", "700")
        record(bb, "b-group", "700")
        record(cc, "c-private")
        record(bb, "b-private")
    with Recorder() as model:
        with (svc.data / "yuki.toml").open("a") as f:
            f.write(
                f'\n[main]\nendpoint="{model.url}"\nbinding_endpoint="http://127.0.0.1:{model.server.server_port}/binding"\nmodel="contacts"\nname="Yuki"\nmerge_ms=1\nroot_slots=12\nmax_input_units=1000000\nmax_output=128\n'
            )
        with svc.running():
            event = record(aa, "current", text="current contacts", accept=True)
            raw, q = model.receive()
            wire = json.loads(raw)
            original = json.loads(
                (ctx.root / "fixtures/contacts/v4-declarations.json").read_bytes()
            )
            declared = {t["function"]["name"]: t["function"] for t in wire["tools"]}
            for tool in original["tools"]:
                ctx.equal(
                    "normal Main retains fixed " + tool["name"] + " schema",
                    declared[tool["name"]]["parameters"],
                    tool["parameters"],
                )

            def invoke(name, args):
                nonlocal q
                key = str(uuid.uuid4())
                q.put(
                    (
                        {
                            "choices": [
                                {
                                    "message": {
                                        "role": "assistant",
                                        "content": None,
                                        "tool_calls": [
                                            {
                                                "id": key,
                                                "type": "function",
                                                "function": {
                                                    "name": name,
                                                    "arguments": json.dumps(args),
                                                },
                                            }
                                        ],
                                    },
                                    "finish_reason": "tool_calls",
                                }
                            ]
                        },
                        0,
                    )
                )
                wire, q = model.receive()
                parsed = json.loads(wire)
                receipt = json.loads(
                    next(
                        m["content"]
                        for m in parsed["messages"]
                        if m.get("tool_call_id") == key
                    )
                )
                ctx.equal(
                    "normal paid " + name + " completed", receipt["state"], "confirmed"
                )
                return receipt["result"]

            actor = invoke(
                "find_contacts", {"kind": "person", "subject_ref": "current_speaker"}
            )
            ctx.equal(
                "current speaker survives original subject selection",
                actor["target_id"],
                a,
            )
            ctx.equal(
                "two transport bindings belong to one person",
                sorted(x["binding_id"] for x in actor["bindings"]),
                sorted([aa["assignment"], alternate["assignment"]]),
            )
            upper = invoke("find_contacts", {"kind": "person", "target_id": a.upper()})
            ctx.equal(
                "original UUID parsing is case-insensitive", upper["target_id"], a
            )
            first = invoke(
                "find_contacts", {"kind": "person", "display_name": "same", "limit": 1}
            )
            second = invoke(
                "find_contacts",
                {
                    "kind": "person",
                    "display_name": "same",
                    "limit": 1,
                    "cursor": first["next_cursor"],
                },
            )
            ctx.equal(
                "keyset pages canonical people instead of duplicate accounts",
                [first["items"][0]["target_id"], second["items"][0]["target_id"]],
                sorted([a, b]),
            )
            ctx.check(
                "duplicate display names remain explicit candidates",
                first["has_more"]
                and not second["has_more"]
                and second["next_cursor"] is None,
            )
            mismatch = invoke(
                "find_contacts", {"kind": "person", "source": {"scope": "group"}}
            )
            ctx.equal(
                "scope is an intersection with person/private kind",
                mismatch["items"],
                [],
            )
            private = invoke(
                "find_conversations", {"source": {"scope": "private", "platform": "qq"}}
            )
            ctx.check(
                "private conversation discovery does not enumerate a peer's history",
                private["items"] and all(x["person_id"] == a for x in private["items"]),
            )
            group = invoke(
                "find_contacts",
                {"kind": "space", "source": {"platform": "qq", "scope": "group"}},
            )
            ctx.equal(
                "one canonical group entry has a real observed binding",
                len(group["items"]),
                1,
            )
            group_id = group["items"][0]["target_id"]
            ctx.check(
                "original contact IDs are UUIDs independent of transport group strings",
                str(uuid.UUID(group_id)) == group_id,
            )
            history = invoke(
                "find_conversations",
                {
                    "target": {"space_id": group_id},
                    "source": {"scope": "group", "platform": "qq"},
                },
            )
            ctx.check(
                "contact and historical conversation identify the same canonical group",
                history["items"]
                and all(
                    x["space_id"] == group_id
                    and x["person_id"] is None
                    and x["platform"] == "qq"
                    for x in history["items"]
                ),
            )
            unseen = invoke("find_contacts", {"kind": "person", "target_id": c})
            ctx.equal(
                "unknown private subject remains outside original metadata scope",
                unseen["items"],
                [],
            )
            root = rows("SELECT id FROM executions WHERE source=?", event)[0][0]
            ctx.check(
                "directory publication retains actual scoped Person witnesses",
                bool(
                    rows(
                        "SELECT e.id FROM main_dependencies d JOIN events e ON e.id=d.event WHERE d.execution=? AND e.raw_actor=?",
                        root,
                        b,
                    )
                ),
            )
            q.put(
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
            end = time.monotonic() + 10
            while time.monotonic() < end:
                if rows(
                    "SELECT id FROM executions WHERE source=? AND state='completed'",
                    event,
                ):
                    break
                time.sleep(0.02)
            ctx.check(
                "actual paid directory turn terminates",
                bool(
                    rows(
                        "SELECT id FROM executions WHERE source=? AND state='completed'",
                        event,
                    )
                ),
            )
    return {
        "current_speaker": actor,
        "paged_people": [first, second],
        "private_conversations": private,
        "space_contacts": group,
        "space_conversations": history,
        "source_commit": original["source_commit"],
    }
