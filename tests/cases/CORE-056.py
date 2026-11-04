"""Use authenticated independent QQ peers. Probe outside the writer, revoke authority, then reply."""

import importlib.util
import os
import sqlite3
import time
from contextlib import ExitStack, closing
from tests.harness.service import Service, previous_schema


def run(ctx):
    spec = importlib.util.spec_from_file_location(
        "peer", ctx.root / "tests/peers/onebot/peer.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    service = Service(ctx, ctx.working / "data")
    previous_schema(ctx, service.data, 7)
    ctx.check(
        "v7 source snapshot migrates", service.command("migrate")["schema_version"] >= 8
    )
    admin = "00000000-0000-4000-8000-000000000056"
    a, b, h = "10001", "10002", "20000"
    cmd = "/ai admin group current enable"

    def op(*args):
        return service.command("operator", *map(str, args))

    with service.running():
        ctx.write("sqlite-build.json", op("database-info"))
        op("person", "create", admin)
        op("person", "set-admin", admin, "true")
        op("account", "open", "qq", "lab", h, 1, "--person", admin)
        stages = {
            i: op("account", "open", "qq", "lab", i, 1, "--start", 1)["assignment"]
            for i in (a, b)
        }
    with (service.data / "yuki.toml").open("a") as config:
        config.write(
            f'\n[qq]\nnamespace="lab"\ndesignated_ingress="{a}"\n[qq.groups]\n"700"="paused"\n'
        )
        for i in (a, b):
            config.write(
                f'\n[[qq.bots]]\nself_id="{i}"\nstage="{stages[i]}"\ntoken_env="SECRET_{i}"\n'
            )
    env = {**os.environ, **{"SECRET_" + i: "fixture-" + i for i in (a, b)}}
    transcript = []

    def event(bot, group, mid, text=cmd, private=False):
        event = {
            "self_id": int(bot),
            "user_id": int(h),
            "message_type": "private" if private else "group",
            "post_type": "message",
            "group_id": int(group),
            "time": 100,
            "message_id": mid,
            "message": text,
        }
        return event

    def send(peer, packet):
        peer.send(packet)
        transcript.append({"incoming": packet})

    def row(sql, args=()):
        with closing(
            sqlite3.connect(
                (service.data / "core.sqlite").as_uri() + "?mode=ro", uri=True
            )
        ) as db:
            return db.execute(sql, args).fetchall()

    def probe(peer, bot, group, ok=True):
        action = peer.receive()
        ctx.equal(
            "probe actual native membership " + str(group),
            action["action"],
            "get_group_member_info",
        )
        ctx.equal(
            "probe is for exact bot and group",
            action["params"],
            {"group_id": int(group), "user_id": int(bot), "no_cache": True},
        )
        resp = {
            "echo": action["echo"],
            "status": "ok",
            "retcode": 0,
            "data": {"group_id": int(group), "user_id": int(bot)} if ok else {},
        }
        transcript.append({"probe": action, "response": resp})
        peer.send(resp)

    def expect(peer, code):
        action = peer.receive()
        transcript.append({"reply": action})
        ctx.equal(
            "control response to originating native group",
            action["action"],
            "send_group_msg",
        )
        ctx.check("control outcome " + code, code in action["params"]["message"])

    with service.running(environment=env), ExitStack() as stack:

        def connect(bot):
            peer = module.Peer(service.address, bot, env["SECRET_" + bot])
            stack.callback(peer.close)
            return peer

        pa = connect(a)
        # Private body/obsolete alias cannot create a canonical group.
        send(pa, event(a, 900, 1, private=True))
        send(pa, event(a, 901, 2, "/ai on"))
        send(pa, event(a, 901, 12, "/ai on", private=True))
        send(pa, event(a, 700, 3, "ordinary paused data"))
        send(pa, event(a, 700, 4))
        probe(pa, a, 700)
        expect(pa, "enabled")
        ctx.equal(
            "paused group now uses receiving stage",
            row("SELECT stage FROM recovered_groups WHERE native_group='700'"),
            [(stages[a],)],
        )
        send(pa, event(a, 700, 4))
        expect(pa, "enabled")
        ctx.equal(
            "reply loss or redelivery cannot add a second control",
            row("SELECT count(*) FROM group_recovery_receipts"),
            [(1,)],
        )
        pb = connect(b)
        send(pb, event(b, 700, 4))
        # Existing healthy assigned route belongs to A, and B cannot send through it.
        probe(pa, a, 700)
        send(pb, event(b, 904, 6))
        probe(pa, a, 904)
        probe(pb, b, 904)
        expect(pb, "ambiguous")
        send(pa, event(a, 905, 7))
        probe(pa, a, 905)
        probe(pb, b, 905, False)
        # Deliberately disconnect without reading the reply after the database commits.
        deadline = time.monotonic() + 4
        while time.monotonic() < deadline and not row(
            "SELECT native_group FROM group_recovery_receipts WHERE native_group='905'"
        ):
            time.sleep(0.02)
        ctx.check(
            "source committed before dropped control reply",
            bool(
                row(
                    "SELECT native_group FROM group_recovery_receipts WHERE native_group='905'"
                )
            ),
        )
        pa.close()
        pa = connect(a)
        # The authorized actor is revoked while the independently authenticated probe is pending.
        send(pa, event(a, 906, 8))
        pending = pa.receive()
        ctx.equal(
            "read-only probe started before revocation",
            pending["action"],
            "get_group_member_info",
        )
        op("person", "set-admin", admin, "false")
        pa.send(
            {
                "echo": pending["echo"],
                "status": "ok",
                "retcode": 0,
                "data": {"group_id": 906, "user_id": int(a)},
            }
        )
        probe(pb, b, 906, False)
        expect(pa, "denied")
        op("person", "set-admin", admin, "true")
        send(pa, event(a, 907, 9))
        old_probe = pa.receive()
        ctx.equal(
            "pending proof belongs to old connection",
            old_probe["action"],
            "get_group_member_info",
        )
        new_a = connect(a)
        probe(pb, b, 907, False)
        send(new_a, event(a, 700, 10, "ordinary resumed group"))
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline and not row(
            "SELECT id FROM events WHERE source_key='10'"
        ):
            time.sleep(0.02)
        ctx.check(
            "ordinary source after recovery admitted",
            bool(row("SELECT id FROM events WHERE source_key='10'")),
        )
        ctx.equal(
            "spoof, revocation and peer replacement leave no route",
            row("SELECT native_group FROM recovered_groups ORDER BY native_group"),
            [("700",), ("905",)],
        )
        ctx.check(
            "no control or paused data persisted to Main",
            not row(
                "SELECT id FROM events WHERE source_key IN ('1','2','3','4','6','7','8','9')"
            ),
        )
        ctx.equal(
            "no Work execution was claimed",
            row("SELECT count(*) FROM executions"),
            [(0,)],
        )
        receipts = row(
            "SELECT native_group,native_message,person,stage,before_state,after_state FROM group_recovery_receipts ORDER BY native_group"
        )
    with service.running(environment=env), ExitStack() as stack:
        pa = module.Peer(service.address, a, env["SECRET_" + a])
        stack.callback(pa.close)
        send(pa, event(a, 905, 7))
        expect(pa, "enabled")
        ctx.equal(
            "restart replay original successful receipts",
            row("SELECT count(*) FROM group_recovery_receipts"),
            [(2,)],
        )
    return {
        "before": {"700": "paused", "905": "unregistered"},
        "after": receipts,
        "wire": transcript,
        "old_alias": "/ai on",
        "domains": ["native-qq"],
    }
