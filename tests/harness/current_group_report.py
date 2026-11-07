"""Original event -> paid create -> persisted group claim and scoped reader."""

import hashlib
import json
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def run(ctx):
    path = ctx.root / "fixtures/memory/v4-current-group-report.json"
    oracle = json.loads(path.read_bytes())
    owner = Owner(ctx, "current-group", 54)
    results = {}
    numbers = []
    original_events = []
    first_effect = None
    with Recorder() as model:
        owner.config(model)
        with owner.svc.running():
            for name, group, ref, scope in [
                ("group", "3001", "current_group", "group"),
                ("same", "3001", "current_group", "group"),
                ("conflict", "3001", "current_group", "group"),
                ("private", None, "current_group", "group"),
                ("wrong_scope", "3001", "current_group", "person"),
                ("speaker_as_group", "3001", "current_speaker", "group"),
            ]:
                text = (
                    "本群每周三讨论音乐" if name == "conflict" else "本群每周三讨论摄影"
                )
                event = owner.human(text, group)
                _, q = owner.wire(model)
                args = {
                    "operation": "create",
                    "target": {"scope_type": scope, "subject_ref": ref},
                    "new_content": text,
                    "memory_key": "topic:group",
                    "category": "group_activity",
                    "evidence_quote": text,
                }
                reply(q, calls=[call("record", "memory_change", args)])
                wire, q = owner.wire(model)
                answer = json.loads(
                    next(
                        m["content"]
                        for m in wire["messages"]
                        if m.get("tool_call_id") == "record"
                    )
                )
                want = oracle["cases"][name]
                ctx.equal(
                    name + " original mutation decision",
                    answer["state"] == "confirmed",
                    want["ok"],
                )
                if want["ok"]:
                    number = answer["result"]["fact_id"]
                    numbers.append(number)
                    original_events.append(event)
                    if first_effect is None:
                        first_effect = (
                            owner.rows(
                                "SELECT id FROM executions WHERE source=?", event
                            )[0][0],
                            answer,
                        )
                    reply(
                        q,
                        calls=[
                            call(
                                "read",
                                "read_memory",
                                {"fact_id": number, "view": "fact"},
                            )
                        ],
                    )
                    wire, q = owner.wire(model)
                    fact = json.loads(
                        next(
                            m["content"]
                            for m in wire["messages"]
                            if m.get("tool_call_id") == "read"
                        )
                    )["result"]
                    evidence = owner.rows(
                        "SELECT e.relation,e.event,e.author FROM memory_evidence e JOIN memory_facts f ON f.id=e.fact WHERE f.number=?",
                        number,
                    )
                    ctx.equal(
                        name + " binds original event and human author",
                        evidence,
                        [("group_statement", event, owner.actor)],
                    )
                    got = {
                        "ok": True,
                        "operation": "create",
                        "reason": answer["result"]["reason_code"],
                        "actual_scope": fact["kind"],
                        "person_bound": fact["person"] is not None,
                        "space_bound": fact["space"] is not None,
                        "authority": fact["authority"],
                        "source_type": fact["source_type"],
                        "evidence": [evidence[0][0]],
                    }
                    ctx.equal(
                        name + " matches fixed v4 claim and provenance", got, want
                    )
                    results[name] = got
                else:
                    results[name] = {
                        "ok": False,
                        "rust_error": answer["result"].get("error"),
                        "original_reason": want["reason"],
                    }
                reply(q, "")
                owner.finished(event)
            ctx.equal(
                "same-key statements create independent facts", len(set(numbers)), 3
            )
            ctx.equal(
                "refused calls create no facts",
                owner.rows("SELECT count(*) FROM memory_facts"),
                [(3,)],
            )
            event = owner.human("另一群的消息", "4001")
            _, q = owner.wire(model)
            forged = dict(
                args,
                target={"scope_type": "group", "subject_ref": "current_group"},
                evidence_event_id=original_events[0],
            )
            reply(q, calls=[call("foreign-source", "memory_change", forged)])
            wire, q = owner.wire(model)
            foreign = json.loads(
                next(
                    m["content"]
                    for m in wire["messages"]
                    if m.get("tool_call_id") == "foreign-source"
                )
            )
            ctx.equal(
                "a source from another group cannot anchor the claim",
                foreign["state"],
                "failed",
            )
            reply(q, "")
            owner.finished(event)
        with owner.svc.running():
            ctx.equal(
                "source, relation and group scope survive cold open",
                owner.rows(
                    "SELECT count(*) FROM memory_evidence e JOIN memory_facts f ON f.id=e.fact WHERE f.kind='group' AND e.relation='group_statement' AND f.person IS NULL AND e.quote IS NOT NULL"
                ),
                [(3,)],
            )
            execution, original = first_effect
            cold = owner.op("effect", "read", execution, original["id"], owner.actor)
            ctx.equal(
                "original group receipt is readable after restart",
                cold["result"],
                original["result"],
            )
            owner.op(
                "privacy",
                "erase",
                "forget-group-statement",
                owner.actor,
                json.dumps([original_events[0]]),
            )
            ctx.equal(
                "forgetting the statement clears its body and quote",
                owner.rows(
                    "SELECT f.status,v.body,e.quote FROM memory_facts f JOIN memory_versions v ON v.fact=f.id JOIN memory_evidence e ON e.fact=f.id WHERE f.number=?",
                    numbers[0],
                ),
                [("erased", None, None)],
            )
    return {
        "oracle_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "source_commit": oracle["source_commit"],
        "cases": results,
    }
