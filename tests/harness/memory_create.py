"""Replay v4 create arguments through the recorded model and the native writer."""
import hashlib
import json

from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def run(ctx):
    fixture = (ctx.root / "fixtures/memory/v4-create-arguments.json").read_bytes()
    oracle = json.loads(fixture)
    owner = Owner(ctx, "create-arguments", 44)
    results = []
    with Recorder() as model:
        owner.config(model)
        with owner.svc.running():
            for case in oracle["cases"]:
                key = case["case"]
                args = case["arguments"]
                before = owner.rows("SELECT COUNT(*) FROM memory_facts")[0][0]
                event = owner.human("记住我" + args.get("new_content", "喜欢爵士乐"))
                _, queue = owner.wire(model)
                reply(queue, calls=[call(key, "memory_change", args)])
                wire, queue = owner.wire(model)
                receipt = json.loads(next(
                    m["content"] for m in wire["messages"] if m.get("tool_call_id") == key
                ))
                after = owner.rows("SELECT COUNT(*) FROM memory_facts")[0][0]
                results.append({"case": key, "receipt": receipt, "before": before, "after": after})
                reply(queue, "done")
                owner.finished(event)
        with owner.svc.running():
            facts = owner.rows("SELECT kind,person,visibility FROM memory_facts ORDER BY number")
            ctx.write("create-arguments-replay.json", {
                "oracle_sha256": hashlib.sha256(fixture).hexdigest(), "results": results, "facts": facts
            })
            for case, row in zip(oracle["cases"], results, strict=True):
                label = case["case"]
                ctx.equal(label + " fact count follows original", row["after"] - row["before"], case["created"])
                receipt = row["receipt"]
                ctx.equal(label + " outcome", receipt["state"], "confirmed" if case["created"] else "failed")
                if not case["created"]:
                    ctx.equal(label + " original rejection", receipt["result"]["error"], case["reason"])
            ctx.equal("visibility hints keep both facts with the original Person",
                      facts, [("person", owner.actor, "subject")] * 2)
