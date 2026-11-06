"""Real owner HTTP and packaged local SDK. No contact with public QQ users or external services."""

import hashlib
import json
import os
import sqlite3
import subprocess
import sys
import time
import uuid
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder
from tests.harness.personal import matches_reply


def key():
    return str(uuid.uuid4())


def wire(address, path, token=None, body=None, method=None):
    raw = json.dumps(body, ensure_ascii=False).encode() if body is not None else None
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token
    req = Request("http://" + address + path, data=raw, headers=headers, method=method)
    try:
        with urlopen(req, timeout=8) as r:
            return r.status, json.load(r)
    except HTTPError as e:
        with e:
            return e.code, json.load(e)


class Plugin:
    def __init__(self, owner, folder):
        self.owner = owner
        self.folder = folder
        self.seq = 10

    def rpc(self, method, params, ok=True):
        self.seq += 1
        path = self.folder / f"cmd-{self.seq}.json"
        t = path.with_suffix(".tmp")
        t.write_text(
            json.dumps({"id": self.seq, "method": method, "params": params}),
            encoding="utf-8",
        )
        t.replace(path)
        output = self.folder / f"out-{self.seq}.json"
        self.owner.until(output.is_file)
        res = json.loads(output.read_bytes())
        output.unlink()
        self.owner.ctx.equal(
            "SDK " + method + " " + ("accepted" if ok else "denied"),
            "result" in res,
            ok,
        )
        return res.get("result")

    def ready(self):
        self.owner.until(lambda: (self.folder / "ready.json").is_file())
        return json.loads((self.folder / "ready.json").read_bytes())


def run(ctx):
    base = ctx.root / "protocol/baseline-v4"
    docs = [
        json.loads((base / file).read_bytes())
        for file in ("client-openapi.json", "admin-openapi.json")
    ]
    for spec in ("personal", "connector/4"):
        index = json.loads((ctx.root / "protocol" / spec / "source.json").read_bytes())
        ctx.check(
            "fixed original " + spec + " members verified",
            all(
                hashlib.sha256((ctx.root / m["path"]).read_bytes()).hexdigest()
                == m["sha256"]
                for m in index["members"]
            ),
        )
    o = Owner(ctx, "personal", 20)
    rejected = key()
    with closing(sqlite3.connect(o.svc.data / "core.sqlite")) as db:
        # Single-use isolated fixture: fail after the private action and mailbox, before receipt commit.
        db.execute(
            "CREATE TRIGGER reject_fixture_receipt BEFORE INSERT ON personal_receipts WHEN NEW.operation='"
            + rejected
            + "' BEGIN SELECT RAISE(ABORT,'receipt fixture'); END"
        )
        db.commit()
    folder = ctx.working / "bridge"
    folder.mkdir()
    sdk = ctx.working / "sdk"
    dist = ctx.output / "sdk-distributions"
    pkg = subprocess.run(
        [
            sys.executable,
            str(ctx.root / "ci/publish-contract.py"),
            "build",
            "--out",
            str(dist),
            "--verify-work",
            str(sdk),
        ],
        capture_output=True,
        text=True,
    )
    if pkg.returncode:
        ctx.write("package-error.json", {"stderr": pkg.stderr, "stdout": pkg.stdout})
    ctx.equal("actual Rust and Python SDKs built and crosswired", pkg.returncode, 0)
    plugin = Plugin(o, folder)
    secrets = []
    routes = set()
    with Recorder() as model:
        o.config(model)
        with (o.svc.data / "yuki.toml").open("a") as cfg:
            cfg.write(
                '\n[[plugins]]\nid="personal-peer"\nnamespace="local-agent"\nprogram='
                + json.dumps(sys.executable)
                + "\nargs="
                + json.dumps(
                    [
                        str(ctx.root / "tests/peers/personal/consumer.py"),
                        str(folder),
                        str(sdk / "python-install"),
                    ]
                )
                + '\ngrants=["personal.auth","personal.work","native.events"]\n'
            )
        with o.svc.running():
            caps = plugin.ready()
            ctx.equal(
                "real installed peer negotiated its methods",
                set(caps["methods"]),
                {
                    "device.auth",
                    "work.get",
                    "work.input",
                    "work.cancel",
                    "native.notify",
                    "native.queryAck",
                },
            )

            def http(path, token=None, data=None, expected=200, method=None):
                status, out = wire(o.svc.address, path, token, data, method)
                if status != expected:
                    ctx.write(
                        "unexpected-http.json",
                        {
                            "path": path,
                            "status": status,
                            "expected": expected,
                            "problem": out.get("problem"),
                        },
                    )
                ctx.equal(
                    "HTTP "
                    + (method or ("POST" if data is not None else "GET"))
                    + " "
                    + path,
                    status,
                    expected,
                )
                if status == 200:
                    verb = method or ("POST" if data is not None else "GET")
                    ctx.check(
                        "pinned wire " + path, matches_reply(docs, verb, path, out)
                    )
                    from tests.harness.personal import template

                    routes.add(verb + " " + template(docs, path))
                return out.get("data", out)

            def issue(purpose, person=None, op=None):
                x = o.op("personal", "issue", person or o.actor, op or key(), purpose)
                if x["code"]:
                    secrets.append(x["code"])
                return x["code"]

            def pair(code, purpose="chat", want=200):
                v = http(
                    "/client/v1/pair" if purpose == "chat" else "/agents/v1/pair",
                    None,
                    {"code": code, "device_label": "HJW synthetic " + purpose},
                    want,
                )
                if want == 200:
                    secrets.append(v["token"])
                return v

            ctx.equal(
                "published Client9 revision", http("/client/v1/meta")["api_revision"], 9
            )
            c = issue("connector")
            pair(c, "chat", 401)
            device = pair(c, "connector")
            ctx.equal(
                "connector pairing creates no app conversation",
                o.rows("SELECT count(*) FROM accounts WHERE platform='app'"),
                [(0,)],
            )
            pair(c, "connector", 401)
            c = issue("chat")
            pair(c, "connector", 401)
            a = pair(c)
            b = pair(issue("chat"))
            me = http("/client/v1/me", a["token"])
            ctx.equal(
                "two devices share the original entry",
                http("/client/v1/me", b["token"])["conversation_id"],
                me["conversation_id"],
            )
            http("/client/v1/me", device["token"], expected=401)
            http(
                "/core/v1/entry/exchange",
                a["token"],
                {"purpose": "connector", "plugin": "personal-peer", "work": "foreign"},
                401,
            )
            http("/admin/v1/health", a["token"], expected=403)
            o.op("person", "set-admin", o.actor, "true")
            http("/admin/v1/health", a["token"])
            http("/admin/v1/health", device["token"], expected=401)
            o.op("person", "set-admin", o.actor, "false")
            http("/admin/v1/health", a["token"], expected=403)
            other = str(uuid.uuid4())
            o.op("person", "create", other)
            visitor = pair(issue("chat", other))
            http(
                "/client/v1/devices/" + b["device_id"],
                visitor["token"],
                expected=404,
                method="DELETE",
            )
            o.op("person", "set-admin", o.actor, "true")
            rolekey = key()
            role_body = {
                "request_id": rolekey,
                "expected_revision": o.rows(
                    "SELECT revision FROM persons WHERE id=?", other
                )[0][0],
            }
            role_url = "/admin/v1/persons/" + other + "/superuser/grant"
            http(role_url, a["token"], role_body)
            http("/admin/v1/health", visitor["token"])
            o.op("person", "set-admin", o.actor, "false")
            http(role_url, a["token"], role_body, 403)
            o.op("person", "set-admin", other, "false")
            evt = o.human("/ai memory add Favorite tea is jasmine.")
            o.until(
                lambda: o.rows("SELECT number FROM memory_facts WHERE status='active'")
            )
            num = o.rows("SELECT number FROM memory_facts WHERE status='active'")[0][0]
            facts_url = "/client/v1/memory/facts"
            own = http(facts_url + "?about=me", a["token"])["items"]
            ctx.equal(
                "personal facts restrict to original speaker",
                (
                    [x["fact_id"] for x in own],
                    http(facts_url, visitor["token"])["items"],
                ),
                ([num], []),
            )
            fact_url = facts_url + "/" + str(num)
            detail = http(fact_url, a["token"])
            http(fact_url, visitor["token"], expected=404)
            edit = {
                "request_id": key(),
                "action": "correct",
                "expected_revision": detail["revision"],
                "text": "Favorite tea is green.",
            }
            update = http(fact_url + "/corrections", a["token"], edit)
            ctx.check(
                "second device reads original correction",
                http(fact_url + "/corrections", b["token"], edit)["replay"],
            )
            ctx.equal(
                "corrected fact still uses original evidence",
                (http(fact_url, a["token"])["text"], detail["evidence"][0]["event_id"]),
                ("Favorite tea is green.", evt),
            )
            http(
                fact_url + "/corrections", a["token"], dict(edit, request_id=key()), 409
            )
            sched_evt = o.human("schedule personal reminder")
            _, sched_q = o.wire(model)
            create = {
                "action": "create",
                "task": {
                    "trigger": {"type": "after", "seconds": 7200},
                    "goal": "later reminder",
                    "name": "personal",
                },
            }
            reply(sched_q, calls=[call("schedule", "automation_manage", create)])
            paid, sched_q = o.wire(model)
            delivered = json.loads(
                next(
                    m["content"]
                    for m in paid["messages"]
                    if m.get("tool_call_id") == "schedule"
                )
            )
            ctx.equal("real model created schedule", delivered["state"], "confirmed")
            reply(sched_q, "scheduled")
            o.finished(sched_evt)
            schedules_url = "/client/v1/automations"
            actual = http(schedules_url, a["token"])["items"][0]
            ctx.equal(
                "other person cannot enumerate schedule",
                http(schedules_url, visitor["token"])["items"],
                [],
            )
            pause_url = (
                schedules_url + "/" + str(actual["automation_id"]) + "/actions/pause"
            )
            pause = {"request_id": key(), "expected_revision": actual["revision"]}
            ctx.equal(
                "personal route pauses original schedule",
                http(pause_url, b["token"], pause)["state"],
                "paused",
            )
            ctx.check(
                "personal schedule mutation replays once",
                http(pause_url, a["token"], pause)["replay"],
            )
            http(pause_url, visitor["token"], dict(pause, request_id=key()), 404)
            event = o.human("accept this original QQ Work")
            _, q = o.wire(model)
            reply(
                q,
                calls=[
                    call(
                        "accept",
                        "task_control",
                        {"action": "accept", "goal": "private handoff"},
                    )
                ],
            )
            body, q = o.wire(model)
            result = json.loads(
                next(
                    m["content"]
                    for m in body["messages"]
                    if m.get("tool_call_id") == "accept"
                )
            )
            work = result["result"]["work_id"]
            work_url = "/client/v1/works/" + work
            http(work_url, visitor["token"], expected=404)
            snap = http(work_url, a["token"])
            req = key()
            request = {"request_id": req, "text": "私下補充 令狐"}
            before = o.rows("SELECT count(*) FROM root_budgets")[0][0]
            http(
                work_url + "/inputs",
                a["token"],
                dict(request, request_id=rejected),
                422,
            )
            ctx.equal(
                "injected failure rolls back action, mailbox and personal receipt",
                (
                    o.rows(
                        "SELECT count(*) FROM work_mail WHERE source_key=?", rejected
                    ),
                    o.rows("SELECT count(*) FROM events WHERE source_key=?", rejected),
                    o.rows(
                        "SELECT count(*) FROM personal_receipts WHERE operation=?",
                        rejected,
                    ),
                ),
                ([(0,)], [(0,)], [(0,)]),
            )
            with ThreadPoolExecutor(max_workers=2) as pool:
                first = pool.submit(
                    wire, o.svc.address, work_url + "/inputs", a["token"], request
                )
                second = pool.submit(
                    wire, o.svc.address, work_url + "/inputs", b["token"], request
                )
                replies = [r.result() for r in (first, second)]
            ctx.equal(
                "two devices same UUID each receives original private input",
                sorted((s, v["data"]["replay"]) for s, v in replies),
                [(200, False), (200, True)],
            )
            receipt = http("/client/v1/requests/" + req, b["token"])
            expected_hash = hashlib.sha256(
                json.dumps(
                    ["client.work_input", work, {"text": request["text"]}],
                    sort_keys=True,
                    separators=(",", ":"),
                ).encode()
            ).hexdigest()
            ctx.equal(
                "original client fingerprint and selected Work",
                (receipt["payload_hash"], receipt["target"]),
                (expected_hash, work),
            )
            http(work_url + "/inputs", a["token"], dict(request, text="different"), 409)
            http("/client/v1/requests/" + req, visitor["token"])
            ctx.equal(
                "one private mailbox, no App normal admission, original budget only",
                (
                    o.rows("SELECT count(*) FROM work_mail WHERE source_key=?", req),
                    o.rows(
                        "SELECT count(*) FROM main_inputs WHERE event=?",
                        receipt["event_id"],
                    ),
                    o.rows("SELECT count(*) FROM root_budgets"),
                ),
                ([(1,)], [(0,)], [(before,)]),
            )
            target = {
                "device_id": device["device_id"],
                "operation_id": "original-op",
                "intent": "prompt",
            }
            r = http(
                work_url + "/inputs",
                a["token"],
                {"request_id": key(), "text": "native prompt", "agent_target": target},
            )

            def exchange(token, purpose):
                v = http(
                    "/core/v1/entry/exchange",
                    token,
                    {"purpose": purpose, "plugin": "personal-peer", "work": work},
                )
                secrets.append(v["proof"])
                return v["proof"]

            ticket = exchange(device["token"], "connector")
            plugin.rpc("device.auth", {"owner": o.actor, "proof": ticket}, False)
            handle = plugin.rpc("device.auth", {"proof": ticket})["handle"]
            secrets.append(handle)
            plugin.rpc("device.auth", {"proof": ticket}, False)
            op = {"handle": handle, "operation": "original-op", "event_key": "e-1"}
            plugin.rpc("native.queryAck", dict(op, operation="unknown-op"), False)
            ctx.equal(
                "no early ACK", plugin.rpc("native.queryAck", op)["accepted"], False
            )
            source = plugin.rpc(
                "native.notify", dict(op, payload={"result": "private answer"})
            )
            replay = plugin.rpc(
                "native.notify", dict(op, payload={"result": "private answer"})
            )
            ctx.equal(
                "original native ACK and watermark",
                (source["watermark"], replay["event"], replay["replay"]),
                (1, source["event"], True),
            )
            plugin.rpc("native.notify", dict(op, payload={"result": "changed"}), False)
            ctx.equal(
                "only one private delivery for native key",
                o.rows("SELECT count(*) FROM native_receipts"),
                [(1,)],
            )
            from tests.peers.personal.wire4 import exercise

            connector_wires = exercise(ctx, wire, o, plugin, work, device, a)
            private = exchange(b["token"], "chat")
            chat = plugin.rpc("device.auth", {"proof": private})["handle"]
            secrets.append(chat)
            plugin.rpc("work.get", {"handle": chat})
            again = plugin.rpc("work.input", {"handle": chat, "request": request})
            ctx.check(
                "SDK recovers the same HTTP input receipt",
                again["replay"] and again["event_id"] == receipt["event_id"],
            )

            plugin.rpc("work.get", {"handle": handle}, False)
            http("/client/v1/devices/" + b["device_id"], a["token"], method="DELETE")
            http("/client/v1/me", b["token"], expected=401)
            plugin.rpc("work.get", {"handle": chat}, False)
            ctx.check(
                "other chat credential still usable",
                http("/client/v1/me", a["token"])["person_id"] == o.actor,
            )

            def need(q, idx):
                reply(
                    q,
                    calls=[
                        call(
                            "need-" + str(idx),
                            "task_control",
                            {
                                "action": "need_input",
                                "work_id": work,
                                "reason": "choose the original source",
                            },
                        )
                    ],
                )

            need(q, 1)
            until = time.monotonic() + 8
            rounds = 1
            while time.monotonic() < until and not o.rows(
                "SELECT id FROM work_nodes WHERE id=? AND status='need_input'", work
            ):
                try:
                    _, _, next_reply = model.wires.get(timeout=0.1)
                except __import__("queue").Empty:
                    continue
                rounds += 1
                need(next_reply, rounds)
            o.finished(event)
            ctx.equal(
                "original Work is waiting on a private answer",
                http(work_url, a["token"])["state"],
                "waiting_user",
            )
            http(
                work_url + "/inputs",
                a["token"],
                {"request_id": key(), "text": "original QQ work should resume"},
            )
            resumed_body, resumed_q = o.wire(model)
            ctx.check(
                "waiting Work resumes on original QQ context",
                "original QQ work should resume"
                in json.dumps(resumed_body, ensure_ascii=False),
            )
            ctx.equal(
                "authenticated wake retains original root count",
                o.rows("SELECT count(*) FROM root_budgets"),
                [(before,)],
            )
            home = http("/client/v1/home", a["token"])
            ctx.check(
                "original QQ Work appears in personal home",
                any(item["work_id"] == work for item in home["works"]),
            )
            page = http("/client/v1/works?limit=1", a["token"])
            ctx.check(
                "public Work page carries original result delivery",
                page["works"][0]["source"] == page["works"][0]["result_delivery"],
            )
            events = http(
                "/client/v1/conversations/"
                + snap["conversation_id"]
                + "/events?limit=10",
                a["token"],
            )
            ctx.check(
                "source page contains original QQ message but no private input",
                event in [i["event_id"] for i in events["items"]]
                and request["text"] not in str(events),
            )
            rev = http(work_url, a["token"])["revision"]
            cancel_body = {"request_id": key(), "expected_revision": rev}
            http(
                work_url + "/cancel",
                a["token"],
                dict(cancel_body, expected_revision=rev + 7),
                409,
            )
            ticket = exchange(a["token"], "chat")
            cancel_handle = plugin.rpc("device.auth", {"proof": ticket})["handle"]
            secrets.extend([ticket, cancel_handle])
            plugin.rpc("work.cancel", {"handle": cancel_handle, "request": cancel_body})
            canceled = http(work_url + "/cancel", a["token"], cancel_body)
            ctx.check(
                "HTTP cancellation replays original native callback", canceled["replay"]
            )

            reply(resumed_q, "late model response")
            http(
                work_url + "/inputs",
                a["token"],
                {"request_id": key(), "text": "late private question"},
                409,
            )
            late = plugin.rpc(
                "native.notify",
                dict(
                    op, event_key="late", payload={"question": "late native question"}
                ),
            )
            ctx.equal(
                "late native terminal event stays private without new mail",
                o.rows(
                    "SELECT count(*) FROM work_wait_mail WHERE source_event=?",
                    late["event"],
                ),
                [(0,)],
            )
            ctx.equal(
                "terminal result still cancelled",
                http(work_url, a["token"])["state"],
                "cancelled",
            )
            with closing(
                sqlite3.connect(
                    (o.svc.data / "core.sqlite").as_uri() + "?mode=ro", uri=True
                )
            ) as db:
                ledger = "\n".join(db.iterdump())
            ctx.check(
                "no raw token, code, proof or handle in permanent records",
                all(s not in ledger for s in secrets),
            )
        (folder / "ready.json").unlink(missing_ok=True)
        with o.svc.running():
            connection = plugin.ready()
            ctx.check(
                "actual cold peer has a different PID", connection["pid"] != caps["pid"]
            )
            plugin.rpc("native.queryAck", op, False)
            fresh = exchange(device["token"], "connector")
            fresh_handle = plugin.rpc("device.auth", {"proof": fresh})["handle"]
            resumed = plugin.rpc("native.queryAck", dict(op, handle=fresh_handle))
            ctx.equal(
                "fresh original owner recovers a committed ACK",
                (resumed["event"], resumed["watermark"]),
                (source["event"], 1),
            )
    from tests.harness.personal_files import observe

    delivered_files = observe(ctx, docs, routes, wire)
    from tests.harness.connector_auth import run as connector_auth
    connector=connector_auth(ctx,sdk,Plugin,wire)
    ctx.write("connector-authentication.json",connector)
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    return {
        "binary_sha256": sha(
            ctx.root
            / "target/debug"
            / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
        ),
        "validated_routes": sorted(routes),
        "delivered_artifact": delivered_files,
        "host_idl_sha256": sha(ctx.root / "sdk/rust/src/host-preview.json"),
        "personal_schema_sha256": sha(
            ctx.root / "protocol/plugin/personal/methods.json"
        ),
        "cases": {
            "H25": {
                "shared_chat_entry": me["conversation_id"],
                "connector_pair_created_chat": False,
                "ordinary_admin": 403,
                "superuser_admin": 200,
                "revoked_chat": 401,
            },
            "H26": {
                "original_work": work,
                "source_event": event,
                "input_receipt": receipt,
                "native_ack": source,
                "late_ack": late,
                "cancellation": canceled,
                "resumed": resumed,
            },
        },
        "wire4_ack_replay": connector_wires,
        "negotiated_methods": sorted(set(caps["methods"])|set(connector['methods'])),
        "client_revision": 9,
        "connector_revision": 4,
    }
