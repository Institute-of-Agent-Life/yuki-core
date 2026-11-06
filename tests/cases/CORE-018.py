"""Exercise due admission, owner-scoped management, delivery and cold recovery through Main."""

import json
import os
import sqlite3
import time
from contextlib import closing
from tests.harness.service import Service, previous_schema
from tests.harness.crash import kill_writer
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer


def answer(q, text="ok", calls=()):
    m = {"role": "assistant", "content": text if not calls else None}
    if calls:
        m["tool_calls"] = [
            {
                "id": key,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": json.dumps(args, ensure_ascii=False),
                },
            }
            for key, name, args in calls
        ]
    q.put(
        (
            {
                "choices": [
                    {"message": m, "finish_reason": "tool_calls" if calls else "stop"}
                ]
            },
            0,
        )
    )


def result(body, key):
    return json.loads(
        next(m["content"] for m in body["messages"] if m.get("tool_call_id") == key)
    )


class Scene:
    def __init__(self, ctx):
        self.ctx = ctx
        self.svc = Service(ctx, ctx.working / "scheduler")
        previous_schema(ctx, self.svc.data, 17)
        (self.svc.data / "SOUL.md").write_text("Yuki", encoding="utf-8")
        ctx.check(
            "additive migration over previous schema",
            self.svc.command("migrate")["schema_version"] >= 18,
        )
        self.a, self.b, self.c = [
            f"00000000-0000-4000-8000-{n:012x}" for n in (18, 118, 218)
        ]
        self.seq = 0
        with self.svc.running():
            ctx.write("sqlite-build.json", self.op("database-info"))
            self.handles = {}
            for actor, native in ((self.a, "200"), (self.b, "201"), (self.c, "202")):
                self.op("person", "create", actor)
                self.handles[actor] = self.op(
                    "account",
                    "open",
                    "qq",
                    "schedule",
                    native,
                    1,
                    "--person",
                    actor,
                    "--start",
                    1,
                )
            self.op("person", "set-admin", self.b, "true")
            self.bot = self.op(
                "account", "open", "qq", "schedule", "100", 1, "--start", 1
            )

    def op(self, *args):
        return self.svc.command("operator", *map(str, args))

    def rows(self, query, *args):
        with closing(
            sqlite3.connect(
                (self.svc.data / "core.sqlite").as_uri() + "?mode=ro", uri=True
            )
        ) as db:
            return db.execute(query, args).fetchall()

    def until(self, fn):
        end = time.monotonic() + 18
        while time.monotonic() < end:
            r = fn()
            if r:
                return r
            time.sleep(0.04)
        self.diagnostic()
        raise AssertionError("original scheduled state not observed")

    def diagnostic(self):
        self.ctx.write(
            "schedule-diagnostic.json",
            {
                "definitions": self.rows(
                    "SELECT id,status,next_due,version,run_count FROM schedules"
                ),
                "occurrences": self.rows("SELECT * FROM schedule_occurrences"),
                "turns": self.rows(
                    "SELECT x.state,x.source,x.origin,t.diagnostic FROM executions x JOIN main_turns t ON t.execution=x.id"
                ),
            },
        )

    def wire(self, http):
        try:
            raw, q = http.receive()
        except Exception:
            self.diagnostic()
            raise
        return json.loads(raw), q

    def human(self, text, actor=None, group=None):
        self.seq += 1
        frame = {
            "source": f"schedule-{self.seq}",
            "account": self.handles[actor or self.a]["account"],
            "bot_stage": self.bot["assignment"],
            "reply_bot": "100",
            "occurred": 100 * self.seq,
            "observed": 100 * self.seq,
            "kind": "message",
            "segments": [{"type": "text", "text": text}],
        }
        if group:
            frame["space"] = group
        return self.op("ingress", "record", json.dumps(frame), "--accept")["event"]

    def finished(self, event):
        return self.until(
            lambda: self.rows(
                "SELECT id FROM executions WHERE source=? AND state='completed'", event
            )
        )

    def tool(self, http, name, args, actor=None, ok=True):
        event = self.human(name, actor)
        _, q = self.wire(http)
        key = f"call-{event}"
        answer(q, calls=[(key, name, args)])
        next_body, q = self.wire(http)
        receipt = result(next_body, key)
        self.ctx.equal(
            "original " + name + " receipt",
            receipt["state"],
            "confirmed" if ok else "failed",
        )
        answer(q)
        self.finished(event)
        return receipt["result"]

    def confirmed_send(self, wire, call_id, locator, target_kind):
        receipt = result(wire, call_id)
        data = receipt["data"]
        self.ctx.equal("scheduled send reports its committed status on " + call_id,
                       (receipt["ok"], receipt["mutation_committed"], receipt["retryable"],
                        data["action"], data["status"], data["target"]["kind"]),
                       (True, True, False, "send_message", "succeeded", target_kind))
        anchored = self.rows(
            "SELECT i.operation_id,d.event,a.entry,a.stage FROM model_tool_intents i "
            "JOIN social_parts s ON s.namespace=i.execution AND s.operation_id=i.operation_id "
            "JOIN social_outbound a ON a.namespace=s.namespace AND a.child=s.child "
            "JOIN personal_deliveries d ON d.outbound=a.id WHERE i.call_id=? AND a.locator=?",
            call_id, str(locator),
        )
        self.ctx.equal("scheduled receipt is paired to its original QQ ACK on " + call_id,
                       anchored, [(data["operation_id"], data["original_event_id"],
                                   data["original_conversation_id"], data["presence_id"])])

    def deliver(self, peer, action, locator):
        while True:
            call = peer.receive()
            if call["action"] == action:
                break
            peer.send(
                {"echo": call.get("echo"), "status": "ok", "retcode": 0, "data": {}}
            )
        peer.send(
            {
                "echo": call["echo"],
                "status": "ok",
                "retcode": 0,
                "data": {"message_id": locator},
            }
        )
        return call["params"]


def run(ctx):
    s = Scene(ctx)
    rule = {"type": "interval", "seconds": 3600}
    goal = "original birthday reminder"
    env = {**os.environ, "YUKI_SCHEDULE_PEER": "offline-schedule-fixture"}
    gold = json.loads((ctx.root / "fixtures/schedule/v4-clock-tools.json").read_bytes())
    with Recorder() as http:
        with (s.svc.data / "yuki.toml").open("a", encoding="utf-8") as f:
            f.write(
                f'\n[main]\nendpoint="{http.url}"\nmodel="schedule-fixture"\nname="Yuki"\nmerge_ms=1\nroot_slots=12\nmax_input_units=1600000\nmax_output=512\n[qq]\nnamespace="schedule"\ndesignated_ingress="100"\n[qq.groups]\n"700"="paused"\n[[qq.bots]]\nself_id="100"\nstage="{s.bot["assignment"]}"\ntoken_env="YUKI_SCHEDULE_PEER"\n'
            )
        with s.svc.running(environment=env):
            created = s.tool(
                http,
                "automation_manage",
                {
                    "action": "create",
                    "task": {"trigger": rule, "goal": goal, "name": "Public name"},
                    "max_runs": 3,
                },
            )
            aid = created["automation_id"]
            ctx.equal(
                "create only persists the schedule",
                (
                    created["confirmation"],
                    created["run_count"],
                    s.rows("SELECT count(*) FROM schedule_occurrences"),
                ),
                ("persisted", 0, [(0,)]),
            )
            before = created["next_due_utc_ms"]
            kept = s.tool(
                http,
                "automation_manage",
                {
                    "action": "update",
                    "automation_id": aid,
                    "task": {"trigger": rule, "goal": goal},
                },
            )
            ctx.equal(
                "omission retains max_runs, display label, run_count and relative due",
                (
                    kept["next_due_utc_ms"],
                    kept["max_runs"],
                    kept["name"],
                    kept["run_count"],
                ),
                (before, 3, "Public name", 0),
            )
            s.tool(
                http,
                "automation_query",
                {"view": "get", "automation_id": aid},
                s.c,
                ok=False,
            )
            safe = s.tool(http, "automation_query", {"view": "list"}, s.c)
            ctx.check(
                "safe directory excludes private goal",
                str(aid) in str(safe) and goal not in str(safe),
            )
            # Queue an actual near-future change. Stop the ordinary owner before it is due.
            short = {"type": "interval", "seconds": 8}
            v = s.tool(
                http,
                "automation_manage",
                {
                    "action": "update",
                    "automation_id": aid,
                    "task": {"trigger": short, "goal": goal},
                },
            )
            due = v["next_due_utc_ms"]
        # Kill the real owner between committed unique admission and first model dispatch.
        kill_writer(ctx, s.svc.data, "schedule_crash_child", "schedule-admitted", env)
        frozen = s.rows(
            "SELECT o.id,o.execution,o.planned_utc,v.goal,x.source_occurrence FROM schedule_occurrences o JOIN executions x ON x.id=o.execution JOIN schedule_versions v ON v.schedule=o.schedule AND v.version=o.version"
        )
        ctx.check(
            "kill retains exactly one original execution and immutable version before purchase",
            len(frozen) == 1
            and frozen[0][0] == frozen[0][4]
            and frozen[0][2:4] == (due, goal)
            and not s.rows(
                "SELECT r.id FROM model_requests r JOIN activations a ON a.id=r.activation WHERE a.execution=?",
                frozen[0][1],
            ),
        )
        with (
            s.svc.running(environment=env),
            closing(Peer(s.svc.address, "100", env["YUKI_SCHEDULE_PEER"])) as peer,
        ):
            first, q_due = s.wire(http)
            ctx.check(
                "first due Main uses original goal without borrowing an active Work",
                goal in str(first["messages"])
                and "no_active_work" in str(first["messages"]),
            )
            revised = s.tool(
                http,
                "automation_manage",
                {
                    "action": "update",
                    "automation_id": aid,
                    "task": {
                        "trigger": short,
                        "goal": "revised private goal",
                        "name": None,
                    },
                    "max_runs": None,
                },
                s.b,
            )
            ctx.equal(
                "admin edit leaves original owner, accepted count and planned instant",
                (
                    revised["owner"],
                    revised["run_count"],
                    revised["next_due_utc_ms"],
                    revised["max_runs"],
                    revised["name"],
                ),
                (s.a, 0, due, None, None),
            )
            # An accepted neutral occurrence can deliver over the real QQ socket.
            answer(
                q_due,
                calls=[("wake-send", "send", {"text": "scheduled original delivery"})],
            )
            params = s.deliver(peer, "send_private_msg", 1801)
            ctx.equal(
                "real QQ receiver is the original private account",
                str(params["user_id"]),
                "200",
            )
            paired, q = s.wire(http)
            s.confirmed_send(paired, "wake-send", 1801, "person")
            answer(q)
            s.until(
                lambda: s.rows(
                    "SELECT id FROM schedule_occurrences WHERE status='succeeded'"
                )
            )
            runs = s.tool(
                http, "automation_query", {"view": "history", "automation_id": aid}
            )
            ctx.equal(
                "history keeps original, even after edit at dispatch",
                runs["runs"][0]["definition"],
                goal,
            )
            # Test pause, resume, later private Work, and late settlement separately.
            s.tool(http, "automation_manage", {"action": "pause", "automation_id": aid})
            paused = s.rows("SELECT status,next_due FROM schedules WHERE id=?", aid)
            ctx.check(
                "pause preserves the already determined due time",
                paused[0][0] == "paused" and paused[0][1] > due,
            )
            # Resume preserves the original next planned time. The later occurrence runs once.
            ev = s.human("resume from original administrator", s.b)
            _, q = s.wire(http)
            answer(
                q,
                calls=[
                    (
                        "resume",
                        "automation_manage",
                        {"action": "resume", "automation_id": aid},
                    )
                ],
            )
            # Admin and original creator have independent Main lanes. Either may arrive first.
            wires = [s.wire(http), s.wire(http)]
            admin = next(
                (b, q)
                for b, q in wires
                if any(m.get("tool_call_id") == "resume" for m in b["messages"])
            )
            scheduled = next((b, q) for b, q in wires if (b, q) != admin)
            answer(admin[1])
            s.finished(ev)
            ctx.check(
                "second distinct occurrence adopts revised private goal",
                "revised private goal" in str(scheduled[0]["messages"]),
            )
            answer(
                scheduled[1],
                calls=[
                    (
                        "begin-work",
                        "task_control",
                        {"action": "accept", "goal": "actual accepted Work goal"},
                    )
                ],
            )
            bound, q_work = s.wire(http)
            work = result(bound, "begin-work")["result"]["work_id"]
            ctx.check(
                "accepted Work owns its new private goal",
                "actual accepted Work goal" in str(bound["messages"]),
            )
            s.tool(
                http,
                "automation_manage",
                {"action": "pause", "automation_id": aid},
                s.b,
            )
            ctx.equal(
                "pause leaves accepted Work running",
                s.rows("SELECT status,goal FROM work_nodes WHERE id=?", work),
                [("active", "actual accepted Work goal")],
            )
            answer(
                q_work,
                calls=[("finish", "complete", {"summary": "original Work done"})],
            )
            s.until(
                lambda: s.rows(
                    "SELECT id FROM schedule_occurrences WHERE work=? AND status='succeeded'",
                    work,
                )
            )
            originals = s.rows(
                "SELECT id,planned_utc,execution,work FROM schedule_occurrences ORDER BY id"
            )
            ctx.check(
                "rescan yields two unique instants and exactly one execution each",
                len(originals) == 2
                and len({r[1] for r in originals}) == 2
                and len({r[2] for r in originals}) == 2,
            )
            ctx.equal(
                "public definition stays paused after original in-flight Work settles",
                s.rows("SELECT status,run_count FROM schedules WHERE id=?", aid),
                [("paused", 2)],
            )
            # SELF receives a registration and an explicit entry, with no human ancestor.
            # Open a concrete group receiver using the actual recovered-group proof.
            peer.send(
                {
                    "self_id": 100,
                    "user_id": 201,
                    "message_type": "group",
                    "post_type": "message",
                    "group_id": 700,
                    "time": 100,
                    "message_id": 4931,
                    "message": "/ai admin group current enable",
                }
            )
            probe = peer.receive()
            ctx.equal(
                "membership probe pins native SELF group",
                probe["action"],
                "get_group_member_info",
            )
            peer.send(
                {
                    "echo": probe["echo"],
                    "status": "ok",
                    "retcode": 0,
                    "data": {"group_id": 700, "user_id": 100},
                }
            )
            control = s.deliver(peer, "send_group_msg", 1802)
            ctx.check(
                "original group control enabled the pinned receiver",
                "enabled" in str(control),
            )
            entry = s.op(
                "entry",
                "group",
                json.dumps(["qq", "schedule", "700"], separators=(",", ":")),
            )["entry"]
            s.op("main", "register", "self", "calendar-self", entry)
            selfish = s.op(
                "main",
                "trigger",
                "calendar-self",
                "create-private",
                json.dumps({"text": "own SELF calendar"}),
            )
            _, q = s.wire(http)
            answer(
                q,
                calls=[
                    (
                        "self-schedule",
                        "automation_manage",
                        {
                            "action": "create",
                            "task": {
                                "goal": "own SELF occurrence",
                                "trigger": {"type": "after", "seconds": 1},
                            },
                        },
                    )
                ],
            )
            rec, q = s.wire(http)
            sid = result(rec, "self-schedule")["result"]["automation_id"]
            answer(q)
            self_wire, q = s.wire(http)
            ctx.check(
                "SELF carries its own original identity and no human ID",
                "calendar-self" in str(self_wire["messages"])
                and all(a not in str(self_wire["messages"]) for a in (s.a, s.b, s.c)),
            )
            answer(
                q,
                calls=[
                    (
                        "self-group-send",
                        "send",
                        {"text": "scheduled SELF group delivery"},
                    )
                ],
            )
            delivered = s.deliver(peer, "send_group_msg", 1803)
            ctx.equal(
                "neutral SELF send uses the pinned original group",
                str(delivered["group_id"]),
                "700",
            )
            paired, q = s.wire(http)
            s.confirmed_send(paired, "self-group-send", 1803, "space")
            answer(q)
            s.until(
                lambda: s.rows(
                    "SELECT id FROM schedule_occurrences WHERE schedule=? AND status='succeeded'",
                    sid,
                )
            )
            ctx.equal(
                "SELF never inherits the conversation's most recent human",
                s.rows(
                    "SELECT s.owner,o.owner,x.original_actor,x.source,p.parent FROM schedules s JOIN schedule_occurrences o ON s.id=o.schedule JOIN executions x ON x.id=o.execution JOIN main_origins p ON p.ref=x.origin WHERE s.id=?",
                    sid,
                ),
                [("calendar-self", "calendar-self", None, None, selfish["execution"])],
            )
            # Transfer before dispatch of an already accepted original DM blocks a later send.
            old = s.tool(
                http,
                "automation_manage",
                {
                    "action": "create",
                    "task": {
                        "goal": "never send to the new holder",
                        "trigger": {"type": "after", "seconds": 1},
                    },
                },
            )
            _, q = s.wire(http)
            s.op(
                "account",
                "transfer",
                s.handles[s.a]["assignment"],
                9000,
                9001,
                "--person",
                s.c,
            )
            answer(q, calls=[("late-send", "send", {"text": "wrong recipient"})])
            s.until(
                lambda: s.rows(
                    "SELECT id FROM schedule_occurrences WHERE schedule=? AND status='blocked'",
                    old["automation_id"],
                )
            )
            ctx.equal(
                "no send operation dispatched after transfer",
                s.rows(
                    "SELECT count(*) FROM social_outbound WHERE namespace=(SELECT execution FROM schedule_occurrences WHERE schedule=?)",
                    old["automation_id"],
                ),
                [(0,)],
            )
            # Keep each original trigger form and the new explicit cron as separate stored definitions.
            rules = [
                {"type": "once", "local_datetime": "2099-05-01T09:00:00+05:30"},
                {"type": "after", "seconds": 3600},
                {"type": "interval", "seconds": 1800},
                {
                    "type": "daily",
                    "hour": 2,
                    "minute": 30,
                    "timezone": "America/New_York",
                },
                {
                    "type": "weekly",
                    "weekdays": [2, 7],
                    "hour": 1,
                    "minute": 30,
                    "timezone": "Australia/Lord_Howe",
                },
                {
                    "type": "cron",
                    "expression": "30 2 * * 2,7",
                    "timezone": "America/New_York",
                },
            ]
            for trigger in rules:
                v = s.tool(
                    http,
                    "automation_manage",
                    {
                        "action": "create",
                        "task": {"goal": "test original rule form", "trigger": trigger},
                    },
                    s.b,
                )
                ctx.equal(
                    "original trigger shape persisted and read intact",
                    v["trigger"],
                    trigger,
                )
            s.tool(
                http,
                "automation_manage",
                {
                    "action": "create",
                    "task": {
                        "goal": "bad once",
                        "trigger": {
                            "type": "once",
                            "local_datetime": "2001-01-01T12:00:00+00:00",
                        },
                    },
                },
                s.b,
                ok=False,
            )
            for bad in ["0 0 30 2 *", "0 12 * * MON#2"]:
                s.tool(
                    http,
                    "automation_manage",
                    {
                        "action": "create",
                        "task": {
                            "goal": "reject unsupported cron",
                            "trigger": {"type": "cron", "expression": bad},
                        },
                    },
                    s.b,
                    ok=False,
                )
            missed = s.tool(
                http,
                "automation_manage",
                {
                    "action": "create",
                    "task": {
                        "goal": "single original overdue occurrence",
                        "trigger": {"type": "interval", "seconds": 3},
                    },
                    "max_runs": 2,
                },
                s.b,
            )
        time.sleep(7)
        with s.svc.running(environment=env):
            due_body, q = s.wire(http)
            ctx.check(
                "restart recovers original missed planned instant",
                "single original overdue occurrence" in str(due_body)
                and str(missed["next_due_utc_ms"]) in str(due_body),
            )
            answer(q)
            missed_id = missed["automation_id"]
            s.until(
                lambda: s.rows(
                    "SELECT id FROM schedule_occurrences WHERE schedule=? AND status='succeeded'",
                    missed_id,
                )
            )
            rows = s.rows(
                "SELECT count(*),min(planned_utc) FROM schedule_occurrences WHERE schedule=?",
                missed_id,
            )
            ctx.equal(
                "multiple missed boundaries collapse into the original first occurrence",
                rows,
                [(1, missed["next_due_utc_ms"])],
            )
            next_due = s.rows(
                "SELECT next_due,run_count FROM schedules WHERE id=?", missed_id
            )[0]
            ctx.check(
                "next slot advances from completion and preserves run count",
                next_due[0] > int(time.time() * 1000) and next_due[1] == 1,
            )
    return {
        "cases": {
            "H08": {
                "originals": originals,
                "cold_kill": frozen,
                "first_delivery": params,
                "self_schedule": sid,
                "self_group_delivery": delivered,
                "transfer_blocked": old["automation_id"],
                "missed_slots": {"original": rows, "advanced": next_due},
            },
            "R14": {
                "original_due": due,
                "counter_after_admin_edit": revised["run_count"],
                "max_runs_after_null": revised["max_runs"],
            },
        },
        "original_clock": gold["clock"],
        "clock_source": gold["source_commit"],
    }
