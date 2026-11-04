"""Use authenticated local operator and real QQ command entry; compare exact source rows across a restart."""

import hashlib
import json
import os
import sqlite3
import time
from contextlib import closing
from tests.harness.service import Service, previous_schema
from tests.peers.onebot.peer import Peer


def run(ctx):
    svc = Service(ctx, ctx.working / "facts")
    previous_schema(ctx, svc.data, 14)
    ctx.check(
        "upgrade Work reader to fact reader",
        svc.command("migrate")["schema_version"] >= 15,
    )

    def op(*args, ok=True):
        return svc.command("operator", *map(str, args), accepted=ok)

    def rows(sql, *args):
        with closing(
            sqlite3.connect((svc.data / "core.sqlite").as_uri() + "?mode=ro", uri=True)
        ) as db:
            return db.execute(sql, args).fetchall()

    a, b, c, legacy, outsider = [
        f"00000000-0000-4000-8000-{i:012x}" for i in range(401, 406)
    ]
    handles = {}
    with svc.running():
        ctx.write("sqlite-build.json", op("database-info"))
        for index, person in enumerate((a, b, c, legacy, outsider)):
            op("person", "create", person)
            if person != legacy:
                handles[person] = op(
                    "account",
                    "open",
                    "qq",
                    "memory",
                    str(200 + index),
                    1,
                    "--person",
                    person,
                    "--start",
                    1,
                )
        bot = op("account", "open", "qq", "memory", "100", 1, "--start", 1)
        op("person", "set-admin", a, "true")
        op("person", "merge", legacy, b)
    (svc.data / "SOUL.md").write_text("Yuki", encoding="utf-8")
    with (svc.data / "yuki.toml").open("a", encoding="utf-8") as f:
        f.write(
            f'\n[qq]\nnamespace="memory"\ndesignated_ingress="100"\n[qq.groups]\n"700"="open"\n"701"="open"\n[[qq.bots]]\nself_id="100"\nstage="{bot["assignment"]}"\ntoken_env="YUKI_R16_FIXTURE"\n'
        )
    env = {**os.environ, "YUKI_R16_FIXTURE": "fixture-local-only"}
    seq = 0

    def record(person, group, text):
        nonlocal seq
        seq += 1
        frame = {
            "source": "fact-" + str(seq),
            "account": handles[person]["account"],
            "bot_stage": bot["assignment"],
            "kind": "message",
            "occurred": 10 + seq,
            "observed": 10 + seq,
            "segments": [{"type": "text", "text": text}],
        }
        if group:
            frame["space"] = group
        event = op("ingress", "record", json.dumps(frame), "--accept")["event"]
        return event, op("ingress", "claim", event)["execution"]

    def read(ex, person, target, *, admin=False, ok=True):
        args = ["memory", "read", ex, person, "--evidence"]
        if target:
            args.extend(["--fact", target])
        if admin:
            args.append("--admin")
        result = op(*args, ok=ok)
        if not ok:
            return result
        return result["items"][0] if target else result["items"]

    def edit(ex, person, key, change, *, admin=False, ok=True):
        args = [
            "memory",
            "edit",
            ex,
            person,
            key,
            json.dumps(change, ensure_ascii=False),
        ]
        if admin:
            args.append("--admin")
        return op(*args, ok=ok)

    def corrected(id, row, text):
        return {
            "action": "correct",
            "fact": id,
            "revision": row["revision"],
            "proof": row["proof"],
            "body": text,
        }

    def settle(cond):
        until = time.monotonic() + 12
        while time.monotonic() < until:
            r = cond()
            if r:
                return r
            time.sleep(0.03)
        raise AssertionError("memory command did not settle")

    notes = []
    with svc.running(environment=env):
        private, ax = record(a, None, "B drinks oolong. Private tea quote 008.")
        alternate, _ = record(a, None, "Second version. Private tea quote 008.")
        _, bx = record(b, None, "inspect my facts")
        _, cx = record(c, None, "inspect directly shared facts")
        _, ox = record(outsider, None, "inspect indirect relations")
        record(a, "700", "A and B share group 700")
        record(b, "700", "B observes group 700")
        record(b, "701", "B and C share group 701")
        record(c, "701", "C observes group 701")
        record(outsider, "702", "outside observer")
        record(a, "702", "A shares group with outside, B does not")
        base = {
            "action": "add",
            "subject": {"kind": "person", "person": legacy},
            "fact_key": "tea",
            "body": "B prefers oolong",
            "confidence": 0.73,
            "evidence": [
                {"kind": "event", "id": private, "quote": "Private tea quote 008."}
            ],
        }
        first = edit(ax, a, "R1", base, admin=True)
        fid = first["fact"]
        second = edit(
            ax, a, "new tea", dict(base, body="B also drinks green tea"), admin=True
        )
        ctx.check("same subject and key preserve two facts", fid != second["fact"])
        own = read(ax, a, fid, admin=True)
        ctx.equal(
            "source author remains A, subject is accountless original B",
            [own["person"], own["evidence"][0]["author"], own["evidence"][0]["key"]],
            [legacy, a, str(private)],
        )
        for ex, p in [(bx, b), (cx, c)]:
            structured = read(ex, p, fid)
            ctx.equal(
                "readable count and detail use the same ACL",
                [
                    structured["readable_evidence"],
                    structured["evidence"],
                    structured["body"],
                ],
                [0, [], "B prefers oolong"],
            )
            ctx.check(
                "hidden quote omitted from serialized result",
                "Private tea quote" not in json.dumps(structured),
            )
        read(ox, outsider, fid, ok=False)
        read(cx, c, fid, admin=True, ok=False)
        edit(
            cx,
            c,
            "no foreign control",
            corrected(fid, read(cx, c, fid), "deny"),
            ok=False,
        )
        row = read(bx, b, fid)
        edit(bx, b, "R2", corrected(fid, row, "B prefers old oolong"))
        ctx.equal(
            "replaying R1 returns its original receipt",
            edit(ax, a, "R1", base, admin=True),
            first,
        )
        row = read(bx, b, fid)
        ctx.equal(
            "R1 cannot undo R2 and omitted confidence retained",
            [row["revision"], row["body"], row["confidence"]],
            [2, "B prefers old oolong", 0.73],
        )
        edit(ax, a, "R1", dict(base, body="different"), admin=True, ok=False)
        edit(
            ax,
            a,
            "missing quote",
            dict(
                base,
                evidence=[{"kind": "event", "id": private, "quote": "fabricated text"}],
            ),
            admin=True,
            ok=False,
        )
        old = read(ax, a, fid, admin=True)
        notes.extend([{"summary": row, "source": own}])
    # Offline mutation of this disposable upgraded database simulates a corrected source row
    # at constant fact revision and evidence count, independent of the Rust mutation counter.
    with closing(sqlite3.connect(svc.data / "core.sqlite")) as db:
        raw = json.loads(
            db.execute(
                "SELECT envelope FROM events WHERE id=?", (alternate,)
            ).fetchone()[0]
        )
        sha = hashlib.sha256(
            json.dumps(
                raw, ensure_ascii=False, sort_keys=True, separators=(",", ":")
            ).encode()
        ).hexdigest()
        db.execute(
            "UPDATE memory_evidence SET id=id+800,source_key=?,event=?,source_version=? WHERE fact=?",
            (str(alternate), alternate, sha, fid),
        )
        db.commit()
    with svc.running(environment=env):
        fresh = read(ax, a, fid, admin=True)
        ctx.equal(
            "same count, different physical evidence and proof",
            [
                fresh["revision"],
                fresh["readable_evidence"],
                fresh["proof"] != old["proof"],
            ],
            [old["revision"], 1, True],
        )
        edit(ax, a, "stale", corrected(fid, old, "not allowed"), admin=True, ok=False)
        edit(
            ax, a, "R3", corrected(fid, fresh, "accepted after re-reading"), admin=True
        )
        group_entry = rows(
            "SELECT id FROM entries WHERE scope='group' AND space='700'"
        )[0][0]
        _, in_group = record(a, "700", "group visibility request")
        subjects = [
            ("group", {"kind": "group", "space": "700"}),
            ("person_group", {"kind": "person_group", "space": "700", "person": b}),
            ("global", {"kind": "self", "visibility": "global"}),
            (
                "group-self",
                {"kind": "self", "visibility": "group", "entry": group_entry},
            ),
        ]
        for label, subject in subjects:
            source_exec = in_group if label == "group-self" else ax
            received = edit(
                source_exec,
                a,
                "scope-" + label,
                dict(base, subject=subject),
                admin=True,
            )["fact"]
            visible = read(bx, b, received, ok=label != "group-self")
            if label in ("group", "person_group", "group-self"):
                read(cx, c, received, ok=False)
            notes.append({"scope": label, "id": received})
        row = read(bx, b, fid)
        withdrawn = edit(
            bx,
            b,
            "remove",
            {
                "action": "retract",
                "fact": fid,
                "revision": row["revision"],
                "proof": row["proof"],
            },
        )
        ctx.equal("owner retracts original ID", withdrawn["outcome"], "retracted")
        ctx.check(
            "retracted fact leaves active list",
            all(item["fact"] != fid for item in read(bx, b, None)),
        )
        # A real reverse QQ session receives the core command's domain response.
        with closing(Peer(svc.address, "100", env["YUKI_R16_FIXTURE"])) as peer:

            def cmd(mid, text):
                peer.send(
                    {
                        "post_type": "message",
                        "message_type": "private",
                        "self_id": 100,
                        "user_id": 201,
                        "message_id": mid,
                        "time": 100,
                        "sender": {"nickname": "B"},
                        "message": [{"type": "text", "data": {"text": text}}],
                    }
                )
                for _ in range(12):
                    wire = peer.receive()
                    if wire["action"] == "send_private_msg":
                        peer.send(
                            {
                                "echo": wire["echo"],
                                "status": "ok",
                                "retcode": 0,
                                "data": {"message_id": mid + 1000},
                            }
                        )
                        return str(wire["params"]["message"])
                    peer.send(
                        {
                            "echo": wire.get("echo"),
                            "status": "ok",
                            "retcode": 0,
                            "data": {},
                        }
                    )
                raise AssertionError("missing QQ memory reply")

            text = cmd(810, "/ai memory add person-controlled entry")
            ctx.check(
                "actual private command returns immutable fact receipt",
                "revision" in text,
            )
            text = cmd(811, "/ai memory list")
            ctx.check(
                "private command lists structured facts",
                "person-controlled entry" in text and "Private tea quote" not in text,
            )
            ctx.equal(
                "no Main purchase for memory command",
                rows("SELECT count(*) FROM model_requests"),
                [(0,)],
            )
            settle(
                lambda: rows(
                    "SELECT count(*) FROM memory_command_events WHERE reply_state='confirmed'"
                )[0][0]
                >= 2
            )
        notes.append(
            {
                "command_receipts": rows(
                    "SELECT event,reply_state FROM memory_command_events ORDER BY event"
                )
            }
        )
    return {
        "cases": {"R16": notes},
        "paid_tools": paid_memory(ctx),
        "replay": first,
        "withdrawal": withdrawn,
        "evidence_swap": {
            "original_version": old["evidence"][0]["version"],
            "replacement_version": fresh["evidence"][0]["version"],
            "old_proof_rejected": True,
        },
    }


def paid_memory(ctx):
    from tests.peers.chat.recorder import Recorder

    svc = Service(ctx, ctx.working / "paid")
    svc.command("init")
    person = "00000000-0000-4000-8000-000000000426"

    def op(*args, ok=True):
        return svc.command("operator", *map(str, args), accepted=ok)

    def rows(sql, *args):
        with closing(
            sqlite3.connect((svc.data / "core.sqlite").as_uri() + "?mode=ro", uri=True)
        ) as db:
            return db.execute(sql, args).fetchall()

    with svc.running():
        op("person", "create", person)
        user = op(
            "account", "open", "qq", "paid", "215", 1, "--person", person, "--start", 1
        )
        bot = op("account", "open", "qq", "paid", "115", 1, "--start", 1)
    with Recorder() as http:
        with (svc.data / "yuki.toml").open("a", encoding="utf-8") as f:
            f.write(
                f'\n[main]\nendpoint="{http.url}"\nmodel="fixture"\nname="Yuki"\nmerge_ms=1\nroot_slots=8\nmax_input_units=1000000\nmax_output=128\n'
            )

        def receive():
            raw, q = http.receive()
            return json.loads(raw), q

        def answer(q, id=None, name=None, params=None):
            msg = {"role": "assistant", "content": "private final marker"}
            if name:
                msg = {
                    "role": "assistant",
                    "content": None,
                    "tool_calls": [
                        {
                            "id": id,
                            "type": "function",
                            "function": {"name": name, "arguments": json.dumps(params)},
                        }
                    ],
                }
            q.put(
                (
                    {
                        "choices": [
                            {
                                "message": msg,
                                "finish_reason": "tool_calls" if name else "stop",
                            }
                        ]
                    },
                    0,
                )
            )

        def paired(wire, id):
            return json.loads(
                next(
                    m["content"]
                    for m in wire["messages"]
                    if m.get("tool_call_id") == id
                )
            )

        with svc.running():
            frame = {
                "source": "actual-memory-event",
                "account": user["account"],
                "bot_stage": bot["assignment"],
                "kind": "message",
                "occurred": 12,
                "observed": 12,
                "segments": [
                    {"type": "text", "text": "please remember: original oolong quote"}
                ],
            }
            ev = op("ingress", "record", json.dumps(frame), "--accept")["event"]
            wire, q = receive()
            names = [t["function"]["name"] for t in wire["tools"]]
            ctx.check(
                "paid original names declared",
                "memory_change" in names and "read_memory" in names,
            )
            answer(
                q,
                "new",
                "memory_change",
                {
                    "operation": "create",
                    "memory_key": "tea",
                    "new_content": "participant drinks oolong",
                    "evidence_quote": "original oolong quote",
                    "confidence": 0.85,
                },
            )
            wire, q = receive()
            receipt = paired(wire, "new")
            ctx.equal(
                "paid create returns actual receipt", receipt["state"], "confirmed"
            )
            n = receipt["result"]["fact_id"]
            fid = receipt["result"]["fact"]
            answer(q, "read", "read_memory", {"view": "fact", "fact_id": n})
            wire, q = receive()
            ctx.equal(
                "paid read binds original numeric ID and revision",
                paired(wire, "read")["result"]["revision"],
                1,
            )
            answer(
                q,
                "revise",
                "memory_change",
                {
                    "operation": "correct",
                    "fact_id": n,
                    "new_content": "corrected in original chain",
                    "evidence_quote": "original oolong quote",
                },
            )
            wire, q = receive()
            ctx.equal(
                "paid write consumes the original read revision",
                paired(wire, "revise")["result"]["revision"],
                2,
            )
            answer(
                q, "stale", "memory_change", {"operation": "invalidate", "fact_id": n}
            )
            wire, q = receive()
            ctx.equal(
                "stale paid view does not authorize later write",
                paired(wire, "stale")["state"],
                "failed",
            )
            answer(q)
            stop = time.monotonic() + 10
            while time.monotonic() < stop and not rows(
                "SELECT id FROM executions WHERE state='completed'"
            ):
                time.sleep(0.03)
            execution = rows("SELECT id FROM executions WHERE source=?", ev)[0][0]
            tool = rows(
                "SELECT operation_id FROM model_tool_intents WHERE call_id='revise'"
            )[0][0]
            extra = {
                "action": "add",
                "subject": {"kind": "person", "person": person},
                "fact_key": "physical",
                "body": "two independent trace sources",
                "evidence": [
                    {
                        "kind": "tool",
                        "execution": execution,
                        "operation": tool,
                        "quote": "active",
                    },
                    {
                        "kind": "run",
                        "execution": execution,
                        "quote": "private final marker",
                    },
                ],
            }
            trace = op(
                "memory", "edit", execution, person, "two-source", json.dumps(extra)
            )
            sources = op(
                "memory",
                "read",
                execution,
                person,
                "--fact",
                trace["fact"],
                "--evidence",
            )["items"][0]["evidence"]
            ctx.equal(
                "real tool and final are bound to separate source types",
                [v["kind"] for v in sources],
                ["tool", "run"],
            )
            op("privacy", "erase", "forget-facts", person, json.dumps([ev]))
            ctx.equal(
                "erasure scrubs fact revisions, original quotes and derived runs",
                rows(
                    "SELECT (SELECT count(*) FROM memory_versions WHERE body IS NOT NULL),(SELECT count(*) FROM memory_evidence WHERE quote IS NOT NULL),(SELECT count(*) FROM memory_facts WHERE status='erased')"
                ),
                [(0, 0, 2)],
            )
            op(
                "memory",
                "edit",
                execution,
                person,
                "two-source",
                json.dumps(extra),
                ok=False,
            )
            ctx.check(
                "erased paid receipt cannot restore the fact",
                rows("SELECT count(*) FROM memory_versions WHERE body IS NOT NULL")
                == [(0,)],
            )
            return {
                "fact_id": n,
                "original_fact": fid,
                "trace_sources": [i["kind"] for i in sources],
                "erased_source": ev,
            }
