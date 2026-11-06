"""Recover the original paid chain across a segment yield, without resetting its root."""

import json
import time
import uuid
from urllib.request import Request, urlopen
from concurrent.futures import ThreadPoolExecutor

from tests.harness.compact import Owner, call, reply
from tests.harness.crash import kill_writer
from tests.peers.chat.recorder import Recorder


def run(ctx):
    proofs = []
    for index, phase in enumerate(
        ("paired-before-normal-observation", "work-segment-yielded")
    ):
        o = Owner(ctx, "segment-cut-" + str(index), 25)
        with o.svc.running():
            original = o.human("preserve this Work across the original segment")
        with Recorder() as peer:
            o.config(peer)
            config = o.svc.data / "yuki.toml"
            base = config.read_text()

            def policy(limit):
                config.write_text(
                    base
                    + f"\n[work_segments]\nmodel_requests={limit}\nresume_delay_ms=2000\n"
                )

            policy(1)
            with ThreadPoolExecutor(max_workers=1) as pool:
                death = pool.submit(
                    kill_writer,
                    ctx,
                    o.svc.data,
                    "work_delivery_crash_child",
                    phase,
                    {},
                )
                _, q = o.wire(peer)
                reply(q, calls=[call("accept", "task_control", {"action": "accept", "goal": "original segment Work"})])
                death.result()
            work, exec = o.rows("SELECT id,owner_execution FROM work_nodes")[0]
            original_requests = o.rows("SELECT id,activation,root FROM model_requests ORDER BY rowid")
            ctx.equal("first segment reserves original root once", o.rows("SELECT reserved_requests FROM root_budgets"), [(1,)])
            policy(2)
            with o.svc.running():
                body, q = o.wire(peer)
                ctx.check("cold original accept receipt survives", any(m.get("tool_call_id") == "accept" for m in body["messages"]))
                reply(q, calls=[call("once", "send", {"text": "one segment delivery"})])
                _, q = o.wire(peer)
                reply(q, calls=[call("read", "find_contacts", {"kind": "person", "subject_ref": "current_speaker"})])
                o.until(lambda: o.rows("SELECT id FROM work_nodes WHERE id=? AND status='queued'", work))
                ctx.equal("segment exhausted after full original tools", o.rows("SELECT count(*) FROM model_tool_intents WHERE paired_receipt IS NULL"), [(0,)])
                ctx.equal("one physical delivery across segment restart", len(peer.deliveries), 1)
                ctx.equal("new segment limit applies without refreshing root", o.rows("SELECT request_limit,request_count FROM main_segments ORDER BY ordinal"), [(1, 1), (2, 2)])
                ctx.equal("queued original execution keeps three root reservations", o.rows("SELECT reserved_requests FROM root_budgets"), [(3,)])
            policy(24)
            with o.svc.running():
                body, q = o.wire(peer)
                read = json.loads(next(m["content"] for m in body["messages"] if m.get("tool_call_id") == "read"))
                ctx.equal("cold third segment sees exact original contact", read["result"]["target_id"], o.actor)
                reply(q, calls=[call("finish", "task_control", {"action": "complete", "result": "original task finished"})])
                o.finished(original)
                ctx.equal("terminal proposal settles before another request", o.rows("SELECT reserved_requests FROM root_budgets"), [(4,)])
                ctx.equal("same Work and original execution complete", o.rows("SELECT id,owner_execution,status FROM work_nodes"), [(work, exec, "completed")])
                ctx.equal("first paid identity remains exact", o.rows("SELECT id,activation,root FROM model_requests ORDER BY rowid LIMIT 1"), original_requests)
                ctx.equal("cold segments never duplicate real send", len(peer.deliveries), 1)
                proofs.append({"cut": phase, "work": work, "execution": exec, "requests": o.rows("SELECT id,activation,root FROM model_requests ORDER BY rowid"), "segments": o.rows("SELECT ordinal,request_limit,state,request_count FROM main_segments ORDER BY ordinal"), "deliveries": peer.deliveries})

    for failure in ("unknown", "root_exhausted"):
        o = Owner(ctx, "segment-" + failure, 25)
        with o.svc.running():
            event = o.human("retain original failure at segment boundary")
        with Recorder() as peer:
            o.config(peer)
            config = o.svc.data / "yuki.toml"
            config.write_text(config.read_text().replace("root_slots=8", "root_slots=2") + "\n[work_segments]\nmodel_requests=1\nresume_delay_ms=1\n")
            with o.svc.running():
                _, q = o.wire(peer)
                reply(q, calls=[call("accept", "task_control", {"action": "accept", "goal": "keep root"})])
                _, q = o.wire(peer)
                if failure == "unknown":
                    q.put((b'{"choices":[', 10))
                else:
                    reply(q, calls=[call("read", "find_contacts", {"kind": "person", "subject_ref": "current_speaker"})])
                o.until(lambda: o.rows("SELECT id FROM executions WHERE source=? AND state='held'", event))
                before = o.rows("SELECT id,state FROM model_attempts ORDER BY rowid")
                ctx.equal("two original reservations at failure", o.rows("SELECT reserved_requests FROM root_budgets"), [(2,)])
            with o.svc.running():
                time.sleep(0.1)
                ctx.equal("cold failure keeps original attempts", o.rows("SELECT id,state FROM model_attempts ORDER BY rowid"), before)
                ctx.check("no unknown or root exhaustion repurchase", peer.wires.empty())
                proofs.append({"failure": failure, "attempts": before, "root": o.rows("SELECT max_requests,reserved_requests FROM root_budgets")})
    o = Owner(ctx, "cancel-queued-segment", 25)
    with Recorder() as peer:
        o.config(peer)
        config = o.svc.data / "yuki.toml"
        config.write_text(config.read_text() + "\n[work_segments]\nmodel_requests=1\nresume_delay_ms=60000\n")
        with o.svc.running():
            code = o.op("personal", "issue", o.actor, str(uuid.uuid4()), "chat")["code"]
            def post(path, body, token=None):
                headers = {"Content-Type": "application/json"}
                if token:
                    headers["Authorization"] = "Bearer " + token
                with urlopen(Request("http://" + o.svc.address + path, data=json.dumps(body).encode(), headers=headers), timeout=5) as response:
                    return json.load(response)["data"]
            token = post("/client/v1/pair", {"code": code, "device_label": "segment cancellation"})["token"]
            o.human("cancel this accepted queued Work")
            _, q = o.wire(peer)
            reply(q, calls=[call("accept", "task_control", {"action": "accept", "goal": "cancel at original boundary"})])
            work = o.until(lambda: o.rows("SELECT id FROM work_nodes WHERE status='queued'"))[0][0]
            with urlopen(Request("http://" + o.svc.address + "/client/v1/works/" + work, headers={"Authorization": "Bearer " + token}), timeout=5) as response:
                revision = json.load(response)["data"]["revision"]
            post("/client/v1/works/" + work + "/cancel", {"request_id": str(uuid.uuid4()), "expected_revision": revision}, token)
            ctx.equal("authorized cancellation closes queued original", o.rows("SELECT status FROM work_nodes WHERE id=?", work), [("cancelled",)])
        config.write_text(config.read_text().replace("resume_delay_ms=60000", "resume_delay_ms=1"))
        with o.svc.running():
            time.sleep(0.1)
            ctx.equal("cold cancelled segment cannot purchase again", o.rows("SELECT reserved_requests FROM root_budgets"), [(1,)])
            ctx.check("cancelled queued owner produces no model wire", peer.wires.empty())
        proofs.append({"cancelled_work": work, "requests": 1})
    return {"original_sha": "329ca7c487414d2f54ddd98ff4727446ed2446cd", "cuts": proofs}
