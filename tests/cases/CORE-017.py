"""Drive the original Main work runner, real QQ recipient and a restart at partial ALL."""

import json
import os
import sqlite3
import sys
import time
from contextlib import closing
from tests.harness.service import Service, previous_schema
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer


def call(id, name, args):
    return {
        "id": id,
        "type": "function",
        "function": {"name": name, "arguments": json.dumps(args)},
    }


def task(id, action, **kwargs):
    return call(id, "task_control", {"action": action, **kwargs})


def answer(q, *calls, text=""):
    m = {"role": "assistant", "content": None if calls else text}
    if calls:
        m["tool_calls"] = list(calls)
    q.put(
        (
            {
                "choices": [
                    {"finish_reason": "tool_calls" if calls else "stop", "message": m}
                ]
            },
            0,
        )
    )


def pair(body, id):
    return json.loads(
        next(
            m["content"]
            for m in reversed(body["messages"])
            if m.get("tool_call_id") == id
        )
    )


def run(ctx):
    svc = Service(ctx, ctx.working / "waits")
    previous_schema(ctx, svc.data, 15)
    (svc.data / "SOUL.md").write_text("Yuki", encoding="utf-8")
    ctx.check(
        "upgrade the prior reader to waits",
        svc.command("migrate")["schema_version"] >= 16,
    )
    a, b = (f"00000000-0000-4000-8000-{i:012x}" for i in (17, 117))

    def op(*args, ok=True):
        return svc.command("operator", *map(str, args), accepted=ok)

    def rows(sql, *args):
        with closing(
            sqlite3.connect((svc.data / "core.sqlite").as_uri() + "?mode=ro", uri=True)
        ) as db:
            return db.execute(sql, args).fetchall()

    def until(fn):
        end = time.monotonic() + 12
        while time.monotonic() < end:
            v = fn()
            if v:
                return v
            time.sleep(0.04)
        raise AssertionError(
            "timeout: "
            + str(
                rows(
                    "SELECT x.state,t.diagnostic FROM executions x LEFT JOIN main_turns t ON t.execution=x.id"
                )
            )
        )

    with svc.running():
        ctx.write("sqlite-build.json", op("database-info"))
        for actor in (a, b):
            op("person", "create", actor)
        handles = {
            k: op(
                "account", "open", "qq", "wait", str(i), 1, "--person", k, "--start", 1
            )
            for k, i in [(a, 200), (b, 201)]
        }
        bot = op("account", "open", "qq", "wait", "100", 1, "--start", 1)
        app = op(
            "account", "open", "app", "client", "A", 1, "--person", a, "--start", 1
        )
        appbot = op("account", "open", "app", "client", "self", 1, "--start", 1)
        acp = op(
            "account",
            "open",
            "acp",
            "terminal",
            "original",
            1,
            "--person",
            a,
            "--start",
            1,
        )
        acpbot = op("account", "open", "acp", "terminal", "self", 1, "--start", 1)
        op(
            "account",
            "open",
            "telegram",
            "wait.telegram",
            "500",
            1,
            "--person",
            a,
            "--start",
            1,
        )
        telebot = op(
            "account", "open", "telegram", "wait.telegram", "501", 1, "--start", 1
        )
    area = ctx.working / "wait-platform"
    area.mkdir()
    argv = json.dumps(
        [
            str(ctx.root / "tests/peers/host/peer.py"),
            str(area),
            str(ctx.root / "sdk/python"),
        ]
    )
    with (svc.data / "yuki.toml").open("a") as config:
        config.write(
            f'\n[[plugins]]\nid="waitgw"\nnamespace="wait.telegram"\nplatform="telegram"\nbot_stage={json.dumps(telebot["assignment"])}\nprogram={json.dumps(sys.executable)}\nargs={argv}\ngrants=["gateway.inbound"]\n'
        )

    def plugin(rid, source, text, accept=True):
        data = {
            "jsonrpc": "2.0",
            "id": rid,
            "method": "gateway.inbound",
            "params": {
                "source": source,
                "native_id": source,
                "sender": "500",
                "occurred": 20,
                "kind": "message",
                "segments": [{"type": "text", "text": text}],
                "accept": accept,
            },
        }
        tmp = area / f"temp-{rid}"
        tmp.write_text(json.dumps(data), encoding="utf-8")
        tmp.replace(area / f"rpc-{rid}.json")

        def result():
            log = area / "observed.jsonl"
            for line in (
                log.read_text(encoding="utf-8").splitlines() if log.exists() else []
            ):
                try:
                    row = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if row.get("kind") == "reply" and row["frame"]["id"] == rid:
                    return row["frame"]

        return until(result)["result"]

    seq = 0

    def human(
        text, *, person=a, reply=None, platform="qq", source=None, answer_work_id=None
    ):
        nonlocal seq
        seq += 1
        account = (
            handles[person] if platform == "qq" else app if platform == "app" else acp
        )
        stage = bot if platform == "qq" else appbot if platform == "app" else acpbot
        segments = [{"type": "text", "text": text}]
        if reply is not None:
            segments.insert(0, {"type": "reply", "native_id": reply})
        frame = {
            "source": source or ("wait-" + str(seq)),
            "account": account["account"],
            "bot_stage": stage["assignment"],
            "reply_bot": "100" if platform == "qq" else None,
            "occurred": 100 + seq,
            "observed": 100 + seq,
            "kind": "message",
            "segments": segments,
        }
        if answer_work_id is not None:
            frame["answer_work_id"] = answer_work_id
        return op("ingress", "record", json.dumps(frame), "--accept")["event"]

    with Recorder() as http:
        with (svc.data / "yuki.toml").open("a") as f:
            f.write(
                f'\n[main]\nendpoint="{http.url}"\nmodel="fixture-wait"\nname="Yuki"\nmerge_ms=5\nroot_slots=32\nmax_input_units=1400000\nmax_output=128\n[qq]\nnamespace="wait"\ndesignated_ingress="100"\n[[qq.bots]]\nself_id="100"\nstage="{bot["assignment"]}"\ntoken_env="YUKI_WAIT_PEER"\n'
            )
        env = {**os.environ, "YUKI_WAIT_PEER": "offline-fixture"}

        def wire():
            try:
                raw, q = http.receive()
            except Exception:
                ctx.write(
                    "wait-diagnostic.json",
                    {
                        "turns": rows(
                            "SELECT x.id,x.state,x.cancelled,t.diagnostic,t.result FROM executions x LEFT JOIN main_turns t ON t.execution=x.id"
                        ),
                        "waits": rows("SELECT * FROM work_waits"),
                        "terms": rows("SELECT * FROM work_wait_terms"),
                        "model": rows(
                            "SELECT i.call_id,i.paired_receipt FROM model_tool_intents i ORDER BY rowid"
                        ),
                        "mail": rows("SELECT id,work,kind FROM work_mail"),
                        "marks": rows("SELECT * FROM work_request_marks"),
                        "attempts": rows(
                            "SELECT state,diagnostic,response FROM model_attempts"
                        ),
                        "events": rows(
                            "SELECT e.id,e.raw_actor,e.entry_id FROM events e"
                        ),
                    },
                )
                raise
            return json.loads(raw), q

        def new_work(goal):
            event = human(goal)
            _, q = wire()
            answer(q, task("accept-" + str(event), "accept", goal=goal))
            body, q = wire()
            work = pair(body, "accept-" + str(event))["result"]["work_id"]
            exec = rows("SELECT id FROM executions WHERE source=?", event)[0][0]
            return event, work, exec, q

        with svc.running(environment=env):
            event, work, exec, q = new_work("wait for an original input")
            # This event arrives while the original paid request is outside the writer.
            earlier = human("already here before registration")
            answer(
                q,
                task(
                    "any",
                    "wait",
                    wait_mode="any",
                    conditions=[
                        {"kind": "conversation"},
                        {"kind": "time_due", "after_seconds": 3600},
                    ],
                ),
            )
            woke, q = wire()
            ctx.check(
                "pre-settlement input delivered to original Work",
                "already here before registration" in str(woke["messages"]),
            )
            anywait = op("work", "status", exec, a, work)
            ctx.equal(
                "any consumes one hit and retains unmatched condition",
                [
                    anywait["mode"],
                    anywait["state"],
                    [v["hit"] is not None for v in anywait["conditions"]],
                ],
                ["any", "woke", [True, False]],
            )
            ctx.equal(
                "single real input, single wake",
                rows("SELECT count(*) FROM work_wait_routed WHERE event=?", earlier),
                [(1,)],
            )
            current = anywait["wake_execution"]
            answer(
                q,
                task(
                    "all",
                    "wait",
                    wait_mode="all",
                    conditions=[
                        {"kind": "time_due", "after_seconds": 0},
                        {"kind": "conversation"},
                    ],
                    deadline_at="2099-01-01T00:00:00Z",
                ),
            )
            until(
                lambda: rows(
                    "SELECT id FROM work_waits WHERE work=? AND state='armed'", work
                )
            )
            allwait = op("work", "status", exec, a, work)
            ctx.check(
                "partial ALL stays armed",
                allwait["conditions"][0]["hit"] is not None
                and allwait["conditions"][1]["hit"] is None
                and allwait["wake_execution"] is None,
            )
            prior = rows("SELECT count(*) FROM model_requests")[0][0]
            original_op = rows(
                "SELECT operation_id FROM model_tool_intents WHERE execution=? ORDER BY rowid LIMIT 1",
                exec,
            )[0][0]
            for i in range(10):
                op("work", "progress", exec, a, original_op, f"percent {i}")
            ctx.equal(
                "ten hints coalesce with no extra purchases",
                (
                    rows("SELECT latest,updates FROM work_progress"),
                    rows("SELECT count(*) FROM model_requests")[0][0],
                ),
                ([("percent 9", 10)], prior),
            )
            # Replaying the already accepted operation cannot move its absolute deadline.
            original_logical, original_ordinal = rows(
                "SELECT logical,ordinal FROM model_tool_intents WHERE call_id='all'"
            )[0]
            op("model", "pair", original_logical, original_ordinal)
            replay = op("work", "status", exec, a, work)
            ctx.equal(
                "original deadline immutable",
                replay["deadline_ms"],
                allwait["deadline_ms"],
            )
        with svc.running(environment=env):
            unchanged = op("work", "status", exec, a, work)
            ctx.equal(
                "restart preserves partial terms and deadline", unchanged, allwait
            )
            evt = human("finish the second condition")
            woke, q = wire()
            ctx.check(
                "all delivers current matching input",
                "finish the second condition" in str(woke["messages"]),
            )
            all_done = op("work", "status", exec, a, work)
            ctx.equal(
                "all waits for this group",
                len([c for c in all_done["conditions"] if c["hit"]]),
                2,
            )
            answer(q)
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'", work
                )
            )
            event2, work2, exec2, q = new_work(
                "an absolute time is handled after a cold start"
            )
            answer(
                q,
                task(
                    "clock",
                    "wait",
                    conditions=[{"kind": "time_due", "after_seconds": 3}],
                ),
            )
            until(
                lambda: rows(
                    "SELECT id FROM work_waits WHERE work=? AND state='armed'", work2
                )
            )
            clockwait = op("work", "status", exec2, a, work2)
        with (
            svc.running(environment=env),
            closing(Peer(svc.address, "100", env["YUKI_WAIT_PEER"])) as peer,
        ):
            timed, q = wire()
            ctx.check(
                "timer resumes the same Work once", work2 in str(timed["messages"])
            )
            answer(q)
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'", work2
                )
            )
            event3, question, owner, q = new_work("ask me for an original QQ reply")
            answer(
                q,
                call("ask-qq", "send", {"text": "which source should I use?"}),
                task("need", "need_input", reason="choose a source"),
            )
            while True:
                act = peer.receive()
                if act["action"] == "send_private_msg":
                    break
                peer.send(
                    {"echo": act.get("echo"), "status": "ok", "retcode": 0, "data": {}}
                )
            peer.send(
                {
                    "echo": act["echo"],
                    "status": "ok",
                    "retcode": 0,
                    "data": {"message_id": 721},
                }
            )
            until(
                lambda: rows(
                    "SELECT outbound FROM work_user_questions WHERE work=? AND outbound IS NOT NULL",
                    question,
                )
            )
            human("unrelated quoted message", reply="722")
            _, neutral = wire()
            answer(neutral)
            ctx.equal(
                "wrong QQ locator retains question",
                rows("SELECT count(*) FROM work_user_questions WHERE work=?", question),
                [(1,)],
            )
            human("somebody else replies to the right locator", person=b, reply="721")
            _, neutral = wire()
            answer(neutral)
            ctx.equal(
                "different sender cannot claim original question",
                rows("SELECT count(*) FROM work_user_questions WHERE work=?", question),
                [(1,)],
            )
            original_answer = human("correct QQ answer", reply="721")
            resumed, q = wire()
            ctx.check(
                "only the original confirmed Social locator routes privately",
                "correct QQ answer" in str(resumed["messages"])
                and question in str(resumed["messages"]),
            )
            answer(q)
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'",
                    question,
                )
            )
            ctx.equal(
                "QQ response hits one Work without another neutral execution",
                rows(
                    "SELECT work FROM work_wait_routed WHERE event=?", original_answer
                ),
                [(question,)],
            )

            # A current authenticated client event can bind the target atomically across entries.
            def client_question():
                event = human("ask in the App", platform="app")
                _, q = wire()
                answer(
                    q, task("client-accept-" + str(event), "accept", goal="user answer")
                )
                got, q = wire()
                target = pair(got, "client-accept-" + str(event))["result"]["work_id"]
                answer(
                    q,
                    task(
                        "client-need-" + str(event),
                        "need_input",
                        reason="original client question",
                    ),
                )
                until(
                    lambda: rows(
                        "SELECT work FROM work_user_questions WHERE work=?", target
                    )
                )
                return target

            target = client_question()
            no_target = human("different client without explicit ID", platform="acp")
            _, neutral = wire()
            answer(neutral)
            ctx.equal(
                "cross entry without original Work remains neutral",
                rows("SELECT count(*) FROM work_wait_routed WHERE event=?", no_target),
                [(0,)],
            )
            missing = human(
                "wrong explicit ID", platform="acp", answer_work_id="nonexistent"
            )
            ctx.equal(
                "wrong explicit ID cannot fall back to another Work",
                rows(
                    "SELECT matched FROM work_explicit_answers WHERE event=?", missing
                ),
                [(0,)],
            )
            pin = human(
                "explicit answer for original Work",
                platform="acp",
                answer_work_id=target,
            )
            got, q = wire()
            ctx.check(
                "explicit ACP response is consumed by original Work only",
                "explicit answer for original Work" in str(got["messages"])
                and target in str(got["messages"]),
            )
            ctx.equal(
                "explicit response did not start another neutral turn",
                rows("SELECT count(*) FROM executions WHERE source=?", pin),
                [(0,)],
            )
            answer(q)
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'",
                    target,
                )
            )
            same = client_question()
            direct = human("same App entry needs no quote", platform="app")
            got, q = wire()
            answer(q)
            ctx.equal(
                "implicit App response matches most recent authorized question",
                rows("SELECT work FROM work_wait_routed WHERE event=?", direct),
                [(same,)],
            )
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'", same
                )
            )

            def branches(label):
                _, root, owner, q = new_work(label)
                answer(
                    q, call(label, "subagent_start", {"goal": "private child " + label})
                )
                first, q1 = wire()
                second, q2 = wire()
                if any(m.get("tool_call_id") == label for m in first["messages"]):
                    return root, owner, pair(first, label)["result"], q1, q2
                return root, owner, pair(second, label)["result"], q2, q1

            parent, manager, child, pq, cq = branches("terminal-parent")
            answer(
                pq,
                task("early-terminal", "complete", result="retained private partial"),
            )
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'",
                    parent,
                )
            )
            answer(
                cq,
                call(
                    "late-question",
                    "subagent_control",
                    {"action": "message", "ask": True, "text": "after terminal"},
                ),
            )
            got, cq = wire()
            result = pair(got, "late-question")["result"]
            ctx.equal(
                "late child stores question without waiting on a terminal parent",
                [result["parent_state"], result["wait"]],
                ["terminal", False],
            )
            answer(cq, text="independent child final")
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'",
                    child["child_id"],
                )
            )
            ctx.equal(
                "terminal parent has just the original execution",
                rows("SELECT count(*) FROM work_runs WHERE work=?", parent),
                [(1,)],
            )

            live, manager, child, pq, cq = branches("waiting-parent")
            answer(pq, task("live-owned", "wait", run_id=child["child_id"]))
            until(
                lambda: rows(
                    "SELECT id FROM work_waits WHERE work=? AND state='armed'", live
                )
            )
            answer(
                cq,
                call(
                    "genuine-question",
                    "subagent_control",
                    {"action": "message", "ask": True, "text": "source R15?"},
                ),
            )
            got, pq = wire()
            ctx.check(
                "business question reaches live parent independently of terminal completion",
                "source R15?" in str(got["messages"]) and live in str(got["messages"]),
            )
            message = rows(
                "SELECT id FROM work_mail WHERE work=? AND kind='question'", live
            )[0][0]
            logical, ordinal = rows(
                "SELECT logical,ordinal FROM model_tool_intents WHERE call_id='genuine-question'"
            )[0]
            op("model", "pair", logical, ordinal)
            op("model", "pair", logical, ordinal)
            ctx.equal(
                "repeat of the original question has one parent input",
                rows(
                    "SELECT count(*) FROM work_mail WHERE work=? AND kind='question'",
                    live,
                ),
                [(1,)],
            )
            answer(
                pq,
                call(
                    "private-answer",
                    "subagent_control",
                    {
                        "action": "message",
                        "child_id": child["child_id"],
                        "reply_to": str(message),
                        "text": "use R15",
                    },
                ),
            )
            first, q1 = wire()
            second, q2 = wire()
            if any(
                m.get("tool_call_id") == "private-answer" for m in first["messages"]
            ):
                pq, cq = q1, q2
            else:
                pq, cq = q2, q1
            answer(cq, text="private result")
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'",
                    child["child_id"],
                )
            )
            answer(pq, task("past-result", "wait", run_id=child["child_id"]))
            got, pq = wire()
            ctx.check(
                "owned child already terminal at registration wakes only its parent",
                "child:" + child["child_id"] + ":completed" in str(got["messages"]),
            )
            answer(pq, text="final")
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'", live
                )
            )

            paused, manager, child, pq, cq = branches("paused-parent")
            answer(pq, task("pause-owned", "wait", run_id=child["child_id"]))
            until(
                lambda: rows(
                    "SELECT id FROM work_waits WHERE work=? AND state='armed'", paused
                )
            )
            op("work", "suspend", manager, a, paused, "independent suspension")
            answer(
                cq,
                call(
                    "question-paused",
                    "subagent_control",
                    {"action": "message", "ask": True, "text": "save until resume"},
                ),
            )
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='need_input'",
                    child["child_id"],
                )
            )
            ctx.equal(
                "question retained without clearing independent suspension",
                (
                    rows("SELECT count(*) FROM work_suspensions WHERE work=?", paused),
                    rows("SELECT count(*) FROM work_runs WHERE work=?", paused),
                    rows(
                        "SELECT count(*) FROM work_mail WHERE work=? AND kind='question'",
                        paused,
                    ),
                ),
                ([(1,)], [(1,)], [(1,)]),
            )
            human("resume the suspended target explicitly")
            _, neutral = wire()
            answer(
                neutral,
                task(
                    "explicit-resume",
                    "resume",
                    work_id=paused,
                    reason="continue original",
                ),
            )
            first, q1 = wire()
            second, q2 = wire()
            if any(
                m.get("tool_call_id") == "explicit-resume" for m in first["messages"]
            ):
                neutral, pq, continued = q1, q2, second
            else:
                neutral, pq, continued = q2, q1, first
            ctx.check(
                "explicit resume can read saved question once",
                "save until resume" in str(continued["messages"]),
            )
            answer(neutral)
            answer(pq, text="partial")
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'",
                    paused,
                )
            )
            plugin_event = plugin(101, "original-telegram", "wait for this plugin")
            _, q = wire()
            answer(
                q,
                task("plugin-accept", "accept", goal="wait for host-verified gateway"),
            )
            got, q = wire()
            plugin_work = pair(got, "plugin-accept")["result"]["work_id"]
            answer(
                q,
                task(
                    "uninstalled-producer",
                    "wait",
                    conditions=[
                        {
                            "kind": "plugin_event",
                            "plugin_id": "uninstalled",
                            "event_type": "message",
                        }
                    ],
                ),
            )
            got, q = wire()
            ctx.equal(
                "cannot wait on an uninstalled event producer",
                pair(got, "uninstalled-producer")["state"],
                "failed",
            )
            answer(
                q,
                task(
                    "verified-gateway",
                    "wait",
                    conditions=[
                        {
                            "kind": "plugin_event",
                            "plugin_id": "waitgw",
                            "event_type": "message",
                        }
                    ],
                ),
            )
            until(
                lambda: rows(
                    "SELECT id FROM work_waits WHERE work=? AND state='armed'",
                    plugin_work,
                )
            )
            pure = plugin(102, "observation-only", "no business admission", False)
            ctx.equal("observation has no admission", pure["accepted"], False)
            ctx.equal(
                "observation cannot satisfy plugin condition",
                rows("SELECT count(*) FROM work_runs WHERE work=?", plugin_work),
                [(1,)],
            )
            match = plugin(103, "committed-business", "accepted plugin signal", True)
            viewed, q = wire()
            ctx.check(
                "same installed producer and original person wake the Work",
                plugin_work in str(viewed["messages"])
                and "accepted plugin signal" in str(viewed["messages"]),
            )
            answer(q)
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'",
                    plugin_work,
                )
            )
            ctx.equal(
                "same native plugin delivery reports its original event",
                plugin(104, "committed-business", "accepted plugin signal", True)[
                    "event"
                ],
                match["event"],
            )
            ctx.equal(
                "plugin signal has one durable wake",
                rows("SELECT count(*) FROM work_runs WHERE work=?", plugin_work),
                [(2,)],
            )

            _, reset_question, _, q = new_work("verify original question generation")
            answer(
                q,
                call("reset-social", "send", {"text": "question in old window"}),
                task("reset-need", "need_input", reason="wait in old window"),
            )
            while True:
                notice = peer.receive()
                if notice["action"] == "send_private_msg":
                    break
                peer.send(
                    {
                        "echo": notice.get("echo"),
                        "status": "ok",
                        "retcode": 0,
                        "data": {},
                    }
                )
            peer.send(
                {
                    "echo": notice["echo"],
                    "status": "ok",
                    "retcode": 0,
                    "data": {"message_id": 744},
                }
            )
            until(
                lambda: rows(
                    "SELECT outbound FROM work_user_questions WHERE work=? AND outbound IS NOT NULL",
                    reset_question,
                )
            )
            reset = human("/ai new")
            until(lambda: rows("SELECT event FROM main_controls WHERE event=?", reset))
            late_quote = human("old generation cannot match", reply="744")
            _, neutral = wire()
            answer(neutral)
            ctx.equal(
                "an original locator from an earlier generation cannot claim new input",
                rows("SELECT count(*) FROM work_wait_routed WHERE event=?", late_quote),
                [(0,)],
            )
            _, pending, pending_owner, q = new_work(
                "hold partial input until explicit resume"
            )
            answer(
                q,
                task(
                    "partial-all",
                    "wait",
                    wait_mode="all",
                    conditions=[
                        {"kind": "conversation"},
                        {"kind": "time_due", "after_seconds": 3600},
                    ],
                ),
            )
            until(
                lambda: rows(
                    "SELECT id FROM work_waits WHERE work=? AND state='armed'", pending
                )
            )
            partial_event = human("retain this private partial match")
            until(
                lambda: rows(
                    "SELECT w.id FROM work_mail w JOIN work_wait_mail m ON m.mail=w.id WHERE w.work=? AND m.source_event=?",
                    pending,
                    partial_event,
                )
            )
            ctx.equal(
                "a partial ALL input is retained without new purchase",
                rows("SELECT count(*) FROM work_runs WHERE work=?", pending),
                [(1,)],
            )
            human("explicitly resume to read the saved input")
            _, neutral = wire()
            answer(
                neutral,
                task(
                    "resume-partial",
                    "resume",
                    work_id=pending,
                    reason="read earlier match",
                ),
            )
            first, q1 = wire()
            second, q2 = wire()
            if any(
                m.get("tool_call_id") == "resume-partial" for m in first["messages"]
            ):
                neutral, pq, body = q1, q2, second
            else:
                neutral, pq, body = q2, q1, first
            ctx.check(
                "explicit resume preserves original partial input",
                "retain this private partial match" in str(body["messages"]),
            )
            ctx.equal(
                "cancelled pending wait retained one original match",
                rows(
                    "SELECT count(*) FROM work_mail WHERE work=? AND kind='matched_input'",
                    pending,
                ),
                [(1,)],
            )
            answer(neutral)
            answer(pq, text="complete after resume")
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'",
                    pending,
                )
            )
            ctx.write(
                "wait-plugin-events.json",
                {
                    "original": plugin_event,
                    "observation": pure,
                    "accepted": match,
                    "work": plugin_work,
                    "late_generation_event": late_quote,
                },
            )
            ctx.write(
                "wait-parent-questions.json",
                {
                    "cases": {
                        "R15": {
                            "terminal_parent": parent,
                            "live_parent": live,
                            "suspended_parent": paused,
                            "idempotent_question": message,
                            "explicit_client": pin,
                            "same_entry_client": direct,
                        }
                    }
                },
            )
            ctx.write(
                "wait-before-settle.json",
                {
                    "cases": {
                        "H05": {
                            "early_event": earlier,
                            "any": anywait,
                            "all_before_restart": allwait,
                            "all_after_restart": all_done,
                            "clock": clockwait,
                            "clock_work": work2,
                            "question": question,
                            "original_locator": "721",
                            "answer_event": original_answer,
                        }
                    }
                },
            )
    return {
        "early": anywait,
        "partial": allwait,
        "complete": all_done,
        "original_work": work,
        "time_work": work2,
        "question_work": question,
        "progress_updates": 10,
        "model_requests_after_progress": prior,
    }
