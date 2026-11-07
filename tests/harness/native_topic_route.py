"""Keep canonical topics and original native roots separate across retries and deployments."""

from contextlib import closing
import json
import sqlite3
import sys

from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def run(ctx, sdk, profile="debug"):
    owner = Owner(ctx, "native-topic", 55, profile)
    peers = {}
    with owner.svc.running():
        for name, platform, tenant in [
            ("topic-a", "feishu", "tenant-a"),
            ("topic-b", "lark", "tenant-b"),
        ]:
            bot = owner.op(
                "account", "open", platform, name, "native-bot", 1, "--start", 1
            )["assignment"]
            owner.op(
                "account",
                "open",
                platform,
                name,
                "sender",
                1,
                "--person",
                owner.actor,
                "--start",
                1,
            )
            folder = ctx.working / name
            folder.mkdir()
            identity = {"application": "same-app-id", "tenant": tenant}
            (folder / "authority.json").write_text(json.dumps(identity))
            (folder / "record-route").touch()
            definition = dict(
                id=name,
                namespace=name,
                platform=platform,
                bot_stage=bot,
                program=sys.executable,
                args=[
                    str(ctx.root / "tests/peers/host-media/peer.py"),
                    str(folder),
                    str(sdk),
                ],
                grants=["gateway.inbound", "gateway.media", "gateway.send"],
            )
            with (owner.svc.data / "yuki.toml").open("a", encoding="utf-8") as config:
                config.write(
                    "\n[[plugins]]\n"
                    + "\n".join(k + "=" + json.dumps(v) for k, v in definition.items())
                )
                config.write(
                    "\n[plugins.gateway_media]\n"
                    + "\n".join(k + "=" + json.dumps(v) for k, v in identity.items())
                    + "\n"
                )
            peers[name] = {"folder": folder, "bot": bot, "identity": identity}

    def submit(peer, params, denied=False):
        folder = peer["folder"]
        result = folder / "probed.json"
        result.unlink(missing_ok=True)
        stage = folder / "probe.tmp"
        stage.write_text(json.dumps({"method": "gateway.inbound", "params": params}))
        stage.replace(folder / "probe.json")
        owner.until(result.exists)
        actual = json.loads(result.read_bytes())
        ctx.equal("original topic admission disposition", actual["denied"], denied)
        return actual["result"]

    def rows(sql, *args):
        return owner.rows(sql, *args)

    def edit(sql, *args):
        with closing(sqlite3.connect(owner.svc.data / "core.sqlite")) as connection:
            connection.execute(sql, args)
            connection.commit()

    events = []
    with Recorder() as model:
        owner.config(model, binding=False)
        with owner.svc.running():
            for name, peer in peers.items():
                folder = peer["folder"]
                owner.until(lambda: (folder / "ready.json").exists())
                params = {
                    "source": "same-message",
                    "native_id": "om_message",
                    "sender": "sender",
                    "space": "same-group",
                    "topic": "omt_thread",
                    "topic_root": "om_root",
                    "occurred": 11,
                    "kind": "message",
                    "accept": True,
                    "media_identity": peer["identity"],
                    "segments": [
                        {"type": "mention", "native": "native-bot", "label": "@Yuki"},
                        {"type": "text", "text": "original topic"},
                    ],
                }
                legacy = {k: v for k, v in params.items() if k != "topic_root"}
                legacy.update(
                    source="legacy-message",
                    native_id="om_legacy",
                    kind="data",
                    accept=False,
                )
                old = submit(peer, legacy)["event"]
                ctx.equal(
                    "a new field never creates a second legacy event",
                    submit(peer, {**legacy, "topic_root": "om_unproven"})["event"],
                    old,
                )
                ctx.equal(
                    "redelivery never invents missing root provenance",
                    rows(
                        "SELECT count(*) FROM gateway_topic_origins WHERE event=?", old
                    ),
                    [(0,)],
                )
                before = rows("SELECT count(*) FROM events")
                for missing in ("topic", "native_id", "media_identity"):
                    submit(
                        peer, {k: v for k, v in params.items() if k != missing}, True
                    )
                    ctx.equal(
                        "incomplete root proof leaves no ingress",
                        rows("SELECT count(*) FROM events"),
                        before,
                    )
                submit(
                    peer,
                    {
                        **params,
                        "media_identity": {**peer["identity"], "tenant": "foreign"},
                    },
                    True,
                )
                ctx.equal(
                    "wrong tenant has no durable ingress",
                    rows("SELECT count(*) FROM events"),
                    before,
                )
                receipt = submit(peer, params)
                ctx.check("new original topic event accepted", receipt["accepted"])
                event = receipt["event"]
                _, queue = owner.wire(model)
                ctx.equal(
                    "exact duplicate returns original event",
                    submit(peer, params)["event"],
                    event,
                )
                submit(peer, {**params, "topic_root": "om_foreign"}, True)
                downgraded = {k: v for k, v in params.items() if k != "topic_root"}
                ctx.equal(
                    "older sender can still replay its event",
                    submit(peer, downgraded)["event"],
                    event,
                )
                ctx.equal(
                    "canonical topic and native root are independently preserved",
                    rows(
                        "SELECT d.topic,o.topic_root,o.namespace,o.stage,json_extract(o.identity,'$.tenant') FROM gateway_topic_origins o JOIN events e ON e.id=o.event JOIN entries d ON d.id=e.entry_id WHERE o.event=?",
                        event,
                    ),
                    [
                        (
                            "omt_thread",
                            "om_root",
                            name,
                            peer["bot"],
                            peer["identity"]["tenant"],
                        )
                    ],
                )
                reply(
                    queue,
                    calls=[call("send-root", "send", {"text": "same native topic"})],
                )
                wire, queue = owner.wire(model)
                sent = json.loads(
                    next(
                        m["content"]
                        for m in wire["messages"]
                        if m.get("tool_call_id") == "send-root"
                    )
                )
                ctx.equal("original topic send confirms", sent["state"], "confirmed")
                planned = [
                    json.loads(p.read_bytes()) for p in folder.glob("route-plan-*.json")
                ]
                delivered = [
                    json.loads(p.read_bytes())
                    for p in folder.glob("route-dispatch-*.json")
                ]
                ctx.equal(
                    "one planned and one physical original part",
                    (len(planned), len(delivered)),
                    (1, 1),
                )
                for args in planned + delivered:
                    ctx.equal(
                        "both phases retain original topic route",
                        (
                            args["route"]["topic"],
                            args["route"]["topic_root"],
                            args["route"]["bot"],
                        ),
                        ("omt_thread", "om_root", "native-bot"),
                    )
                reply(queue)
                owner.finished(event)
                peer.update(params=params, event=event)
                events.append(event)
            ctx.check(
                "same native keys remain separate between tenants",
                len(set(events)) == 2,
            )
            peer = peers["topic-b"]
            folder = peer["folder"]
            race = submit(
                peer,
                {**peer["params"], "source": "deployment-race", "native_id": "om_race"},
            )["event"]
            _, queue = owner.wire(model)
            original = rows(
                "SELECT value FROM core_meta WHERE key=?", "plugin-approval:topic-b"
            )[0][0]
            before = rows(
                "SELECT count(*) FROM host_calls WHERE method='gateway.dispatch_part'"
            )
            (folder / "pause-route-plan").touch()
            reply(
                queue,
                calls=[call("deployment-race", "send", {"text": "original root"})],
            )
            owner.until(lambda: (folder / "route-plan-waiting.json").exists())
            edit(
                "UPDATE core_meta SET value=json_set(value,'$.deployment','replacement') WHERE key=?",
                "plugin-approval:topic-b",
            )
            (folder / "pause-route-plan").unlink()
            owner.until(
                lambda: rows(
                    "SELECT id FROM executions WHERE source=? AND state='held'", race
                )
            )
            ctx.equal(
                "deployment changed after planning opens no physical dispatch",
                rows(
                    "SELECT count(*) FROM host_calls WHERE method='gateway.dispatch_part'"
                ),
                before,
            )
            ctx.equal(
                "replacement never receives a native part",
                len(list(folder.glob("route-dispatch-*.json"))),
                1,
            )
            edit(
                "UPDATE core_meta SET value=? WHERE key=?",
                original,
                "plugin-approval:topic-b",
            )
            ctx.write(
                "native-topic-before-restart.json",
                {
                    name: {
                        "event": p["event"],
                        "bot": p["bot"],
                        "identity": p["identity"],
                    }
                    for name, p in peers.items()
                },
            )
        before = len(model.header_shapes)
        for peer in peers.values():
            (peer["folder"] / "ready.json").unlink()
        with owner.svc.running():
            for name, peer in peers.items():
                owner.until(lambda: (peer["folder"] / "ready.json").exists())
                ctx.equal(
                    "cold replay keeps the same event",
                    submit(peer, peer["params"])["event"],
                    peer["event"],
                )
            ctx.equal(
                "cold replay purchases no new model request",
                len(model.header_shapes),
                before,
            )
            peer = peers["topic-a"]
            original = rows(
                "SELECT value FROM core_meta WHERE key=?", "plugin-approval:topic-a"
            )[0][0]
            edit(
                "UPDATE core_meta SET value=json_set(value,'$.deployment','replacement') WHERE key=?",
                "plugin-approval:topic-a",
            )
            submit(peer, peer["params"], True)
            ctx.equal(
                "replacement does not rebind original evidence",
                rows(
                    "SELECT deployment FROM gateway_topic_origins WHERE event=?",
                    peer["event"],
                ),
                [(json.loads(original)["deployment"],)],
            )
            edit(
                "UPDATE core_meta SET value=? WHERE key=?",
                original,
                "plugin-approval:topic-a",
            )
            owner.op(
                "privacy",
                "erase",
                "erase-topic-origin",
                owner.actor,
                json.dumps([peer["event"]]),
            )
            ctx.equal(
                "forget removes original native root",
                rows(
                    "SELECT count(*) FROM gateway_topic_origins WHERE event=?",
                    peer["event"],
                ),
                [(0,)],
            )
            erased = submit(peer, peer["params"])
            ctx.check(
                "redelivery after forget remains erased",
                erased["erased"] and not erased["accepted"],
            )
            ctx.equal(
                "redelivery cannot recreate private native root",
                rows(
                    "SELECT count(*) FROM gateway_topic_origins WHERE event=?",
                    peer["event"],
                ),
                [(0,)],
            )
            ctx.equal(
                "other tenant root remains available",
                rows(
                    "SELECT topic_root FROM gateway_topic_origins WHERE event=?",
                    peers["topic-b"]["event"],
                ),
                [("om_root",)],
            )
    return {
        "status": "PASS",
        "synthetic_consumers": True,
        "paid_calls": 0,
        "canonical_topic": "omt_thread",
        "original_root": "om_root",
        "events": events,
    }
