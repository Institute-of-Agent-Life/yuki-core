"""Inspect only the public SDK wire plus original owner effects. No Manager is launched."""

import hashlib, json, sqlite3, sys
from contextlib import closing
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


class Peer:
    def __init__(self, o, folder):
        self.o = o
        self.folder = folder
        folder.mkdir()
        self.seq = 10

    def post(self, body):
        self.seq += 1
        f = self.folder / ("cmd-" + str(self.seq) + ".json")
        tmp = f.with_suffix(".tmp")
        tmp.write_text(json.dumps(body), encoding="utf-8")
        tmp.replace(f)

    def rpc(self, method, params, ok=True):
        id = self.seq + 1
        self.post({"id": id, "method": method, "params": params})
        file = self.folder / ("out-" + str(id) + ".json")
        self.o.until(file.is_file)
        r = json.loads(file.read_bytes())
        self.o.ctx.equal(
            method + " public RPC " + ("granted" if ok else "denied"), "result" in r, ok
        )
        return r.get("result")

    def answer(self, f, result):
        self.post({"id": f["id"], "reply": {"ok": True, "data": result}})


def exercise(ctx, sdk, profile="debug"):
    o = Owner(ctx, "terminal-work", 50, profile)
    peers = {n: Peer(o, ctx.working / n) for n in ["manager", "foreign"]}
    p = peers["manager"]
    source = ctx.root / "tests/peers/terminal/consumer.py"
    cfg = ""
    for name, peer in peers.items():
        cfg += (
            f'\n[[plugins]]\nid="{name}"\nnamespace="{name}"\nprogram='
            + json.dumps(sys.executable)
            + "\nargs="
            + json.dumps([str(source), str(peer.folder), str(sdk)])
            + '\ngrants=["tool.call","terminal.work","forget.owner"]\n'
        )
        if name == "manager":
            for tool, required in [
                ("terminal_exec", ["command"]),
                ("terminal_read", ["run_id"]),
                ("terminal_control", ["run_id", "action"]),
            ]:
                props = ",".join(k + '={type="string"}' for k in required)
                cfg += (
                    f'[[plugins.tools]]\nname="{tool}"\ndescription="Original {tool}"\ncacheable=false\nparallel_read=false\ncode_direct=false\nschema={{type="object",required='
                    + json.dumps(required)
                    + ",properties={"
                    + props
                    + "},additionalProperties=false}\n"
                )
    seen = set()

    def take():
        def new():
            return next(
                (f for f in sorted(p.folder.glob("call-*.json")) if f.name not in seen),
                None,
            )

        a = o.until(new)
        seen.add(a.name)
        return json.loads(a.read_bytes())

    def accepted(q):
        reply(
            q,
            calls=[
                call(
                    "accept",
                    "task_control",
                    {"action": "accept", "goal": "run the original job"},
                )
            ],
        )
        wire, q = o.wire(model)
        value = json.loads(
            next(
                m["content"]
                for m in wire["messages"]
                if m.get("tool_call_id") == "accept"
            )
        )
        return value["result"]["work_id"], q

    def edit(stmt, *args):
        with closing(sqlite3.connect(o.svc.data / "core.sqlite")) as db:
            db.execute(stmt, args)
            db.commit()

    with Recorder() as model:
        o.config(model, cfg)
        with o.svc.running():
            for peer in peers.values():
                o.until(lambda: (peer.folder / "ready.json").is_file())
            ev = o.human("start own shell")
            _, q = o.wire(model)
            w, q = accepted(q)
            reply(
                q,
                calls=[call("start", "terminal_exec", {"command": "bounded fixture"})],
            )
            launch = take()
            h = launch["params"]["handle"]
            lp = {"handle": h}
            s = p.rpc("terminal.scope", lp)
            ctx.equal(
                "Host fixes original Work, installation and launch key",
                (
                    s["work_id"],
                    s["mode"],
                    s["installation"],
                    s["request_id"],
                    s["launch_request"],
                ),
                (w, "launch", "manager", h, h),
            )
            ctx.equal(
                "scope execution comes from paid Host invocation",
                o.rows("SELECT execution,operation FROM host_calls WHERE handle=?", h),
                [(s["execution"], s["operation"])],
            )
            peers["foreign"].rpc("terminal.scope", lp, False)
            receipt = {"handle": h, "request_id": h, "run_id": "manager-original-run"}
            p.rpc("terminal.bind", {**receipt, "request_id": "guessed"}, False)
            bound = p.rpc("terminal.bind", receipt)
            ctx.equal(
                "idempotent same Manager run receipt",
                p.rpc("terminal.bind", receipt),
                bound,
            )
            p.rpc("terminal.bind", {**receipt, "run_id": "other-run"}, False)
            ctx.equal(
                "recorded original run can only reconcile",
                p.rpc("terminal.scope", lp)["mode"],
                "reconcile",
            )
            p.answer(launch, {"run_id": receipt["run_id"], "pending": True})
            _, q = o.wire(model)
            ctx.equal(
                "generic status discloses no confirmed Manager payload",
                p.rpc("tool.status", lp),
                {"handle": h, "state": "confirmed", "result": None},
            )
            for name, args, mode in [
                ("terminal_read", {"run_id": receipt["run_id"]}, "read"),
                (
                    "terminal_control",
                    {"run_id": receipt["run_id"], "action": "input"},
                    "input",
                ),
            ]:
                reply(q, calls=[call("step-" + mode, name, args)])
                f = take()
                scope = p.rpc("terminal.scope", {"handle": f["params"]["handle"]})
                ctx.equal(
                    "read or input targets only the original run",
                    (
                        scope["launch_request"],
                        scope["run_id"],
                        scope["work_id"],
                        scope["mode"],
                    ),
                    (h, receipt["run_id"], w, mode),
                )
                p.rpc(
                    "terminal.bind", {**receipt, "handle": f["params"]["handle"]}, False
                )
                p.answer(f, {"pending": True})
                _, q = o.wire(model)
            reply(
                q,
                calls=[call("observe", "terminal_read", {"run_id": receipt["run_id"]})],
            )
            observation = take()
            oh = observation["params"]["handle"]
            ctx.equal(
                "active observation has no mutation authority",
                p.rpc("terminal.scope", {"handle": oh})["mode"],
                "read",
            )
            old = json.loads((p.folder / "ready.json").read_bytes())["pid"]
            o.op("host", "enable", "manager", "false")
            o.until(
                lambda: o.rows("SELECT state FROM host_calls WHERE handle=?", oh)
                == [("unknown",)]
            )
            ctx.equal(
                "interrupting the observation leaves the original launch and run unchanged",
                o.rows(
                    "SELECT state,run FROM host_calls JOIN terminal_work_calls USING(handle) WHERE handle=?",
                    h,
                ),
                [("confirmed", receipt["run_id"])],
            )
            o.op("host", "enable", "manager", "true")
            o.until(
                lambda: json.loads((p.folder / "ready.json").read_bytes())["pid"] != old
            )
            p.rpc("terminal.scope", {"handle": oh}, False)
            _, q = o.wire(model)
            reply(q, "")
            o.finished(ev)
            ev2 = o.human("independent Work")
            _, q = o.wire(model)
            other, q = accepted(q)
            ctx.check("distinct Work accepted from new real ingress", other != w)
            reply(
                q, calls=[call("guess", "terminal_read", {"run_id": receipt["run_id"]})]
            )
            wire, q = o.wire(model)
            ctx.check(
                "cross Work guess produced no physical call",
                len(list(p.folder.glob("call-*.json"))) == 4,
            )
            ctx.check(
                "cross Work refusal reaches its own caller",
                "manager-original-run"
                not in next(
                    m["content"]
                    for m in wire["messages"]
                    if m.get("tool_call_id") == "guess"
                ),
            )
            reply(
                q,
                calls=[call("second", "terminal_exec", {"command": "second fixture"})],
            )
            second = take()
            h2 = second["params"]["handle"]
            s2 = p.rpc("terminal.scope", {"handle": h2})
            edit("UPDATE executions SET work_id=? WHERE id=?", w, s2["execution"])
            p.rpc("terminal.scope", {"handle": h2}, False)
            edit("UPDATE executions SET work_id=? WHERE id=?", other, s2["execution"])
            # Stop the plugin after the physical call, before recording a run. The Manager is independent.
            old = json.loads((p.folder / "ready.json").read_bytes())["pid"]
            o.op("host", "enable", "manager", "false")
            o.until(
                lambda: o.rows("SELECT state FROM host_calls WHERE handle=?", h2)
                == [("unknown",)]
            )
            o.op("host", "enable", "manager", "true")
            o.until(
                lambda: (p.folder / "ready.json").is_file()
                and json.loads((p.folder / "ready.json").read_bytes())["pid"] != old
            )
            rec = p.rpc("terminal.scope", {"handle": h2})
            ctx.equal(
                "new generation recovers original key, without authority to submit",
                (rec["mode"], rec["work_id"], rec["request_id"]),
                ("reconcile", other, h2),
            )
            p.rpc(
                "terminal.bind",
                {"handle": h2, "request_id": h2, "run_id": "recovered-manager-run"},
            )
            ctx.equal(
                "recovered run does not redispatch unknown",
                o.rows(
                    "SELECT state,run FROM host_calls JOIN terminal_work_calls USING(handle) WHERE handle=?",
                    h2,
                ),
                [("unknown", "recovered-manager-run")],
            )
            original = o.rows(
                "SELECT value FROM core_meta WHERE key='plugin-approval:manager'"
            )[0][0]
            edit(
                "UPDATE core_meta SET value=json_set(value,'$.deployment','replacement') WHERE key='plugin-approval:manager'"
            )
            p.rpc("tool.status", {"handle": h2}, False)
            p.rpc("terminal.scope", {"handle": h2}, False)
            edit(
                "UPDATE core_meta SET value=? WHERE key='plugin-approval:manager'",
                original,
            )
            ctx.equal(
                "restored original deployment has a recovery receipt",
                p.rpc("tool.status", {"handle": h2})["state"],
                "unknown",
            )
            o.op(
                "privacy", "erase", "terminal-source-forget", o.actor, json.dumps([ev2])
            )
            ctx.equal(
                "source erasure removes Manager reference",
                o.rows("SELECT run FROM terminal_work_calls WHERE handle=?", h2),
                [(None,)],
            )
            p.rpc("terminal.scope", {"handle": h2}, False)
            edit(
                "UPDATE core_meta SET value=json_set(value,'$.deployment','replacement') WHERE key='plugin-approval:manager'"
            )
            p.rpc("terminal.scope", lp, False)
            p.rpc("tool.status", lp, False)
            p.rpc("terminal.scope", {"handle": h2}, False)
            ctx.equal(
                "observation did not cancel or interrupt the actual target",
                o.rows(
                    "SELECT count(*) FROM terminal_work_calls t JOIN host_calls c USING(handle) WHERE t.tool='terminal_control' AND json_extract(c.args,'$.action')!='input'"
                ),
                [(0,)],
            )
            ctx.equal(
                "one physical launch per original request",
                o.rows(
                    "SELECT count(*) FROM terminal_work_calls WHERE tool='terminal_exec'"
                ),
                [(2,)],
            )
    result = {
        "methods": ["terminal.scope", "terminal.bind"],
        "work": w,
        "independent_work": other,
        "manager_request": h,
        "recovered_request": h2,
        "cross_work_dispatches": 0,
        "physical_launches": 2,
        "recovered_effect": "unknown",
        "interrupted_observation": {
            "state": "unknown",
            "original_launch": "confirmed",
            "extra_control_calls": 0,
        },
        "manager_pty_tested": False,
        "original_oracle_sha256": hashlib.sha256(
            (ctx.root / "fixtures/host/terminal-work-v4.json").read_bytes()
        ).hexdigest(),
    }
    ctx.write("terminal-work-scope.json", result)
    return result
