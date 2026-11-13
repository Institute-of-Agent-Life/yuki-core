"""Original reply target, paid mutation, erasure and cold evidence through the owner."""

import hashlib
import json
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def run(ctx):
    oracle_path = ctx.root / "fixtures/memory/v4-reply-author.json"
    oracle = json.loads(oracle_path.read_bytes())["projection"]
    o = Owner(ctx, "reply-author", 54)
    receipts = {}
    people = {o.actor: "1001"}
    target = {"scope_type": "person_group", "subject_ref": "replied_message_author"}

    def send(tag, person=None, group="3001", refs=(), native=None, accepted=True, occurred=None):
        o.seq += 1
        frame = dict(source="reply-" + tag, account=(person or o.user)["account"],
            bot_stage=o.bot["assignment"], reply_bot="100", kind="message",
            occurred=o.seq * 100 if occurred is None else occurred, observed=o.seq * 100,
            segments=[{"type":"text", "text":"他喜欢摄影"}] +
                [{"type":"reply", "native_id":ref} for ref in refs])
        if group:
            frame["space"] = group
        if native:
            frame["native_id"] = native
        return o.op("ingress", "record", json.dumps(frame, ensure_ascii=False), *(["--accept"] if accepted else []))["event"]

    def finish(event):
        reply(q, "NO_REPLY")
        o.finished(event)

    def invoke(tag, args, expected="confirmed", name="memory_change"):
        nonlocal q
        reply(q, calls=[call(tag, name, args)])
        wire, q = o.wire(model)
        value = json.loads(next(m["content"] for m in wire["messages"] if m.get("tool_call_id") == tag))
        receipts[tag] = value
        ctx.write("reply-author-receipts.json", receipts)
        ctx.equal(tag + " decision: " + str(value["result"].get("error", "")), value["state"], expected)
        return value

    def args(tag, **extra):
        return dict(operation="create", target=target, new_content="喜欢摄影",
            memory_key="reply:" + tag, category="hobby") | extra

    def snapshot(number):
        person, space, scope, authority, body = o.rows(
            "SELECT f.person,f.space,f.kind,a.authority,v.body FROM memory_facts f JOIN memory_attributions a ON a.fact=f.id JOIN memory_versions v ON v.fact=f.id AND v.revision=f.revision WHERE f.number=?", number)[0]
        return dict(subject=people[person], group=space, scope=scope, authority=authority, content=body,
            evidence=[dict(author=people[a], quote=b, authority=c) for a,b,c in o.rows(
                "SELECT e.author,e.quote,e.attribution FROM memory_evidence e JOIN memory_facts f ON f.id=e.fact WHERE f.number=? ORDER BY e.id", number)])

    with Recorder() as model:
        o.config(model)
        with o.svc.running():
            other_id = "00000000-0000-4000-8000-000000000452"
            people[other_id] = "2002"
            o.op("person", "create", other_id)
            other = o.op("account", "open", "qq", "compact", "202", 1, "--person", other_id, "--start", 1)
            original = send("original", other, native="original")
            _, q = o.wire(model)
            finish(original)
            own = send("own", native="own")
            _, q = o.wire(model)
            created = invoke("origin", dict(args("origin"), target=dict(target, subject_ref="current_speaker")))
            finish(own)
            for tag, locator in [("direct", "original"), ("self_reply", "own")]:
                event = send(tag, refs=[locator])
                wire, q = o.wire(model)
                declaration = next(t["function"] for t in wire["tools"] if t["function"]["name"] == "memory_change")
                result = invoke(tag, args(tag))
                ctx.check("public replied_message_author declared", "replied_message_author" in declaration["parameters"]["properties"]["target"]["properties"]["subject_ref"]["anyOf"][0]["enum"])
                number = result["result"]["fact_id"]
                want = {k:v for k,v in oracle[tag].items() if k not in ("ok", "reason")}
                ctx.equal(tag + " matches fixed v4 attribution", snapshot(number), want)
                if tag == "direct":
                    direct_number, direct_event, direct = number, event, result
                    sibling = invoke("same-trigger-self", dict(args("same-trigger-self"), target=dict(target, subject_ref="current_speaker")))
                    sibling_number = sibling["result"]["fact_id"]
                    ctx.equal("same event evidence keeps each fact origin", o.rows("SELECT e.origin_execution IS NOT NULL FROM memory_evidence e JOIN memory_facts f ON f.id=e.fact WHERE f.number IN(?,?) ORDER BY f.number", number, sibling_number), [(1,), (0,)])
                finish(event)
            event = send("self_reassign", refs=["own"])
            _, q = o.wire(model)
            invoke("read-self-origin", {"fact_id":created["result"]["fact_id"], "view":"fact"}, name="read_memory")
            ctx.equal("v4 rejects self reassignment", oracle["self_reassigned"]["ok"], False)
            invoke("self_reassign", dict(operation="reassign", target=target, fact_id=created["result"]["fact_id"], reason="misattributed_subject"), "failed")
            finish(event)
            event = send("reassign", refs=["original"])
            _, q = o.wire(model)
            invoke("read-origin", {"fact_id":created["result"]["fact_id"], "view":"fact"}, name="read_memory")
            moved = invoke("reassign", dict(operation="reassign", target=target, fact_id=created["result"]["fact_id"], reason="misattributed_subject"))
            ctx.equal("reassign matches fixed v4 attribution", snapshot(moved["result"]["fact_id"]),
                {k:v for k,v in oracle["reassigned"].items() if k not in ("ok", "reason")})
            finish(event)
            event = send("future_target", refs=["future-target"])
            _, q = o.wire(model)
            late = send("late_target", other, native="future-target", accepted=False, occurred=1)
            ctx.check("earlier occurrence does not change later admission", late > event)
            invoke("future_target", args("future_target"), "failed")
            finish(event)
            stable = send("stable_target", other, native="stable-target")
            _, q = o.wire(model)
            finish(stable)
            event = send("future_collision", refs=["stable-target"])
            _, q = o.wire(model)
            send("late_collision", native="stable-target", accepted=False, occurred=1)
            stable_result = invoke("future_collision", args("future_collision"))
            ctx.equal("future native collision does not retarget a prior original", snapshot(stable_result["result"]["fact_id"])["subject"], "2002")
            finish(event)
            foreign = send("foreign", other, group="3002", native="foreign")
            _, q = o.wire(model)
            finish(foreign)
            for tag, refs, group, extras in [
                ("missing", [], "3001", {}),
                ("unknown", ["unknown"], "3001", {}),
                ("cross_scope", ["foreign"], "3001", {}),
                ("private", ["original"], None, {}),
                ("ambiguous_segments", ["original", "own"], "3001", {}),
                ("foreign_evidence", ["original"], "3001", {"evidence_event_id":original}),
                ("wrong_scope", ["original"], "3001", {"target":dict(target, scope_type="person")}),
                ("internal_alias", ["original"], "3001", {"target":dict(target, subject_ref="reply_author")}),
            ]:
                event = send(tag, refs=refs, group=group)
                _, q = o.wire(model)
                before = o.rows("SELECT (SELECT count(*) FROM memory_facts),(SELECT count(*) FROM memory_evidence),(SELECT count(*) FROM memory_versions)")
                invoke(tag, args(tag, **extras), "failed")
                ctx.equal(tag + " leaves no partial fact", o.rows("SELECT (SELECT count(*) FROM memory_facts),(SELECT count(*) FROM memory_evidence),(SELECT count(*) FROM memory_versions)"), before)
                finish(event)
            o.op("person", "set-enabled", other_id, "false")
            event = send("disabled_author", refs=["original"])
            _, q = o.wire(model)
            invoke("disabled_author", args("disabled_author"), "failed")
            finish(event)
            o.op("person", "set-enabled", other_id, "true")
            duplicate = send("duplicate", other, native="duplicate")
            _, q = o.wire(model)
            finish(duplicate)
            duplicate2 = send("duplicate2", native="duplicate")
            _, q = o.wire(model)
            finish(duplicate2)
            event = send("ambiguous_locator", refs=["duplicate"])
            _, q = o.wire(model)
            invoke("ambiguous_locator", args("ambiguous_locator"), "failed")
            finish(event)
            o.op("privacy", "erase", "erase-one-collision", other_id, json.dumps([duplicate]))
            event = send("erased_collision", refs=["duplicate"])
            _, q = o.wire(model)
            invoke("erased_collision", args("erased_collision"), "failed")
            finish(event)
        with o.svc.running():
            execution = o.rows("SELECT id FROM executions WHERE source=?", direct_event)[0][0]
            ctx.equal("cold original result readable", o.op("effect", "read", execution, direct["id"], o.actor)["result"], direct["result"])
            ctx.equal("cold reference retained as dependency", o.rows("SELECT event FROM main_dependencies WHERE execution=? AND event=?", execution, original), [(original,)])
            o.svc.command("operator", "effect", "read", execution, direct["id"], other_id, accepted=False)
            event = send("cold_consumer")
            _, q = o.wire(model)
            consumed = invoke("cold-read", {"fact_id":direct_number, "view":"fact"}, name="read_memory")
            consumer = o.rows("SELECT id FROM executions WHERE source=?", event)[0][0]
            ctx.equal("a later fact read retains the original reply dependency", o.rows("SELECT event FROM main_dependencies WHERE execution=? AND event=?", consumer, original), [(original,)])
            finish(event)
            o.op("privacy", "erase", "forget-reply-target", other_id, json.dumps([original]))
            ctx.equal("reference erasure clears derived body and assertion quote", o.rows("SELECT f.status,v.body,e.quote FROM memory_facts f JOIN memory_versions v ON v.fact=f.id JOIN memory_evidence e ON e.fact=f.id WHERE f.number=?", direct_number), [("erased", None, None)])
            o.svc.command("operator", "effect", "read", consumer, consumed["id"], o.actor, accepted=False)
            o.svc.command("operator", "memory", "read", consumer, o.actor, "--fact", direct["result"]["fact"], "--evidence", accepted=False)
            ctx.equal("execution-level erasure includes the same-trigger sibling", o.rows("SELECT status FROM memory_facts WHERE number=?", sibling_number), [("erased",)])
            event = send("erased", refs=["original"])
            _, q = o.wire(model)
            invoke("erased", args("erased"), "failed")
            finish(event)
            event = send("retiring", other, native="retiring")
            _, q = o.wire(model)
            finish(event)
            o.op("account", "end", other["assignment"], o.seq * 100 + 1)
            event = send("retired_author", refs=["retiring"])
            _, q = o.wire(model)
            invoke("retired_author", args("retired_author"), "failed")
            finish(event)
    return dict(oracle_sha256=hashlib.sha256(oracle_path.read_bytes()).hexdigest(), receipts=receipts)
