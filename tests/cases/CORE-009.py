"""Two authenticated reverse sockets, scoped private entries, native mentions and typing leases."""

import importlib.util
import json
import hashlib
import shutil
import subprocess
import sys
import os
import sqlite3
import time
from contextlib import closing, ExitStack
from tests.harness.service import Service


def run(ctx):
    spec = importlib.util.spec_from_file_location(
        "peer", ctx.root / "tests/peers/onebot/peer.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    Peer = module.Peer
    owner = Service(ctx, ctx.working / "data")
    owner.command("init", "--memory-mode", "lexical-only")

    def op(*args, **kw):
        return owner.command("operator", *args, **kw)

    a, b, h = "10001", "10002", "20000"
    actor = "00000000-0000-4000-8000-000000000009"
    with owner.running():
        ctx.write("sqlite-build.json", op("database-info"))
        op("person", "create", actor)
        op(
            "account",
            "open",
            "qq",
            "local",
            h,
            "10",
            "--person",
            actor,
            "--start",
            "10",
        )
        stages = {
            bot: op("account", "open", "qq", "local", bot, "10", "--start", "10")[
                "assignment"
            ]
            for bot in (a, b)
        }
    with (owner.data / "yuki.toml").open("a") as config:
        config.write(
            f'\n[qq]\ndesignated_ingress="{a}"\nnamespace="local"\n[qq.groups]\n"800"="mention"\n'
        )
        for bot in (a, b):
            config.write(
                f'\n[[qq.bots]]\nself_id="{bot}"\nstage="{stages[bot]}"\ntoken_env="TEST_QQ_{bot}"\ntyping_private=true\n'
            )
    # Two independent synthetic deployments start at the same quiesced schema and identity snapshot.
    shared_data = ctx.working / "shared"
    shutil.copytree(owner.data, shared_data)
    samples = []

    def transmit(peer, payload, label, session, seq):
        payload["message_seq"] = seq
        raw = peer.send(payload)
        outfile = ctx.working / ("capture-" + str(len(samples)) + ".json")
        outfile.write_bytes(raw)
        samples.append(
            {
                "occurrence": label,
                "session": session,
                "file": outfile.name,
                "sha256": hashlib.sha256(raw).hexdigest(),
            }
        )

    env = {
        **os.environ,
        "TEST_QQ_" + a: "synthetic-A-secret",
        "TEST_QQ_" + b: "synthetic-B-secret",
    }

    def packet(bot, native, *, group=True):
        result = {
            "post_type": "message",
            "message_type": "group" if group else "private",
            "self_id": int(bot),
            "time": 100,
            "message_id": native,
            "user_id": int(h),
            "sender": {"nickname": "human"},
            "message": [
                {"type": "text", "data": {"text": "literal @10001 "}},
                {"type": "at", "data": {"qq": b, "name": "B"}},
                {"type": "at", "data": {"qq": b, "name": "B"}},
                {"type": "image", "data": {"file": "image-key"}},
                {"type": "reply", "data": {"id": 12}},
                {"type": "face", "data": {"id": 0}},
            ],
        }
        if group:
            result["group_id"] = 800
        return result

    def get_event(native, bot):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            with closing(
                sqlite3.connect(
                    (owner.data / "core.sqlite").as_uri() + "?mode=ro", uri=True
                )
            ) as db:
                row = db.execute(
                    "SELECT e.id,e.envelope,e.mentions,e.entry_id FROM event_locators l JOIN events e ON e.id=l.event_id WHERE l.receiver=? AND l.native_id=?",
                    (stages[bot], str(native)),
                ).fetchone()
            if row:
                return {
                    "id": row[0],
                    "frame": json.loads(row[1]),
                    "mentions": json.loads(row[2]),
                    "entry": row[3],
                }
            time.sleep(0.025)
        raise AssertionError("no committed event " + str(native))

    with owner.running(environment=env), ExitStack() as sockets:

        def connect(bot, token):
            p = Peer(owner.address, bot, token)
            sockets.callback(p.close)
            return p

        ctx.rejects(
            "wrong token rejected", ValueError, lambda: Peer(owner.address, a, "wrong")
        )
        ctx.rejects(
            "self header cannot impersonate another endpoint",
            ValueError,
            lambda: Peer(owner.address, a, "synthetic-A-secret", claimed=b),
        )
        pa = connect(a, "synthetic-A-secret")
        first = packet(a, 1)
        first["sender"]["card"] = ""
        transmit(pa, first, "first", "A1", 101)
        observed = get_event(1, a)
        ctx.equal(
            "native mentions retain duplicates and positions",
            [(m["position"], m["native"]) for m in observed["mentions"]],
            [(1, b), (2, b)],
        )
        ctx.check(
            "native mention codepoint ranges match the two visible markers",
            all(
                observed["frame"]["text"][m["text_start"] : m["text_end"]]
                == "[提及Agent]"
                for m in observed["mentions"]
            ),
        )
        ctx.equal(
            "empty card explicitly provided", observed["frame"]["sender"]["card"], ""
        )
        ctx.equal(
            "preferred mentioned bot stays offline",
            op("qq", "route", str(observed["id"])),
            {"bot": b, "online": False},
        )
        pb = connect(b, "synthetic-B-secret")
        transmit(pb, packet(b, 1002), "second", "B1", 102)
        transmit(pb, packet(b, 1001), "first", "B1", 101)
        transmit(pa, packet(a, 2), "second", "A1", 102)
        echo = {**packet(b, 3000), "post_type": "message_sent", "user_id": int(b)}
        transmit(pb, echo, "own-echo", "B1", 103)
        twin = get_event(2, a)
        ctx.check(
            "two same-second identical messages distinct", twin["id"] != observed["id"]
        )
        ctx.check("absent card remains absent", "card" not in twin["frame"]["sender"])
        pa.send(packet(a, 30, group=False))
        pb.send(packet(b, 40, group=False))
        dm_a, dm_b = get_event(30, a), get_event(40, b)
        ctx.check(
            "different receiving bots have distinct private entries",
            dm_a["entry"] != dm_b["entry"],
        )
        ctx.equal(
            "two group originals, no second admission for later other receiver",
            len(op("ingress", "pending")["pending"]),
            4,
        )
        ctx.equal(
            "B now routes to B",
            op("qq", "route", str(observed["id"])),
            {"bot": b, "online": True},
        )
        op("qq", "typing", str(observed["id"]), "group", "true")
        op("qq", "typing", str(dm_a["id"]), "turn-1", "true")
        start = pa.receive()
        ctx.equal(
            "private typing uses original user",
            start["params"],
            {"user_id": int(h), "event_type": 1},
        )
        op("qq", "typing", str(dm_a["id"]), "turn-2", "true")
        op("qq", "typing", str(dm_a["id"]), "turn-1", "false")
        op("qq", "typing", str(dm_a["id"]), "turn-2", "false")
        stop = pa.receive()
        ctx.equal(
            "last lease emits stop without waiting for start echo",
            stop["params"]["event_type"],
            0,
        )
        pa.send({"status": "ok", "retcode": 0, "echo": start["echo"], "data": None})
        pa.send({"status": "ok", "retcode": 0, "echo": stop["echo"], "data": None})
        empty = packet(a, 41, group=False)
        empty["message"] = []
        pa.send(empty)
        get_event(41, a)
        ctx.equal(
            "empty frame and echoes create no ordinary work",
            len(op("ingress", "pending")["pending"]),
            4,
        )
        ctx.equal(
            "only that connection matched the two original echoes",
            op("qq", "status", a)["matched_echoes"],
            2,
        )
        fake = packet(a, 99)
        fake["self_id"] = int(b)
        pa.send(fake)
        ctx.equal("spoofed frame closes the A socket", pa.receive(), None)
        replacement = connect(a, "synthetic-A-secret")
        transmit(replacement, first, "first", "A2", 101)
        replacement.send(packet(a, 3))
        get_event(3, a)
        ctx.check("B survives A reconnection", op("qq", "status", b)["online"])
    peer_file = ctx.root / "tests/peers/onebot/peer.py"
    synthetic_manifest = {
        "kind": "synthetic-OneBot-peer",
        "peer_sha256": hashlib.sha256(peer_file.read_bytes()).hexdigest(),
    }
    digest = (
        "sha256:"
        + hashlib.sha256(
            json.dumps(synthetic_manifest, sort_keys=True).encode()
        ).hexdigest()
    )
    evidence = {
        "kind": "target_capture",
        "target_image_digest": digest,
        "capture_image_digest": digest,
        "sequence_field": "message_seq",
        "observations": samples,
    }
    batch = ctx.working / "pairs.json"
    batch.write_text(json.dumps(evidence))
    decision = subprocess.check_output(
        [sys.executable, str(ctx.root / "baseline/qq-key/decision.py"), str(batch)]
    )
    ctx.equal(
        "paired test target qualifies its own group sequence",
        json.loads(decision)["group"]["mode"],
        "scoped_shared_key",
    )
    (shared_data / "decision.json").write_bytes(decision)
    with (shared_data / "yuki.toml").open("a") as config:
        config.write(
            f'\n[qq.shared_decision]\nfile="decision.json"\nsha256="{hashlib.sha256(decision).hexdigest()}"\ntarget_image_digest="{digest}"\n'
        )
    owner = Service(ctx, shared_data)
    with owner.running(environment=env), ExitStack() as sockets:
        pa = connect(a, "synthetic-A-secret")
        pb = connect(b, "synthetic-B-secret")
        pa.send({**packet(a, 210), "message_seq": 801})
        from_a = get_event(210, a)
        pb.send({**packet(b, 720), "message_seq": 801})
        from_b = get_event(720, b)
        ctx.equal(
            "two real receiving locators refer to one proved shared source",
            from_a["id"],
            from_b["id"],
        )
        ctx.equal(
            "shared case has one durable admission",
            op("ingress", "pending")["pending"],
            [from_a["id"]],
        )
        ctx.equal(
            "shared late receipt cannot replace explicitly mentioned B",
            op("qq", "route", str(from_a["id"]))["bot"],
            b,
        )
    return {
        "group": observed,
        "second_group": twin,
        "private": [dm_a, dm_b],
        "typing": [start, stop],
        "synthetic_shared": {
            "peer": synthetic_manifest,
            "decision": json.loads(decision),
            "event": from_a["id"],
            "native_locators": [210, 720],
        },
        "real_QQ": "not_sampled",
    }
