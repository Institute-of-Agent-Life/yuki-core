"""Drive the native Main through real purchases, ordered physical delivery, and a killed writer."""

import json
import sqlite3
import time
import threading
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from tests.harness.service import Service, previous_schema
from tests.harness.crash import kill_writer
from tests.peers.chat.recorder import Recorder


def call(id, name, **args):
    return {
        "id": id,
        "type": "function",
        "function": {"name": name, "arguments": json.dumps(args, ensure_ascii=False)},
    }


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


def run(ctx):
    from tests.harness.work_segments import run as segment_recovery

    ctx.write("work-segment-recovery.json", segment_recovery(ctx))
    svc = Service(ctx, ctx.working / "work")
    previous_schema(ctx, svc.data, 13)
    (svc.data / "SOUL.md").write_bytes((ctx.root / "character/SOUL.md").read_bytes())
    ctx.check(
        "upgrade original Host reader", svc.command("migrate")["schema_version"] >= 14
    )

    def op(*args):
        return svc.command("operator", *map(str, args))

    def rows(sql, *args):
        with closing(
            sqlite3.connect((svc.data / "core.sqlite").as_uri() + "?mode=ro", uri=True)
        ) as db:
            return db.execute(sql, args).fetchall()

    def until(fn):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            v = fn()
            if v:
                return v
            time.sleep(0.03)
        raise AssertionError(
            "timed out: "
            + str(
                rows(
                    "SELECT x.state,t.diagnostic FROM executions x LEFT JOIN main_turns t ON t.execution=x.id"
                )
            )
        )

    def wire():
        raw, q = peer.receive()
        return json.loads(raw), q

    def paired(body, id):
        return json.loads(
            next(
                m["content"]
                for m in reversed(body["messages"])
                if m.get("tool_call_id") == id
            )
        )

    seq = 0
    actor = "00000000-0000-4000-8000-000000000016"

    def human(text):
        nonlocal seq
        seq += 1
        frame = {
            "source": "work-" + str(seq),
            "account": user["account"],
            "bot_stage": bot["assignment"],
            "occurred": 100 + seq,
            "observed": 100 + seq,
            "kind": "message",
            "segments": [{"type": "text", "text": text}],
        }
        return op("ingress", "record", json.dumps(frame), "--accept")["event"]

    def task(id, action, **args):
        return call(id, "task_control", action=action, **args)

    with svc.running():
        ctx.write("sqlite-build.json", op("database-info"))
        op("person", "create", actor)
        user = op(
            "account", "open", "qq", "work", "200", 1, "--person", actor, "--start", 1
        )
        bot = op("account", "open", "qq", "work", "100", 1, "--start", 1)
        entry = op("entry", "private", user["assignment"], bot["assignment"])["entry"]
        original = human("do the durable job and acknowledge delivery")

    with Recorder() as peer:
        with (svc.data / "yuki.toml").open("a") as f:
            f.write(
                f'\n[main]\nendpoint="{peer.url}"\nbinding_endpoint="{peer.url.removesuffix("/v1/chat/completions")}/binding"\nmodel="fixture-work"\nname="Yuki"\nmerge_ms=5\nroot_slots=24\nmax_input_units=2000000\nmax_output=128\n'
            )
        with ThreadPoolExecutor(max_workers=1) as pool:
            death = pool.submit(
                kill_writer,
                ctx,
                svc.data,
                "work_delivery_crash_child",
                "delivered-before-pair",
                {},
            )
            first, q = wire()
            ctx.check(
                "neutral human receives scoped no_active_work",
                "no_active_work" in first["messages"][-2]["content"],
            )
            answer(q, task("accept", "accept", goal="inspect the original"))
            second, q = wire()
            accepted = paired(second, "accept")
            ctx.equal("accept returns current Work", accepted["state"], "confirmed")
            root = accepted["result"]["work_id"]
            exec = rows("SELECT id FROM executions WHERE source=?", original)[0][0]
            ctx.equal(
                "attach preserves original paid snapshot",
                rows("SELECT original_work_id FROM model_requests ORDER BY ordinal"),
                [(None,), (root,)],
            )
            answer(
                q,
                task("finish", "complete", result="private result"),
                call("send", "send", text="exactly one public delivery"),
            )
            death.result()
        ctx.equal(
            "physical recipient saw one original operation", len(peer.deliveries), 1
        )
        ctx.equal(
            "crash before final receipt pairing",
            rows(
                "SELECT i.paired_receipt,o.state FROM model_tool_intents i JOIN operations o ON o.namespace=i.execution AND o.operation_id=i.operation_id WHERE i.call_id='send'"
            ),
            [(None, "confirmed")],
        )
        with svc.running():
            until(
                lambda: rows(
                    "SELECT summary FROM work_nodes WHERE id=? AND status='completed'",
                    root,
                )
            )
            ctx.equal(
                "restart paired original receipt without purchase or redelivery",
                (
                    len(peer.deliveries),
                    rows("SELECT count(*) FROM model_attempts"),
                    peer.wires.empty(),
                ),
                (1, [(2,)], True),
            )
            ctx.check(
                "original parent retained actual effects",
                len(rows("SELECT ref FROM work_activities WHERE work=?", root)) >= 3,
            )
            ctx.write(
                "settlement-tail.json",
                {
                    "cases": {
                        "H04": {
                            "original_execution": exec,
                            "work": root,
                            "receipt": peer.deliveries[0],
                            "physical_requests": 2,
                            "crash_after_domain_before_pair": True,
                        }
                    }
                },
            )
            human("neutral action matrix")
            _, q = wire()
            answer(
                q,
                task("read", "get", work_id=root),
                task("derive", "derive", goal="not authorized"),
                call("start", "subagent_start", goal="needs parent"),
                call("put", "put", target="ordinary", value=3),
            )
            got, q = wire()
            ctx.equal(
                "neutral Work rights depend on action",
                [paired(got, k)["state"] for k in ["read", "derive", "start", "put"]],
                ["confirmed", "failed", "failed", "confirmed"],
            )
            ctx.equal(
                "ordinary local write creates no Work",
                rows("SELECT count(*) FROM work_nodes"),
                [(1,)],
            )
            answer(q)
            until(
                lambda: rows("SELECT count(*) FROM executions WHERE state='completed'")[
                    0
                ][0]
                >= 2
            )

            human("start a tree with one private worker")
            _, q = wire()
            answer(q, task("accept2", "accept", goal="parent goal"))
            got, q = wire()
            parent = paired(got, "accept2")["result"]["work_id"]
            answer(
                q,
                call(
                    "child",
                    "subagent_start",
                    goal="private child",
                    context="child-only seed",
                ),
            )
            a, qa = wire()
            b, qb = wire()
            if any(m.get("tool_call_id") == "child" for m in a["messages"]):
                parent_body, parent_q, child_body, child_q = a, qa, b, qb
            else:
                parent_body, parent_q, child_body, child_q = b, qb, a, qa
            child = paired(parent_body, "child")["result"]["child_id"]
            child_run = paired(parent_body, "child")["result"]["execution"]
            ctx.check(
                "private assignment delivered to child",
                any(
                    "child-only seed" in str(m.get("content"))
                    for m in child_body["messages"]
                ),
            )
            ctx.equal(
                "worker inherits root budget and full manifest",
                rows(
                    "SELECT count(distinct b.root),count(distinct v.revision) FROM budget_members b JOIN execution_tool_views v ON v.execution=b.execution JOIN work_runs r ON r.execution=b.execution WHERE r.work IN (?,?)",
                    parent,
                    child,
                ),
                [(1, 1)],
            )
            answer(
                child_q,
                task("again", "accept", goal="denied"),
                task("foreign_parent", "get", work_id=parent),
            )
            got, child_q = wire()
            ctx.equal(
                "worker cannot accept again or manage outside its tree",
                [paired(got, k)["state"] for k in ["again", "foreign_parent"]],
                ["failed", "failed"],
            )
            answer(
                child_q,
                call(
                    "question",
                    "subagent_control",
                    action="message",
                    ask=True,
                    text="which private source?",
                ),
            )
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='need_input'",
                    child,
                )
            )
            answer(parent_q, task("question-status", "get", work_id=child))
            question_body, parent_q = wire()
            ctx.check(
                "parent sees question through own private inbox",
                "which private source?" in question_body["messages"][-1]["content"],
            )
            question_id = rows(
                "SELECT id FROM work_mail WHERE work=? AND kind='question'", parent
            )[0][0]
            answer(
                parent_q,
                call(
                    "bad-reply",
                    "subagent_control",
                    action="message",
                    child_id=child,
                    reply_to="99999",
                    text="wrong",
                ),
                call(
                    "answer",
                    "subagent_control",
                    action="message",
                    child_id=child,
                    reply_to=str(question_id),
                    text="use private source R",
                ),
                task(
                    "bad-note",
                    "update",
                    context_note={
                        "facts": [{"text": "no evidence", "refs": ["event:99999"]}]
                    },
                ),
                task(
                    "note",
                    "update",
                    context_note={
                        "facts": [
                            {
                                "text": "retain partial findings",
                                "refs": ["goal", "child:" + child],
                            }
                        ]
                    },
                ),
            )
            a, qa = wire()
            b, qb = wire()
            if any(m.get("tool_call_id") == "answer" for m in a["messages"]):
                management, parent_q, child_body, child_q = a, qa, b, qb
            else:
                management, parent_q, child_body, child_q = b, qb, a, qa
            ctx.equal(
                "reply and note check exact original references",
                [
                    paired(management, k)["state"]
                    for k in ["bad-reply", "answer", "bad-note", "note"]
                ],
                ["failed", "confirmed", "failed", "confirmed"],
            )
            ctx.check(
                "child resumes with original private reply",
                "use private source R" in str(child_body["messages"]),
            )
            answer(child_q, text="private partial result")
            until(
                lambda: rows(
                    "SELECT id FROM work_mail WHERE work=? AND kind='child_terminal'",
                    parent,
                )
            )
            answer(parent_q, task("readchild", "get", work_id=child))
            got, parent_q = wire()
            ctx.check(
                "parent reads actual child result",
                "private partial result" in str(got["messages"][-1]),
            )
            ctx.equal(
                "private data has no public event",
                rows(
                    "SELECT count(*) FROM events WHERE envelope LIKE '%private partial result%'"
                ),
                [(0,)],
            )
            answer(
                parent_q,
                task("proposal", "complete", result="parent retained partial result"),
            )
            until(
                lambda: rows(
                    "SELECT summary FROM work_nodes WHERE id=? AND status='completed'",
                    parent,
                )
            )
            ctx.equal(
                "private input watermark committed once",
                rows(
                    "SELECT count(*) FROM work_consumption WHERE work IN (?,?)",
                    parent,
                    child,
                ),
                [(2,)],
            )
            ctx.equal("no extra public delivery", len(peer.deliveries), 1)

            human("cancel self")
            _, q = wire()
            answer(q, task("accept3", "accept", goal="cancel me"))
            got, q = wire()
            self_id = paired(got, "accept3")["result"]["work_id"]
            answer(
                q,
                task("cancel-self", "cancel"),
                call("tail", "send", text="suppressed"),
            )
            until(
                lambda: rows(
                    "SELECT i.paired_receipt FROM model_tool_intents i WHERE i.call_id='cancel-self' AND i.paired_receipt IS NOT NULL"
                )
            )
            ctx.equal(
                "self-cancel preserves unsent tail",
                rows(
                    "SELECT o.state FROM operations o JOIN model_tool_intents i ON i.execution=o.namespace AND i.operation_id=o.operation_id WHERE i.call_id='tail'"
                ),
                [("intent",)],
            )
            ctx.equal(
                "cancelled delivery never reaches recipient", len(peer.deliveries), 1
            )
            human("a fresh action targets another Work and then sends")
            _, q = wire()
            answer(
                q,
                task("other", "cancel", work_id=self_id),
                call("allowed", "send", text="still eligible"),
            )
            got, q = wire()
            ctx.equal(
                "other cancellation does not suppress eligible send",
                paired(got, "allowed")["state"],
                "confirmed",
            )
            answer(q)
            until(lambda: len(peer.deliveries) == 2)

            # A past terminal and later unknown attempt stay attached to their own runs.
            human("revisit the completed original with fresh evidence")
            _, q = wire()
            answer(
                q,
                task(
                    "resume-original",
                    "resume",
                    work_id=root,
                    reason="fresh evidence one",
                ),
            )
            a, qa = wire()
            b, qb = wire()
            if any(m.get("tool_call_id") == "resume-original" for m in a["messages"]):
                resumed, a_q, neutral_q = b, qb, qa
            else:
                resumed, a_q, neutral_q = a, qa, qb
            ctx.check(
                "new business input continues a completed Work",
                "fresh evidence one" in str(resumed["messages"]),
            )
            answer(neutral_q)
            a_q.put((b'{"partial":', 12))
            until(
                lambda: rows(
                    "SELECT p.id FROM model_attempts p JOIN model_requests r ON r.id=p.logical JOIN activations a ON a.id=r.activation JOIN work_runs w ON w.execution=a.execution JOIN executions x ON x.id=w.execution WHERE w.work=? AND p.state='unknown' AND x.state='held'",
                    root,
                )
            )
            human("continue after an unknown only because this is new input")
            _, q = wire()
            answer(
                q,
                task("new-input", "resume", work_id=root, reason="fresh evidence two"),
            )
            a, qa = wire()
            b, qb = wire()
            if any(m.get("tool_call_id") == "new-input" for m in a["messages"]):
                resumed, a_q, neutral_q = b, qb, qa
            else:
                resumed, a_q, neutral_q = a, qa, qb
            ctx.check(
                "new run consumes only the new instruction",
                "fresh evidence two" in str(resumed["messages"])
                and "fresh evidence one" not in str(resumed["messages"]),
            )
            answer(a_q, text="")
            answer(neutral_q, text="")
            until(
                lambda: rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed' AND summary=''",
                    root,
                )
            )
            ctx.equal(
                "old unknown attempt retained once",
                rows("SELECT count(*) FROM model_attempts WHERE ever_unknown"),
                [(1,)],
            )
            ctx.equal(
                "old send not replayed on new business input", len(peer.deliveries), 2
            )

            # The recipient accepts the original child call, then blocks its HTTP ACK.
            human("cancel a whole tree during a physical child effect")
            _, q = wire()
            answer(q, task("late-accept", "accept", goal="cancel this branch"))
            current, q = wire()
            cancelled_root = paired(current, "late-accept")["result"]["work_id"]
            answer(q, call("late-child", "subagent_start", goal="one late receipt"))
            a, qa = wire()
            b, qb = wire()
            if any(m.get("tool_call_id") == "late-child" for m in a["messages"]):
                original_child, parent_q, child_q = (
                    paired(a, "late-child")["result"],
                    qa,
                    qb,
                )
            else:
                original_child, parent_q, child_q = (
                    paired(b, "late-child")["result"],
                    qb,
                    qa,
                )
            received, release = threading.Event(), threading.Event()

            def slow_receipt(_call):
                received.set()
                release.wait(15)

            peer.before_binding_reply = slow_receipt
            try:
                answer(
                    child_q,
                    call(
                        "late-send", "send", text="retain original recipient evidence"
                    ),
                )
                ctx.check("actual recipient got the child effect", received.wait(10))
                answer(
                    parent_q,
                    task("cancel-tree", "cancel", reason="keep partial results"),
                )
                until(
                    lambda: rows(
                        "SELECT count(*) FROM work_nodes WHERE root=? AND status='cancelled'",
                        cancelled_root,
                    )
                    == [(2,)]
                )
            finally:
                release.set()
                peer.before_binding_reply = None
            late = until(
                lambda: rows(
                    "SELECT o.namespace,o.operation_id,o.state FROM operations o JOIN model_tool_intents i ON i.operation_id=o.operation_id AND i.execution=o.namespace WHERE i.call_id='late-send' AND o.state='confirmed'"
                )
            )
            ctx.equal(
                "late physical receipt stays on original cancelled child",
                late[0][:2],
                (original_child["execution"], peer.deliveries[-1]["operation"]),
            )
            ctx.equal(
                "both actual executions are held after subtree cancellation",
                rows(
                    "SELECT count(*) FROM work_runs r JOIN work_nodes w ON w.id=r.work JOIN executions x ON x.id=r.execution WHERE w.root=? AND x.cancelled AND x.state='held'",
                    cancelled_root,
                ),
                [(2,)],
            )

            ctx.equal(
                "cancelled child keeps the late ledger transition",
                rows(
                    "SELECT kind FROM work_activities WHERE work=? AND ref=? ORDER BY rowid",
                    original_child["child_id"],
                    late[0][1],
                ),
                [("dispatched",), ("confirmed",)],
            )
            op("main", "register", "plugin", "foreign-source", entry)
            foreign = op(
                "main",
                "trigger",
                "foreign-source",
                "different-principal",
                json.dumps({"text": "look at the directory"}),
            )["execution"]
            _, q = wire()
            answer(
                q, task("guess", "get", work_id=root), task("dir", "list", status="all")
            )
            got, q = wire()
            ctx.equal(
                "unrelated registered source cannot see or control person Work",
                [paired(got, "guess")["state"], paired(got, "dir")["result"]["items"]],
                ["failed", []],
            )
            answer(q)
            until(
                lambda: rows(
                    "SELECT id FROM executions WHERE id=? AND state='completed'",
                    foreign,
                )
            )
            ctx.write(
                "work-action-matrix.json",
                {
                    "cases": {
                        "H10": {
                            "neutral": ["read original", "ordinary write"],
                            "refused": [
                                "derive without Work",
                                "start without Work",
                                "worker accept",
                                "cross tree",
                                "foreign owner",
                            ],
                            "cancelled_self": self_id,
                            "permitted_tail": peer.deliveries[1],
                        }
                    }
                },
            )
            tail = json.loads((ctx.output / "settlement-tail.json").read_bytes())
            tail["cases"]["H04"].update(
                {
                    "self_cancelled": self_id,
                    "other_target_delivery": peer.deliveries[1],
                    "late_receipt": late[0],
                    "original_unknown_attempts": 1,
                    "empty_final_work": root,
                }
            )
            ctx.write("settlement-tail.json", tail)
            return {
                "original": original,
                "root": root,
                "parent": parent,
                "child": child,
                "child_run": child_run,
                "tree": rows("SELECT id,parent,status FROM work_nodes ORDER BY rowid"),
                "private_mail": rows("SELECT work,kind,id FROM work_mail ORDER BY id"),
                "recipient": peer.deliveries,
                "original_unknown_attempts": rows(
                    "SELECT id FROM model_attempts WHERE ever_unknown"
                ),
                "late_receipt": late[0],
            }
