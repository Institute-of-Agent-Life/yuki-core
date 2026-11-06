from pathlib import Path

"""Drive an actual Rust Main over two authenticated OneBot sessions; stop after the second write."""

import importlib.util
import json
import os
import io
import hashlib
import sqlite3
import time
from contextlib import closing, ExitStack
from concurrent.futures import ThreadPoolExecutor
from tests.harness.service import Service, previous_schema
from tests.harness.crash import kill_writer
from tests.peers.chat.recorder import Recorder


def run(ctx):
    service = Service(ctx, ctx.working / "owner")
    previous_schema(ctx, service.data, 9)
    ctx.check(
        "upgrade original Main data", service.command("migrate")["schema_version"] >= 10
    )
    (service.data / "SOUL.md").write_text("", encoding="utf-8")
    actor = "00000000-0000-4000-8000-000000000013"
    bot = "10001"
    peer_user = "20000"
    spec = importlib.util.spec_from_file_location(
        "onebot_peer", ctx.root / "tests/peers/onebot/peer.py"
    )
    ws = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(ws)

    frame127 = object.__new__(ws.Peer)
    frame127.reader = io.BytesIO(b"\x81\x7e\x00\x7f" + json.dumps("x" * 125).encode())
    ctx.equal(
        "127 byte payload uses only the 16-bit extended length",
        frame127.receive(),
        "x" * 125,
    )

    def op(*args, ok=True):
        return service.command("operator", *map(str, args), accepted=ok)

    def db(sql, *args):
        with closing(
            sqlite3.connect(
                (service.data / "core.sqlite").as_uri() + "?mode=ro", uri=True
            )
        ) as con:
            return con.execute(sql, args).fetchall()

    def until(fn):
        end = time.monotonic() + 8
        while time.monotonic() < end:
            if result := fn():
                return result
            time.sleep(0.03)
        raise AssertionError("durable outbox state timed out")

    with service.running():
        ctx.write("sqlite-build.json", op("database-info"))
        op("person", "create", actor)
        op("privacy", "owner", "fixture-image", "true")
        op(
            "social",
            "select-images",
            "fixture-image",
            hashlib.sha256(b"synthetic installed provider").hexdigest(),
        )
        account = op(
            "account",
            "open",
            "qq",
            "lab",
            peer_user,
            1,
            "--person",
            actor,
            "--start",
            1,
        )
        stage = op("account", "open", "qq", "lab", bot, 1, "--start", 1)["assignment"]
        second_bot = "10002"
        stage_b = op("account", "open", "qq", "lab", second_bot, 1, "--start", 1)[
            "assignment"
        ]
        others = []
        for ix, native in enumerate(("20001", "20002"), 1):
            person = f"00000000-0000-4000-8000-{(0x1300 + ix):012x}"
            op("person", "create", person)
            others.append(
                op(
                    "account",
                    "open",
                    "qq",
                    "lab",
                    native,
                    1,
                    "--person",
                    person,
                    "--start",
                    1,
                )
            )
        g = op("entry", "group", '["qq","lab","700"]')["entry"]

        for n, (handle, label, occurred) in enumerate(
            zip(others, ("AlphaLong", "Alpha"), (3000, 1000))
        ):
            op(
                "ingress",
                "record",
                json.dumps(
                    {
                        "source": "seen-" + str(n),
                        "account": handle["account"],
                        "bot_stage": stage,
                        "space": '["qq","lab","700"]',
                        "occurred": occurred,
                        "observed": occurred,
                        "kind": "message",
                        "segments": [{"type": "text", "text": "fixture member"}],
                        "sender": {"nickname": label, "card": label},
                    }
                ),
            )

    def packet(mid, text, *, sender=peer_user, kind="message"):
        return {
            "post_type": kind,
            "self_id": int(bot),
            "message_type": "group",
            "user_id": int(sender),
            "group_id": 700,
            "message_id": mid,
            "time": 100,
            "sender": {"card": "Alpha", "nickname": "Alpha"},
            "message": [{"type": "text", "data": {"text": text}}],
        }

    env = {**os.environ, "YUKI_SOCIAL_FIXTURE": "fixture-socket-secret"}

    def ack(peer, action, data):
        peer.send({"echo": action["echo"], "status": "ok", "retcode": 0, "data": data})

    with Recorder() as http:
        incoming = service.data / "media-inputs" / "managed-staging"
        incoming.mkdir(parents=True, exist_ok=True)
        (incoming / "file-fixture").write_bytes(b"original file bytes")
        with (service.data / "yuki.toml").open("a") as cfg:
            cfg.write(
                "\n[media]\ngateway_transfer_directory="
                + json.dumps(str(service.data / "media-transfer"))
                + "\n"
            )
            cfg.write(
                f'\n[main]\nendpoint="{http.url}"\nmodel="fixture"\nname="Yuki"\nmerge_ms=0\nroot_slots=4\nmax_input_units=100000\nmax_output=128\n[qq]\nnamespace="lab"\ndesignated_ingress="{bot}"\n[qq.groups]\n"700"="open"\n[[qq.bots]]\nself_id="{bot}"\nstage="{stage}"\ntoken_env="YUKI_SOCIAL_FIXTURE"\n'
            )
            cfg.write(
                f'\n[[qq.bots]]\nself_id="{second_bot}"\nstage="{stage_b}"\ntoken_env="YUKI_SOCIAL_SECOND"\n'
            )
        env["YUKI_SOCIAL_SECOND"] = "fixture-second-secret"
        with service.running(environment=env), ExitStack() as sockets:
            peer = ws.Peer(service.address, bot, env["YUKI_SOCIAL_FIXTURE"])
            sockets.callback(peer.close)
            peer_b = ws.Peer(service.address, second_bot, env["YUKI_SOCIAL_SECOND"])
            sockets.callback(peer_b.close)
            trigger = packet(1, "ask for delivery")
            trigger["message"].append({"type": "forward", "data": {"id": "forward-id"}})
            peer.send(trigger)
            wire, q = http.receive()
            args = {
                "text": "first @Alpha / @AlphaLong\nsecond @"
                + "{account:"
                + account["account"]
                + "}\nthird",
                "emoji": {"goal": "cheer"},
            }
            tool = {
                "id": "T",
                "type": "function",
                "function": {"name": "send", "arguments": json.dumps(args)},
            }
            q.put(
                (
                    {
                        "choices": [
                            {
                                "finish_reason": "tool_calls",
                                "message": {
                                    "role": "assistant",
                                    "content": None,
                                    "tool_calls": [tool],
                                },
                            }
                        ]
                    },
                    0,
                )
            )
            part1 = peer.receive()
            ctx.equal("real first native send", part1["action"], "send_group_msg")
            ctx.check(
                "real mention reconstructed on first part",
                any(
                    x["type"] == "at" and str(x["data"]["qq"]) == peer_user
                    for x in part1["params"]["message"]
                ),
            )
            ctx.equal(
                "longest and same-name recent speaker come from original group",
                [
                    str(x["data"]["qq"])
                    for x in part1["params"]["message"]
                    if x["type"] == "at"
                ],
                ["20000", "20001"],
            )
            ack(peer_b, part1, {"message_id": 9999})
            ids = until(lambda: db("SELECT namespace,operation_id FROM social_plans"))
            exec_id, op_id = ids[0]
            image = op("social", "inspect", exec_id, op_id, actor)["image"]
            ctx.check(
                "image intent persisted before first native ACK",
                image["actor"] == actor
                and image["goal"] == "cheer"
                and image["state"] == "pending",
            )
            peer.send(packet(701, "outbound first", sender=bot, kind="message_sent"))
            until(lambda: db("SELECT id FROM events WHERE source_key='echo:701'"))
            ctx.equal(
                "early echo alone did not settle first part",
                db("SELECT state FROM social_parts WHERE ordinal=0"),
                [("dispatched",)],
            )
            ack(peer, part1, {"message_id": 701})
            part2 = peer.receive()
            ctx.check(
                "explicit UUID account on second native part",
                any(x["type"] == "at" for x in part2["params"]["message"]),
            )
            peer.close()
            result = until(
                lambda: (
                    r
                    if (r := op("social", "inspect", exec_id, op_id, actor))["parts"][
                        1
                    ]["state"]
                    == "unknown"
                    else None
                )
            )
            ctx.equal(
                "three original statuses",
                [x["state"] for x in result["parts"]],
                ["confirmed", "unknown", "prepared"],
            )
            ctx.equal(
                "ack attached original observed echo",
                result["parts"][0]["locator"],
                "701",
            )
            ctx.equal(
                "echo never claimed a human trigger",
                db(
                    "SELECT count(*) FROM accepted_sources a JOIN events e ON e.id=a.event_id WHERE e.source_key LIKE 'echo:%'"
                ),
                [(0,)],
            )
        with service.running(environment=env), ExitStack() as sockets:
            peer = ws.Peer(service.address, bot, env["YUKI_SOCIAL_FIXTURE"])
            sockets.callback(peer.close)
            replay = op("social", "deliver", exec_id, op_id, actor)
            ctx.equal(
                "recovery reads original unknown without resending",
                replay["parts"],
                result["parts"],
            )
            with ThreadPoolExecutor(1) as pool:
                f = pool.submit(op, "social", "recall", exec_id, op_id, actor, 0)
                native = peer.receive()
                ctx.equal(
                    "retract exact original BOT locator",
                    native["params"],
                    {"message_id": 701},
                )
                ack(peer, native, {})
                ctx.check(
                    "original recall confirmed", f.result()["state"] == "confirmed"
                )
                op("social", "recall", exec_id, op_id, "unrelated", 0, ok=False)
                f = pool.submit(op, "social", "forward", exec_id, actor, "forward-id")
                read = peer.receive()
                ctx.equal("actual forward protocol", read["action"], "get_forward_msg")
                ack(peer, read, {"messages": [{"content": "fixture"}]})
                ctx.check(
                    "records original forward result",
                    f.result()["data"]["messages"][0]["content"] == "fixture",
                )
                f = pool.submit(op, "social", "member", exec_id, actor, int(peer_user))
                read = peer.receive()
                ctx.equal(
                    "group-bound membership read", read["params"]["group_id"], 700
                )
                ack(peer, read, {"user_id": int(peer_user), "group_id": 700})
                ctx.equal(
                    "member original target",
                    f.result()["data"]["user_id"],
                    int(peer_user),
                )
            op(
                "effect",
                "intent",
                exec_id,
                "already-issued",
                actor,
                "external",
                "send",
                json.dumps({"text": "do not send again"}),
            )
            op("effect", "dispatch", exec_id, "already-issued", actor)
            op("social", "deliver", exec_id, "already-issued", actor, ok=False)
            ctx.equal(
                "different binding cannot acquire an issued operation",
                db(
                    "SELECT count(*) FROM social_plans WHERE operation_id='already-issued'"
                ),
                [(0,)],
            )
            op("social", "forward", exec_id, actor, "unseen-forward", ok=False)
            op("privacy", "owner", "fixture-image", "false")
            op(
                "effect",
                "intent",
                exec_id,
                "plain-with-goal",
                actor,
                "external",
                "send",
                json.dumps(
                    {
                        "text": "safe正文<yuki-state>private tail",
                        "emoji": {"goal": "unavailable"},
                    }
                ),
            )
            with ThreadPoolExecutor(1) as pool:
                plain = pool.submit(
                    op, "social", "deliver", exec_id, "plain-with-goal", actor
                )
                body = peer.receive()
                ctx.equal(
                    "state tail does not leave original wire",
                    body["params"]["message"],
                    [{"type": "text", "data": {"text": "safe正文"}}],
                )
                ack(peer, body, {"message_id": 713})
                ctx.check(
                    "disabled image owner produces only text",
                    plain.result()["image"] is None,
                )
            op(
                "effect",
                "intent",
                exec_id,
                "unavailable-image",
                actor,
                "external",
                "send",
                json.dumps({"emoji": {"goal": "absent"}}),
            )
            op("social", "deliver", exec_id, "unavailable-image", actor, ok=False)
            op("privacy", "owner", "fixture-image", "true")
            op(
                "effect",
                "intent",
                exec_id,
                "text-image",
                actor,
                "external",
                "send",
                json.dumps(
                    {"text": "crash before send", "emoji": {"goal": "original picture"}}
                ),
            )
            op(
                "social",
                "bind-media",
                exec_id,
                actor,
                "frozen-asset",
                "file",
                "report.txt",
                "managed-staging/file-fixture",
            )
            op(
                "effect",
                "intent",
                exec_id,
                "file-caption",
                actor,
                "external",
                "send",
                json.dumps(
                    {
                        "text": "file caption",
                        "artifact_id": "frozen-asset",
                        "attachment_kind": "file",
                    }
                ),
            )
            with ThreadPoolExecutor(1) as pool:
                fut = pool.submit(
                    op, "social", "deliver", exec_id, "file-caption", actor
                )
                upload = peer.receive()
                ctx.equal(
                    "file uses actual QQ upload", upload["action"], "upload_group_file"
                )
                ctx.equal(
                    "receiver sees the immutable staged bytes",
                    Path(upload["params"]["file"]).read_bytes(),
                    b"original file bytes",
                )
                ack(peer, upload, {})
                caption = peer.receive()
                ctx.equal(
                    "caption is a separate original send",
                    caption["action"],
                    "send_group_msg",
                )
                ack(peer, caption, {"message_id": 710})
                file_result = fut.result()
                ctx.equal(
                    "successful file without message locator remains identifiable",
                    [(x["state"], x["locator"]) for x in file_result["parts"]],
                    [("confirmed", None), ("confirmed", "710")],
                )
            with ThreadPoolExecutor(1) as pool:
                pending = pool.submit(
                    op, "social", "recall", exec_id, "file-caption", actor, 1
                )
                retract = peer.receive()
                ctx.equal(
                    "lost retract response keeps original message",
                    retract["params"],
                    {"message_id": 710},
                )
                ctx.equal(
                    "timeout retract is unknown", pending.result()["state"], "unknown"
                )
                ack(peer, retract, {})
                until(
                    lambda: db(
                        "SELECT recalled FROM social_outbound WHERE locator='710'"
                    )
                    == [(1,)]
                )
                ctx.equal(
                    "late receipt confirms original recall without another request",
                    op("social", "recall", exec_id, "file-caption", actor, 1)["state"],
                    "confirmed",
                )
            op(
                "effect",
                "intent",
                exec_id,
                "collision",
                actor,
                "external",
                "send",
                json.dumps({"text": "conflicting locator"}),
            )
            with ThreadPoolExecutor(1) as pool:
                collision = pool.submit(
                    op, "social", "deliver", exec_id, "collision", actor
                )
                outgoing = peer.receive()
                ack(peer, outgoing, {"message_id": 701})
                conflict = collision.result()["parts"][0]
                ctx.check(
                    "conflicting native locator cannot consume another delivery",
                    conflict["state"] == "unknown"
                    and conflict["locator"] is None
                    and conflict["error"] == "locator_collision",
                )
            op(
                "effect",
                "intent",
                exec_id,
                "late",
                actor,
                "external",
                "send",
                json.dumps({"text": "late ACK"}),
            )
            with ThreadPoolExecutor(1) as pool:
                waiting = pool.submit(op, "social", "deliver", exec_id, "late", actor)
                delayed = peer.receive()
                ctx.equal(
                    "live socket timeout records unknown",
                    waiting.result()["parts"][0]["state"],
                    "unknown",
                )
                ack(peer, delayed, {"message_id": 711})
                late_result = until(
                    lambda: (
                        r
                        if (r := op("social", "inspect", exec_id, "late", actor))[
                            "state"
                        ]
                        == "confirmed"
                        else None
                    )
                )
                ctx.equal(
                    "late original ACK records positive evidence after unknown",
                    db(
                        "SELECT state FROM social_part_receipts WHERE namespace=? AND child=? ORDER BY rowid",
                        exec_id,
                        late_result["parts"][0]["child"],
                    ),
                    [("unknown",), ("confirmed",)],
                )
            op(
                "effect",
                "intent",
                exec_id,
                "image-only",
                actor,
                "external",
                "send",
                json.dumps({"emoji": {"goal": "wave"}}),
            )
        kill_writer(
            ctx,
            service.data,
            "social_crash_child",
            "after",
            {"YUKI_SOCIAL_TEST": json.dumps([exec_id, "image-only", actor])},
        )
        kill_writer(
            ctx,
            service.data,
            "social_crash_child",
            "after",
            {"YUKI_SOCIAL_TEST": json.dumps([exec_id, "text-image", actor])},
            sample="with-text",
        )
        with service.running(environment=env), ExitStack() as sockets:
            only = op("social", "inspect", exec_id, "image-only", actor)
            ctx.check(
                "image-only durable before any transport",
                only["image"]["goal"] == "wave" and not only["parts"],
            )
            interrupted = op("social", "inspect", exec_id, "text-image", actor)
            ctx.check(
                "first pre-I/O grant and selected image survive together",
                interrupted["parts"][0]["state"] == "unknown"
                and interrupted["image"]["goal"] == "original picture"
                and interrupted["image"]["state"] == "pending",
            )
            peer = ws.Peer(service.address, bot, env["YUKI_SOCIAL_FIXTURE"])
            sockets.callback(peer.close)
            op(
                "effect",
                "intent",
                exec_id,
                "transfer",
                actor,
                "external",
                "send",
                json.dumps({"text": "sent before transfer"}),
            )
            replacement = "00000000-0000-4000-8000-000000000044"
            op("person", "create", replacement)
            with ThreadPoolExecutor(1) as pool:
                delivery = pool.submit(
                    op, "social", "deliver", exec_id, "transfer", actor
                )
                physical = peer.receive()
                op(
                    "account",
                    "transfer",
                    account["assignment"],
                    200000,
                    200000,
                    "--person",
                    replacement,
                )
                ack(peer, physical, {"message_id": 712})
                ctx.equal(
                    "ACK retains dispatch-time actor after transfer",
                    db("SELECT original_actor FROM social_outbound WHERE locator='712'")
                    or until(
                        lambda: db(
                            "SELECT original_actor FROM social_outbound WHERE locator='712'"
                        )
                    ),
                    [(actor,)],
                )
                transferred = delivery.result()
                ctx.equal(
                    "receipt committed for original stage after authority changed",
                    transferred["parts"][0]["state"],
                    "confirmed",
                )
            op("social", "recall", exec_id, "transfer", replacement, 0, ok=False)
            claimed = op("effect", "read", exec_id, "image-only", actor)["result"]
            ctx.check(
                "image acceptance does not claim physical delivery",
                claimed["image_intent_accepted"]
                and not claimed["image_delivered"]
                and not claimed["public_delivery_confirmed"],
            )
            ctx.equal(
                "selected image provider revision is frozen",
                interrupted["image"]["owner"],
                "fixture-image",
            )
            ctx.check(
                "no new image selected after provider disabled",
                db(
                    "SELECT count(*) FROM social_images WHERE operation_id='plain-with-goal'"
                )
                == [(0,)],
            )
            original_source = db("SELECT source FROM executions WHERE id=?", exec_id)[
                0
            ][0]
            op("privacy", "erase", "erase-social", actor, json.dumps([original_source]))
            ctx.equal(
                "correlated outbound and peer echo bodies scrubbed together",
                db(
                    "SELECT o.body,e.envelope FROM social_outbound o JOIN events e ON e.id=o.echo_event WHERE o.locator='701'"
                ),
                [("{}", "{}")],
            )
            peer.send(packet(701, "outbound first", sender=bot, kind="message_sent"))
            ctx.equal(
                "associated observations stay erased",
                db(
                    "SELECT count(*) FROM social_outbound WHERE body!='{}' AND namespace=?",
                    exec_id,
                ),
                [(0,)],
            )

    from tests.harness.send_entry import run as send_entry
    ctx.write("canonical-send-entry.json",send_entry(ctx))
    from tests.harness.recall_wire import run as recall
    ctx.write("original-recall-wire.json",recall(ctx))
    from tests.harness.observed_members import run as members
    member_result=members(ctx)
    ctx.write("observed-members.json",member_result)
    return {
        "wire": wire,
        "original_entry": g,
        "first_action": part1,
        "lost_action": part2,
        "original_receipt": result,
        "recovery": replay,
        "image_only": only,
        "first_grant_crash": interrupted,
        "file_caption": file_result,
        "late_original_ack": late_result,
        "transfer_at_ack": transferred,
    }
