"""Real Rust admission and SELF sends through installed SDK and OneBot peers."""

import base64
import hashlib
import importlib.util
import json
import os
import sqlite3
import subprocess
import sys
from contextlib import closing
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
from tests.harness.compact import reply, call
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer as QQ


def load(ctx, card):
    spec = importlib.util.spec_from_file_location(
        card, ctx.root / "tests/cases" / f"{card}.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run(ctx):
    previous = load(ctx, "CORE-018")
    aux = load(ctx, "CORE-050")
    sdk = ctx.working / "sdk"
    build = subprocess.run(
        [
            sys.executable,
            str(ctx.root / "ci/publish-contract.py"),
            "build",
            "--out",
            str(ctx.output / "sdk-distributions"),
            "--verify-work",
            str(sdk),
        ],
        capture_output=True,
        text=True,
    )
    if build.returncode:
        ctx.write("sdk-failure.json", {"stderr": build.stderr})
    ctx.equal("both SDK packages verify participation methods", build.returncode, 0)
    s = previous.Scene(ctx)
    space = json.dumps(["qq", "schedule", "700"], separators=(",", ":"))
    with s.svc.running():
        entry = s.op("entry", "group", space)["entry"]
        foreign = s.op("entry", "group", json.dumps(["qq", "schedule", "701"]))["entry"]
        open_entry = s.op("entry", "group", json.dumps(["qq", "schedule", "702"]))[
            "entry"
        ]
    folder = ctx.working / "participant"
    folder.mkdir()
    methods = json.loads(
        (ctx.root / "protocol/plugin/participation/methods.json").read_bytes()
    )["methods"]
    grants = ["observe.metadata", "forget.owner", *methods]
    env = {**os.environ, "YUKI_SCHEDULE_PEER": "offline-participation"}
    proofs = {}

    def record(key, text="original observation", person=None, accept=False):
        frame = {
            "source": key,
            "account": s.handles[person or s.a]["account"],
            "bot_stage": s.bot["assignment"],
            "reply_bot": "100",
            "space": space,
            "kind": "message",
            "observed": 200,
            "occurred": 200,
            "segments": [{"type": "text", "text": text}],
        }
        args = ["ingress", "record", json.dumps(frame)]
        if accept:
            args += ["--accept"]
        return s.op(*args)["event"]

    with Recorder() as http:
        config = s.svc.data / "yuki.toml"
        with config.open("a", encoding="utf-8") as f:
            f.write(
                f'\n[main]\nendpoint={json.dumps(http.url)}\nmodel="fixture"\nname="Yuki"\nmerge_ms=2\nroot_slots=24\nmax_input_units=4000000\nmax_output=512\n[qq]\nnamespace="schedule"\ndesignated_ingress="100"\n[qq.groups]\n"700"="paused"\n"702"="open"\n[[qq.bots]]\nself_id="100"\nstage={json.dumps(s.bot["assignment"])}\ntoken_env="YUKI_SCHEDULE_PEER"\n'
            )
            f.write(
                '\n[[plugins]]\nid="participation"\nnamespace="participation"\nprogram='
                + json.dumps(sys.executable)
                + "\nargs="
                + json.dumps(
                    [
                        str(ctx.root / "tests/peers/aux-model/peer.py"),
                        str(folder),
                        str(sdk / "python-install"),
                    ]
                )
                + "\ngrants="
                + json.dumps(grants)
                + "\n[plugins.participation]\nentries="
                + json.dumps([entry, open_entry])
                + "\n[plugins.participation.self_scenes]\n"
                + json.dumps(entry)
                + "="
                + json.dumps(s.bot["assignment"])
                + "\n"
                + json.dumps(open_entry)
                + "="
                + json.dumps(s.bot["assignment"])
                + "\n"
            )
        p = aux.Peer(s, folder)
        with s.svc.running(environment=env):
            s.until((folder / "ready.json").exists)
            opened = p.call(
                "participation.self_propose",
                {
                    "entry": open_entry,
                    "key": "configured-open",
                    "text": "ordinary configured group",
                },
            )
            _, q = s.wire(http)
            reply(q, "quiet configured group")
            s.until(
                lambda: s.rows(
                    "SELECT id FROM executions WHERE id=? AND state='completed'",
                    opened["execution"],
                )
            )
            ctx.equal(
                "configured open group needs no recovery command",
                s.rows(
                    "SELECT count(*) FROM recovered_groups WHERE native_group='702'"
                ),
                [(0,)],
            )
            src = record("race-source")
            p.call("participation.promote", {"event": src}, False)
            proposal = {
                "entry": entry,
                "key": "self-original",
                "text": "start from the original SELF scene",
            }
            p.call("participation.self_propose", proposal, False)
            p.call("participation.sources", {"entry": foreign}, False)
            ctx.equal(
                "paused group has no accepted source",
                s.rows("SELECT count(*) FROM accepted_sources"),
                [(0,)],
            )
            qq = QQ(s.svc.address, "100", "offline-participation")
            # Recover a specific receiver via the real authenticated group command.
            qq.send(
                {
                    "self_id": 100,
                    "user_id": 201,
                    "message_type": "group",
                    "post_type": "message",
                    "group_id": 700,
                    "time": 100,
                    "message_id": 36001,
                    "message": "/ai admin group current enable",
                }
            )
            probe = qq.receive()
            ctx.equal(
                "original SELF membership probe",
                probe["action"],
                "get_group_member_info",
            )
            qq.send(
                {
                    "echo": probe["echo"],
                    "status": "ok",
                    "retcode": 0,
                    "data": {"group_id": 700, "user_id": 100},
                }
            )
            s.deliver(qq, "send_group_msg", 36002)
            page = p.call("participation.sources", {"entry": entry})
            item = next(i for i in page["items"] if i["event"] == src)
            raw = p.call(
                "participation.source", {"event": src, "version": item["version"]}
            )
            ctx.equal(
                "source page has original content",
                json.loads(base64.b64decode(raw["data"]))["source"],
                "race-source",
            )
            origin_query = {"event": src, "version": raw["version"], "include_origin": True}
            origin_page = p.call("participation.source", origin_query)
            first_origin = {
                "entry": entry, "original_person": s.a,
                "actor_assignment": s.handles[s.a]["assignment"], "current_person": s.a,
                "observed_generation": 1, "current_generation": 1,
            }
            ctx.equal("source origin comes from the actual ingress row", origin_page["origin"], first_origin)
            ctx.equal("opting in leaves the original envelope and version intact",
                      {key: value for key, value in origin_page.items() if key != "origin"}, raw)
            p.call("participation.source", {**origin_query, "entry": foreign}, False)
            ctx.equal(
                "preview acquires original owner obligation",
                s.rows(
                    "SELECT source FROM source_holds WHERE owner=? AND item=?",
                    "participation",
                    "participation:" + str(src),
                ),
                [(src,)],
            )
            p.call("participation.source", {"event": src, "version": "wrong"}, False)
            # Rule admission races two invitation callbacks over the exact stored ID.
            a = p.submit("participation.promote", {"event": src, "expected_generation": 1})
            b = p.submit("participation.promote", {"event": src})
            with ThreadPoolExecutor(max_workers=1) as pool:
                same = pool.submit(
                    record, "race-source", "original observation", None, True
                )
                first = p.receive(a)
                second = p.receive(b)
                ctx.equal("rule keeps original transport ID", same.result(), src)
            ctx.equal(
                "one accepted event across rule and invitations",
                s.rows("SELECT count(*) FROM accepted_sources WHERE event_id=?", src),
                [(1,)],
            )
            _, q = s.wire(http)
            reply(q, "private reasoning only")
            s.finished(src)
            feedback = p.call("participation.feedback", {"event": src})
            ctx.check(
                "quiet is actual completed execution without send",
                feedback["executions"][0]["quiet"],
            )
            proofs["promotion"] = {
                "event": src,
                "receipts": [first, second],
                "feedback": feedback,
            }
            # An ordinary promoted event resumes the original armed Work.
            start = record("work-origin", "wait for another real human input")
            p.call("participation.promote", {"event": start})
            _, q = s.wire(http)
            reply(
                q,
                calls=[
                    call(
                        "accept-work",
                        "task_control",
                        {"action": "accept", "goal": "original human Work"},
                    )
                ],
            )
            body, q = s.wire(http)
            work = json.loads(
                next(
                    m["content"]
                    for m in body["messages"]
                    if m.get("tool_call_id") == "accept-work"
                )
            )["result"]["work_id"]
            reply(
                q,
                calls=[
                    call(
                        "wait-human",
                        "task_control",
                        {"action": "wait", "conditions": [{"kind": "conversation"}]},
                    )
                ],
            )
            s.until(
                lambda: s.rows(
                    "SELECT id FROM work_waits WHERE work=? AND state='armed'", work
                )
            )
            answer = record("original-answer", "answer for original human Work")
            p.call("participation.promote", {"event": answer})
            body, q = s.wire(http)
            ctx.check(
                "promoted input reaches original Work",
                "answer for original human Work" in str(body),
            )
            ctx.equal(
                "one matching original Work wake",
                s.rows("SELECT work FROM work_wait_routed WHERE event=?", answer),
                [(work,)],
            )
            reply(
                q,
                calls=[
                    call(
                        "complete-work",
                        "task_control",
                        {"action": "complete", "result": "done"},
                    )
                ],
            )
            s.until(
                lambda: s.rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'", work
                )
            )
            proofs["work"] = {
                "work": work,
                "source": start,
                "answer": answer,
                "feedback": p.call("participation.feedback", {"event": answer}),
            }
            # The most recent member is deliberately different from the original human.
            unrelated = record("other-member", "nearby group member", s.c)
            admitted = p.call("participation.self_propose", proposal)
            self_exec = admitted["execution"]
            body, q = s.wire(http)
            ctx.equal(
                "SELF has no borrowed person or event",
                s.rows(
                    "SELECT source,original_actor FROM executions WHERE id=?", self_exec
                ),
                [(None, None)],
            )
            ctx.check(
                "SELF model input excludes unrelated member",
                "nearby group member" not in str(body),
            )
            reply(
                q,
                calls=[
                    call(
                        "actual-self-send",
                        "send",
                        {"text": "original SELF group delivery"},
                    )
                ],
            )
            delivered = s.deliver(qq, "send_group_msg", 36003)
            ctx.equal("SELF sends to pinned group", str(delivered["group_id"]), "700")
            _, q = s.wire(http)
            reply(q, "finished")
            s.until(
                lambda: s.rows(
                    "SELECT id FROM executions WHERE id=? AND state='completed'",
                    self_exec,
                )
            )
            actual = p.call("participation.feedback", {"key": proposal["key"]})
            ctx.equal(
                "feedback reports a confirmed physical part",
                actual["executions"][0]["sends"][0]["confirmed_parts"],
                1,
            )
            ctx.check("actual send is not quiet", not actual["executions"][0]["quiet"])
            duplicate = p.call("participation.self_propose", proposal)
            ctx.equal(
                "same SELF proposal keeps execution",
                duplicate,
                {"execution": self_exec, "newly_accepted": False},
            )
            p.call("participation.self_propose", {**proposal, "text": "changed"}, False)
            p.call("participation.promote", {"event": 999999}, False)
            p.call("participation.feedback", {"event": unrelated}, False)
            proofs["self"] = {
                "proposal": proposal,
                "admitted": admitted,
                "feedback": actual,
                "route": delivered,
            }
            # Erased source is unavailable even though this generation has seen it.
            erase = record("erase-before-promote", "erasure canary", s.c)
            p.call("participation.source", {"event": erase})
            s.op("privacy", "erase", "participation-source", s.c, json.dumps([erase]))
            s.until(
                lambda: not s.op("privacy", "pending", "participation")["obligations"]
            )
            p.call("participation.promote", {"event": erase}, False)
            p.call("participation.source", {"event": erase}, False)
            p.call("participation.source", {"event": erase, "include_origin": True}, False)
            disabled = record("disabled-before-promote", "original disabled actor", s.c)
            s.op("person", "set-enabled", s.c, "false")
            p.call("participation.promote", {"event": disabled}, False)
            p.call("participation.source", {"event": disabled, "include_origin": True}, False)
            s.op("person", "set-enabled", s.c, "true")
            proofs["erasure"] = {
                "event": erase,
                "accepted": s.rows(
                    "SELECT event_id FROM accepted_sources WHERE event_id=?", erase
                ),
            }
            reset = record("reset-window", "/ai new", accept=True)
            s.until(lambda: s.rows("SELECT event FROM main_controls WHERE event=?", reset))
            for generation in (1, 2):
                p.call("participation.promote", {"event": unrelated, "expected_generation": generation}, False)
            ctx.equal("a stale observation cannot start a new turn after reset",
                      s.rows("SELECT event_id FROM accepted_sources WHERE event_id=?", unrelated), [])
            fresh = record("new-window-observation", "observed without Main")
            fresh_origin = p.call("participation.source", {"event": fresh, "include_origin": True})["origin"]
            ctx.equal("an unpromoted event records the new generation", fresh_origin,
                      {**first_origin, "observed_generation": 2, "current_generation": 2})
            ctx.equal("a redelivery after reset keeps the original event", record("race-source"), src)
            s.op("person", "merge", s.a, s.b)
            moved = {**first_origin, "current_person": s.b, "current_generation": 2}
            ctx.equal("current scope changes without replacing the original observation",
                      p.call("participation.source", origin_query)["origin"], moved)
            qq.close()
        # Insert an event with the pre-migration column list in this isolated database.
        # It has original authority but no observation-time generation evidence.
        with closing(sqlite3.connect(s.svc.data / "core.sqlite")) as database, database:
            legacy = database.execute(
                """INSERT INTO events(source_scope,source_key,entry_id,account_id,raw_actor,actor_stage,
                       bot_stage,occurred_at,observed_at,envelope,mentions)
                   SELECT source_scope,'pre-reader-61',entry_id,account_id,raw_actor,actor_stage,
                       bot_stage,occurred_at,observed_at,json_set(envelope,'$.source','pre-reader-61'),mentions
                   FROM events WHERE id=? RETURNING id""", (fresh,)
            ).fetchone()[0]
        with s.svc.running(environment=env):
            # No process-local envelope is recreated after a cold start.
            again = p.call("participation.self_propose", proposal)
            ctx.equal(
                "cold SELF proposal retains original ID", again["execution"], self_exec
            )
            p.call("participation.promote", {"event": 999999}, False)
            cold = p.call("participation.feedback", {"key": proposal["key"]})
            ctx.equal("cold feedback retains physical send", cold, actual)
            ctx.check(
                "cold duplicate does not generate new model attempt", http.wires.empty()
            )
            cold_origin = p.call("participation.source", origin_query)["origin"]
            legacy_origin = p.call("participation.source", {"event": legacy, "include_origin": True})["origin"]
            p.call("participation.promote", {"event": legacy, "expected_generation": 2}, False)
            ctx.equal("cold hydration retains the original identity and generation", cold_origin, moved)
            ctx.equal("older rows without a recorded observation generation remain unknown",
                      legacy_origin, {**moved, "observed_generation": None})
            proofs["observation_origin"] = {"event": src, "original": first_origin, "cold": cold_origin,
                                            "fresh": fresh_origin, "legacy": legacy_origin}
        import re

        config.write_text(
            re.sub(
                r"\n\[qq\]\n.*?(?=\n\[\[plugins\]\])",
                "\n",
                config.read_text(),
                flags=re.S,
            ),
            encoding="utf-8",
        )
        before = s.rows("SELECT count(*) FROM model_requests")
        with s.svc.running(environment=env):
            p.call(
                "participation.self_propose",
                {**proposal, "key": "globally-disabled"},
                False,
            )
            p.call("participation.sources", {"entry": entry}, False)
            p.call("participation.promote", {"event": src}, False)
            ctx.equal(
                "disabled QQ cannot admit new activation",
                s.rows("SELECT count(*) FROM model_requests"),
                before,
            )
        proofs["global_disabled"] = {"model_requests": before, "new_activations": 0}
    ready = json.loads((folder / "ready.json").read_bytes())
    proofs["negotiated_participation_methods"] = sorted(
        m for m in ready["methods"] if m.startswith("participation.")
    )
    ctx.equal(
        "actual SDK negotiates all participation methods",
        proofs["negotiated_participation_methods"],
        sorted(methods),
    )
    proofs["binary_sha256"] = hashlib.sha256(
        (
            ctx.root
            / "target/debug"
            / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
        ).read_bytes()
    ).hexdigest()
    proofs["host_idl_sha256"] = hashlib.sha256(
        (ctx.root / "sdk/rust/src/host-preview.json").read_bytes()
    ).hexdigest()
    proofs["participation_schema_sha256"] = hashlib.sha256(
        (ctx.root / "protocol/plugin/participation/methods.json").read_bytes()
    ).hexdigest()
    proofs["external"] = external(ctx, sdk, aux)
    proofs["real_extension_consumer"] = "pending EXT-025"
    return proofs


def external(ctx, sdk, aux):
    from tests.harness.compact import Owner

    o = Owner(ctx, "participation-external", 31)
    with o.svc.running():
        bot = o.op("account", "open", "telegram", "scene-test", "100", 1, "--start", 1)
        entry = o.op("entry", "group", json.dumps(["telegram", "scene-test", "room"]))[
            "entry"
        ]
    folder = ctx.working / "external-participant"
    folder.mkdir()
    gateway = ctx.working / "external-gateway"
    gateway.mkdir()
    methods = json.loads(
        (ctx.root / "protocol/plugin/participation/methods.json").read_bytes()
    )["methods"]
    with Recorder() as http:
        # Actual installed gateway peer records every dispatched physical part.
        with (o.svc.data / "yuki.toml").open("a", encoding="utf-8") as f:
            f.write(
                "\n[main]\nendpoint="
                + json.dumps(http.url)
                + '\nmodel="fixture"\nname="Yuki"\nmerge_ms=2\nroot_slots=16\nmax_input_units=4000000\nmax_output=128\n'
            )
            f.write(
                '\n[[plugins]]\nid="external_participant"\nnamespace="external_participant"\nprogram='
                + json.dumps(sys.executable)
                + "\nargs="
                + json.dumps(
                    [
                        str(ctx.root / "tests/peers/aux-model/peer.py"),
                        str(folder),
                        str(sdk / "python-install"),
                    ]
                )
                + "\ngrants="
                + json.dumps(["forget.owner", *methods])
                + "\n[plugins.participation]\nentries="
                + json.dumps([entry])
                + "\n[plugins.participation.self_scenes]\n"
                + json.dumps(entry)
                + "="
                + json.dumps(bot["assignment"])
                + "\n"
            )
            f.write(
                '\n[[plugins]]\nid="gateway"\nnamespace="scene-test"\nplatform="telegram"\nbot_stage='
                + json.dumps(bot["assignment"])
                + "\nprogram="
                + json.dumps(sys.executable)
                + "\nargs="
                + json.dumps(
                    [
                        str(ctx.root / "tests/peers/host/peer.py"),
                        str(gateway),
                        str(sdk / "python-install"),
                    ]
                )
                + '\ngrants=["gateway.send"]\n'
            )
        p = aux.Peer(o, folder)
        with o.svc.running():
            o.until((folder / "ready.json").exists)
            o.until(
                lambda: o.rows(
                    "SELECT plugin FROM host_installations WHERE plugin='gateway' AND online"
                )
            )
            proposal = {
                "entry": entry,
                "key": "native-two-parts",
                "text": "send from original SELF gateway",
            }
            accepted = p.call("participation.self_propose", proposal)
            _, q = o.wire(http)
            reply(q, calls=[call("native-send", "send", {"text": "part-one|part-two"})])
            _, q = o.wire(http)
            reply(q, "done")
            o.until(
                lambda: o.rows(
                    "SELECT id FROM executions WHERE id=? AND state='completed'",
                    accepted["execution"],
                )
            )
            feedback = p.call("participation.feedback", {"key": proposal["key"]})
            ctx.equal(
                "external feedback has two confirmed physical parts",
                feedback["executions"][0]["sends"][0]["confirmed_parts"],
                2,
            )
            deliveries = [
                json.loads(l)
                for l in (gateway / "deliveries.jsonl").read_text().splitlines()
            ]
            ctx.equal(
                "external receiver is original scene for both parts",
                [v["arguments"]["route"]["chat"] for v in deliveries],
                ["room", "room"],
            )
            ctx.equal(
                "external SELF retains independent original root",
                o.rows(
                    "SELECT source,original_actor,root FROM executions x JOIN budget_members b ON b.execution=x.id WHERE x.id=?",
                    accepted["execution"],
                ),
                [(None, None, accepted["execution"])],
            )
            (gateway / "cut-second").write_text("fixture crash after second dispatch")
            cut = {
                "entry": entry,
                "key": "unknown-part",
                "text": "original partially confirmed send",
            }
            unknown = p.call("participation.self_propose", cut)
            _, q = o.wire(http)
            reply(
                q,
                calls=[
                    call(
                        "unknown-send",
                        "send",
                        {"text": "confirmed-first|unknown-second"},
                    )
                ],
            )
            o.until(
                lambda: o.rows(
                    "SELECT id FROM executions WHERE id=? AND state IN('held','completed')",
                    unknown["execution"],
                )
            )
            ctx.check(
                "offline original gateway prevents another activation",
                http.wires.empty(),
            )
            # Historical feedback is available while the gateway itself is down.
            partial = p.call("participation.feedback", {"key": cut["key"]})
            ctx.equal(
                "partial send keeps one confirmation and unknown state",
                [
                    (v["state"], v["confirmed_parts"])
                    for v in partial["executions"][0]["sends"]
                ],
                [("unknown", 1)],
            )
            ctx.check(
                "unknown send is never quiet", not partial["executions"][0]["quiet"]
            )
            p.call("participation.self_propose", {**cut, "key": "offline-new"}, False)
            before = (gateway / "deliveries.jsonl").read_text().splitlines()
        with o.svc.running():
            o.until(
                lambda: o.rows(
                    "SELECT plugin FROM host_installations WHERE plugin='gateway' AND online"
                )
            )
            recovered = p.call("participation.self_propose", cut)
            ctx.equal(
                "cold unknown SELF proposal keeps execution",
                recovered["execution"],
                unknown["execution"],
            )
            ctx.equal(
                "cold unknown send does not replay parts",
                (gateway / "deliveries.jsonl").read_text().splitlines(),
                before,
            )
            ctx.equal(
                "cold original feedback keeps partial truth",
                p.call("participation.feedback", {"key": cut["key"]}),
                partial,
            )
            ctx.check("cold unknown creates no model request", http.wires.empty())
        return {"normal": feedback, "partial": partial, "physical_parts": len(before)}
