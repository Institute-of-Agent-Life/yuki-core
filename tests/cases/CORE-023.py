"""The original receipt's paid batch survives two process deaths before normal observation."""

import json
import os
from contextlib import closing
from tests.peers.onebot.peer import Peer
from concurrent.futures import ThreadPoolExecutor
from tests.harness.compact import Owner, call, reply, sha, approx
from tests.harness.crash import kill_writer
from tests.peers.chat.recorder import Recorder


def round_case(ctx, state):
    o = Owner(ctx, "H07-" + state, 18)
    with o.svc.running():
        event = o.human("current original requirement " + state)
    with Recorder(binding_status=200 if state == "confirmed" else 503) as http:
        o.config(http, "compact_soft_tokens=1\n")
        with ThreadPoolExecutor(max_workers=1) as pool:
            death = pool.submit(
                kill_writer,
                ctx,
                o.svc.data,
                "compact_crash_child",
                "paired-before-normal-observation",
                {},
                sample=state,
            )
            original, q = o.wire(http)
            calls = [
                call("prior-read", "get", {"target": "fixture"}),
                call(
                    "original-send", "send", {"text": "one original delivery " + state}
                ),
            ]
            reply(q, calls=calls)
            death.result()
            source = o.rows(
                "SELECT x.id,x.work_id FROM executions x WHERE x.source=?", event
            )[0]
            paired = o.rows(
                "SELECT i.call_id,i.operation_id,i.paired_receipt FROM model_tool_intents i WHERE i.execution=? ORDER BY i.ordinal",
                source[0],
            )
            ctx.check(
                "complete original paid round paired before a normal response",
                len(paired) == 2
                and all(r[2] for r in paired)
                and len(http.deliveries) == 1,
            )
            ctx.equal("original send state", json.loads(paired[1][2])["state"], state)
            ctx.equal(
                "no maintenance or next normal request at first kill",
                o.rows("SELECT count(*) FROM model_attempts"),
                [(1,)],
            )
            death = pool.submit(
                kill_writer,
                ctx,
                o.svc.data,
                "compact_crash_child",
                "private-response-before-publication",
                {},
                sample=state,
            )
            compact, q = o.wire(http)
            ctx.check(
                "E uses original paid input plus instruction, with the complete paired batch",
                compact["messages"][-1]["content"].startswith("[Host 上下文压缩要求]")
                and [
                    m["tool_call_id"]
                    for m in compact["messages"]
                    if m["role"] == "tool"
                ]
                == [r[0] for r in paired],
            )
            ctx.check(
                "ordinary original E has no business Work",
                source[1] is None
                and compact["messages"][-1]["content"].endswith("null"),
            )
            reply(q, "stored private checkpoint " + state)
            death.result()
        ctx.equal(
            "second process death leaves paid response before publication",
            o.rows(
                "SELECT j.state,p.state FROM exec_compacts j JOIN model_attempts p ON p.logical=j.logical"
            ),
            [("pending", "response")],
        )
        with o.svc.running():
            normal, q = o.wire(http)
            content = json.dumps(normal, ensure_ascii=False)
            ctx.check(
                "next normal wire contains own checkpoint, initial goal and full original effect keys",
                "stored private checkpoint " + state in content
                and "current original requirement " + state in content
                and all(r[1] in content for r in paired),
            )
            ctx.equal(
                "next Chat wire retains call IDs, original arguments and exact persisted receipt text",
                (
                    [
                        m["tool_calls"]
                        for m in normal["messages"]
                        if m.get("tool_calls")
                    ],
                    [
                        (m["tool_call_id"], m["content"])
                        for m in normal["messages"]
                        if m["role"] == "tool"
                    ],
                ),
                ([calls], [(r[0], r[2]) for r in paired]),
            )
            ctx.equal(
                "three requests on one root; no replay of physical sends or E",
                (
                    o.rows("SELECT count(*),count(distinct root) FROM model_requests"),
                    len(http.deliveries),
                    o.rows("SELECT state FROM exec_compacts"),
                ),
                ([(3, 1)], 1, [("published",)]),
            )
            reply(q, "original work complete")
            o.finished(event)
            ctx.write("next-normal-" + state + ".json", normal)
    return {
        "execution": source[0],
        "original": sha(original),
        "paid_private": sha(compact),
        "normal": sha(normal),
        "calls": paired,
    }


def gemini_text(q, text):
    q.put(
        (
            {
                "candidates": [
                    {
                        "finishReason": "STOP",
                        "content": {"role": "model", "parts": [{"text": text}]},
                    }
                ]
            },
            0,
        )
    )


def native_handoff(ctx):
    o = Owner(ctx, "native-E-to-P", 18)
    with Recorder("/v1beta/models/fixture:generateContent") as http:
        o.config(http, 'protocol="gemini"\ncompact_soft_tokens=23000\n')
        with o.svc.running():
            seed = o.human("PUBLIC SOURCE FOR NATIVE COMPACT")
            _, q = o.wire(http)
            gemini_text(q, "seed done")
            o.finished(seed)
            current = o.human("THIS ACTIVATION IS PRIVATE")
            ordinary, q = o.wire(http)
            args = {"target": "original lookup " + "原" * 14_000}
            signed = {
                "functionCall": {"name": "get", "args": args, "id": "native-original"},
                "thoughtSignature": "only-original-physical-chain",
            }
            q.put(
                (
                    {
                        "candidates": [
                            {
                                "finishReason": "STOP",
                                "content": {"role": "model", "parts": [signed]},
                            }
                        ]
                    },
                    0,
                )
            )
            private, q = o.wire(http)
            raw = json.dumps(private, ensure_ascii=False)
            ctx.check(
                "native E includes exact original signed Part and paired result",
                "[Host 上下文压缩要求]" in raw
                and any(signed in c["parts"] for c in private["contents"])
                and "functionResponse" in raw,
            )
            gemini_text(q, "private execution checkpoint")
            public, q = o.wire(http)
            raw = json.dumps(public["contents"], ensure_ascii=False)
            ctx.check(
                "retiring the native signature makes the original P legal once",
                "[Host 会话上下文压缩要求]" in raw
                and "PUBLIC SOURCE FOR NATIVE COMPACT" in raw
                and all(
                    x not in raw
                    for x in (
                        "private execution checkpoint",
                        "THIS ACTIVATION IS PRIVATE",
                        "only-original-physical-chain",
                        "native-original",
                    )
                ),
            )
            gemini_text(q, "published public checkpoint")
            normal, q = o.wire(http)
            tags = [
                part["text"]
                for row in normal["contents"]
                for part in row["parts"]
                if "text" in part
            ]
            ctx.check(
                "new native chain has both checkpoints and its original private input",
                "published public checkpoint" in str(tags)
                and "private execution checkpoint" in str(tags)
                and "THIS ACTIVATION IS PRIVATE" in str(tags)
                and "PUBLIC SOURCE FOR NATIVE COMPACT" not in str(tags)
                and "only-original-physical-chain" not in str(normal),
            )
            portable = next(
                json.loads(t)
                for t in tags
                if t.startswith("{")
                and json.loads(t).get("kind") == "work_unobserved_tool_round"
            )
            row = o.rows(
                "SELECT i.operation_id,i.paired_receipt FROM model_tool_intents i WHERE i.call_id='native-original'"
            )[0]
            c = portable["calls"][0]
            ctx.equal(
                "native receipt explicitly carries database operation, exact arguments and output",
                (
                    c["call_id"],
                    c["effect_key"],
                    c["tool"],
                    json.loads(c["arguments"]),
                    c["result"],
                ),
                ("native-original", row[0], "get", args, row[1]),
            )
            ctx.equal(
                "new native business chain contains no transplanted functionResponse",
                any(
                    "functionResponse" in p
                    for m in normal["contents"]
                    for p in m["parts"]
                ),
                False,
            )
            gemini_text(q, "observed")
            o.finished(current)
            ctx.equal(
                "original E and P paid only once",
                o.rows(
                    "SELECT (SELECT count(*) FROM exec_compacts),(SELECT count(*) FROM compact_jobs)"
                ),
                [(1, 1)],
            )
            ctx.write("native-E-P-next.json", normal)
            return {
                "ordinary": sha(ordinary),
                "E": sha(private),
                "P": sha(public),
                "normal": sha(normal),
                "effect_key": row[0],
            }


def public_handoff(ctx, reject):
    o = Owner(ctx, "P-to-E-" + str(reject), 18)
    with Recorder() as http:
        o.config(http, "compact_soft_tokens=23000\n")
        with o.svc.running():
            seed = o.human("source for P fallback")
            _, q = o.wire(http)
            reply(q, "ok")
            o.finished(seed)
            current = o.human("private present input")
            _, q = o.wire(http)
            args = {"target": "expensive paired private input " + "重" * 14_000}
            reply(q, calls=[call("h07-original", "get", args)])
            public, q = o.wire(http)
            ctx.check(
                "eligible P precedes private E",
                "[Host 会话上下文压缩要求]" in public["messages"][-1]["content"]
                and "private present input" not in str(public["messages"][-1]),
            )
            if reject:
                reply(q, calls=[call("forbidden", "send", {"text": "not permitted"})])
            else:
                reply(q, "approved public checkpoint")
            private, q = o.wire(http)
            ctx.check(
                "remaining private paired batch reaches E",
                "[Host 上下文压缩要求]" in private["messages"][-1]["content"]
                and "h07-original" in str(private),
            )
            reply(q, "paid private fallback")
            normal, q = o.wire(http)
            ctx.write(f"P-to-E-normal-{reject}.json", normal)
            ctx.check(
                "known candidate never purchased again and original pair preserved",
                "paid private fallback" in str(normal)
                and any(
                    m.get("tool_call_id") == "h07-original" for m in normal["messages"]
                )
                and "private present input" in str(normal),
            )
            ctx.equal(
                "public approval or rejection retained",
                o.rows("SELECT state FROM compact_jobs"),
                [("rejected" if reject else "published",)],
            )
            ctx.equal(
                "E paid once", o.rows("SELECT count(*) FROM exec_compacts"), [(1,)]
            )
            reply(q, "observed")
            o.finished(current)
            return {
                "P": sha(public),
                "E": sha(private),
                "normal": sha(normal),
                "public_accepted": not reject,
            }


def actual_partial(ctx):
    o = Owner(ctx, "native-partial", 18)
    with Recorder() as http:
        with (o.svc.data / "yuki.toml").open("a") as f:
            f.write(
                f'\n[main]\nendpoint="{http.url}"\nmodel="fixture"\nname="Yuki"\nmax_output=128\nroot_slots=10\nmax_input_units=4000000\nmerge_ms=1\ncompact_soft_tokens=1\n[qq]\nnamespace="compact"\ndesignated_ingress="100"\n[[qq.bots]]\nself_id="100"\nstage="{o.bot["assignment"]}"\ntoken_env="YUKI_COMPACT_QQ_KEY"\n'
            )
        env = {**os.environ, "YUKI_COMPACT_QQ_KEY": "offline-test"}
        with (
            o.svc.running(environment=env),
            closing(Peer(o.svc.address, "100", env["YUKI_COMPACT_QQ_KEY"])) as peer,
        ):
            event = o.human("keep each original sent segment")
            _, q = o.wire(http)
            msg = "part one\npart two\npart three"
            reply(q, calls=[call("partial", "send", {"text": msg})])
            first = peer.receive()
            ctx.equal("native first dispatch", "send_private_msg", first["action"])
            peer.send(
                {
                    "echo": first["echo"],
                    "retcode": 0,
                    "status": "ok",
                    "data": {"message_id": 7001},
                }
            )
            second = peer.receive()
            ctx.equal(
                "native second dispatch before disconnect",
                "send_private_msg",
                second["action"],
            )
            peer.close()
            pr, q = o.wire(http)
            ctx.check(
                "private E sees the already paired native partial batch",
                "[Host 上下文压缩要求]" in pr["messages"][-1]["content"],
            )
            receipt = next(
                m["content"]
                for m in pr["messages"]
                if m.get("tool_call_id") == "partial"
            )
            fields = json.loads(receipt)
            parts = fields["result"]["deliveries"]
            ctx.equal(
                "partial native delivery keeps original confirmed/unknown/unattempted boundaries",
                [(r["state"], r["locator"]) for r in parts],
                [("confirmed", "7001"), ("unknown", None), ("prepared", None)],
            )
            ctx.equal(
                "confirmed outbound ID references its actual source",
                o.rows("SELECT id,child FROM social_outbound"),
                [(parts[0]["outbound_id"], parts[0]["child"])],
            )
            reply(q, "partially delivered checkpoint")
            normal, q = o.wire(http)
            ctx.equal(
                "next normal wire preserves each received key and full receipt exactly",
                next(
                    m["content"]
                    for m in normal["messages"]
                    if m.get("tool_call_id") == "partial"
                ),
                receipt,
            )
            reply(q, "observed")
            o.finished(event)
            ctx.write("partial-native-next.json", normal)
            return {"next": sha(normal), "operation": fields["id"], "parts": parts}


def faults(ctx):
    records = {}
    hard_capacity = 64_000
    for kind in ("unknown", "reset", "erased", "hard"):
        o = Owner(ctx, "private-" + kind, 18)
        with Recorder() as http:
            o.config(
                http,
                "compact_soft_tokens=1\n"
                + (f"max_input_tokens={hard_capacity}\n" if kind == "hard" else ""),
            )
            with o.svc.running():
                event = o.human("source-" + kind)
                original, q = o.wire(http)
                if kind == "hard":
                    ctx.check("hard-cut fixture admits the full original declaration",approx(original) < hard_capacity)
                args = {
                    "target": "needed unobserved result "
                    + ("重" * hard_capacity if kind == "hard" else "private")
                }
                reply(q, calls=[call("source-call", "get", args)])
                if kind == "hard":
                    o.until(
                        lambda: o.rows(
                            "SELECT 1 FROM executions WHERE source=? AND state='held'",
                            event,
                        )
                    )
                    ctx.equal(
                        "original hard cap cannot discard a required unobserved tool round",
                        o.rows(
                            "SELECT diagnostic FROM main_turns t JOIN executions x ON x.id=t.execution WHERE x.source=?",
                            event,
                        ),
                        [("model_request_capacity",)],
                    )
                    ctx.equal(
                        "no paid E when no admissible hard cut",
                        o.rows("SELECT count(*) FROM exec_compacts"),
                        [(0,)],
                    )
                    ctx.equal("hard failure retains exactly the first real purchase",o.rows("SELECT count(*) FROM model_attempts"),[(1,)])
                    records[kind] = {"diagnostic":"model_request_capacity","initial_input_units":approx(original),"hard_capacity":hard_capacity,"required_call_argument_characters":len(args["target"]),"physical_attempts":1}
                    continue
                private, q = o.wire(http)
                ctx.check(
                    "original private request sent for " + kind,
                    "[Host 上下文压缩要求]" in private["messages"][-1]["content"],
                )
                if kind == "unknown":
                    q.put(
                        (
                            {
                                "choices": [
                                    {
                                        "message": {
                                            "role": "assistant",
                                            "content": "interrupted",
                                        },
                                        "finish_reason": "stop",
                                    }
                                ]
                            },
                            9,
                        )
                    )
                    o.until(
                        lambda: o.rows(
                            "SELECT 1 FROM executions WHERE source=? AND state='held'",
                            event,
                        )
                    )
                else:
                    if kind == "reset":
                        reset = o.human("/ai new")
                        o.until(
                            lambda: o.rows(
                                "SELECT event FROM main_controls WHERE event=?", reset
                            )
                        )
                    else:
                        o.op(
                            "privacy", "erase", "forget-E", o.actor, json.dumps([event])
                        )
                    reply(q, "unpublishable paid E")
                    o.until(
                        lambda: o.rows(
                            "SELECT 1 FROM exec_compacts j JOIN model_attempts p ON j.logical=p.logical WHERE p.state!='dispatched'"
                        )
                    )
                record = o.rows(
                    "SELECT j.state,j.summary,j.selected,p.state FROM exec_compacts j JOIN model_attempts p ON p.logical=j.logical"
                )
                ctx.check(
                    kind + " cannot publish or make another physical attempt",
                    len(record) == 1
                    and record[0][0] in ("rejected", "invalid")
                    and record[0][1] is None
                    and o.rows("SELECT count(*) FROM model_attempts") == [(2,)],
                )
                if kind == "unknown":
                    ctx.equal(
                        "truncated private response stays unknown and cannot open normal continuation",
                        record[0][3],
                        "unknown",
                    )
                else:
                    ctx.equal(
                        "invalidated private protocol objects were scrubbed",
                        record[0][2],
                        None,
                    )
                records[kind] = {"job": record[0][0], "attempt": record[0][3]}
    return records


def work_scope(ctx):
    o = Owner(ctx, "bound-Work", 18)
    with Recorder() as http:
        o.config(http, "compact_soft_tokens=1\n")
        with o.svc.running():
            event = o.human("accept private Work")
            _, q = o.wire(http)
            reply(
                q,
                calls=[
                    call(
                        "accept",
                        "task_control",
                        {"action": "accept", "goal": "EXPLICIT PRIVATE WORK GOAL"},
                    ),
                    call("original", "get", {"target": "thing"}),
                ],
            )
            private, q = o.wire(http)
            ctx.check(
                "E focus comes from the actual bound Work",
                "EXPLICIT PRIVATE WORK GOAL" in private["messages"][-1]["content"],
            )
            fresh = o.human("NEW AUTHORIZED HUMAN INPUT")
            o.until(
                lambda: o.rows("SELECT execution FROM main_inputs WHERE event=?", fresh)
            )
            reply(q, "own private Work checkpoint")
            normal, q = o.wire(http)
            ctx.check(
                "fresh original input survives private publication",
                "NEW AUTHORIZED HUMAN INPUT" in str(normal)
                and "own private Work checkpoint" in str(normal),
            )
            wid = o.rows("SELECT id FROM work_nodes")[0][0]
            ctx.equal(
                "checkpoint is bound to original Work owner",
                o.rows("SELECT work,state FROM exec_compacts"),
                [(wid, "published")],
            )
            # This normal response observes the previous pair, then proposes a real Work wait.
            wait_args = {
                "action": "wait",
                "wait_mode": "any",
                "conditions": [{"kind": "conversation"}],
            }
            reply(q, calls=[call("wait", "task_control", wait_args)])
            o.until(lambda: o.rows("SELECT id FROM work_waits WHERE state='armed'"))
            next_event = o.human("wake the original Work")
            resumed, q = o.wire(http)
            ctx.check(
                "new Work activation carries the private checkpoint, without a previously observed pair",
                "own private Work checkpoint" in str(resumed)
                and "EXPLICIT PRIVATE WORK GOAL" in str(resumed)
                and "work_unobserved_tool_round" not in str(resumed),
            )
            reply(q, calls=[call("close", "complete", {"summary": "done"})])
            o.until(
                lambda: o.rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'", wid
                )
            )
            # A separate neutral input cannot inherit the previous Work's private summary.
            outsider = o.human("independent neutral input")
            peer, q = o.wire(http)
            if peer["messages"][-1]["content"].startswith("[Host 会话上下文压缩要求]"):
                ctx.check(
                    "P never gets a prior Work checkpoint or goal",
                    "own private Work checkpoint" not in str(peer)
                    and "EXPLICIT PRIVATE WORK GOAL" not in str(peer),
                )
                reply(q, "public context")
                peer, q = o.wire(http)
            ctx.check(
                "neutral activation does not resurrect former private state",
                "no_active_work" in str(peer)
                and "own private Work checkpoint" not in str(peer)
                and "EXPLICIT PRIVATE WORK GOAL" not in str(peer),
            )
            reply(q, "done")
            o.finished(outsider)
            return {
                "work": wid,
                "source": event,
                "append": fresh,
                "wake": next_event,
                "normal": sha(normal),
                "resumed": sha(resumed),
            }


def run(ctx):
    gold = json.loads((ctx.root / "fixtures/compact-exec/v4-rounds.json").read_bytes())
    ctx.check(
        "original integration oracle covers actual delivery and native E to P",
        any("unknown" in k for k in gold["original_cases"])
        and any("native_execution" in k for k in gold["original_cases"]),
    )
    return {
        "cases": {
            "H07": {state: round_case(ctx, state) for state in ["confirmed", "unknown"]}
        },
        "oracle": gold["source_commit"],
        "native_E_to_P": native_handoff(ctx),
        "partial_delivery": actual_partial(ctx),
        "faults": faults(ctx),
        "bound_work": work_scope(ctx),
        "public_to_E": [public_handoff(ctx, reject) for reject in [False, True]],
    }
