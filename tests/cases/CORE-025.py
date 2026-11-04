"""Bound media handles go from actual QQ input to an explicitly visual normal request or immutable upload."""

import base64
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import select
import zlib
import struct
from contextlib import closing
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer


def png():
    def chunk(tag, body):
        return (
            struct.pack(">I", len(body))
            + tag
            + body
            + struct.pack(">I", zlib.crc32(tag + body) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(b"\x00\x90\x40\x10"))
        + chunk(b"IEND", b"")
    )


def pair(body, key):
    return json.loads(
        next(m["content"] for m in body["messages"] if m.get("tool_call_id") == key)
    )


def configuration(o, http, vision, protocol="chat"):
    with (o.svc.data / "yuki.toml").open("a") as f:
        f.write(
            "\n[media]\ngateway_transfer_directory="
            + json.dumps(str(o.svc.data / "media-transfer"))
            + "\n"
        )
        f.write(
            f'\n[main]\nprotocol="{protocol}"\nendpoint="{http.url}"\nmodel="fixture"\nname="Yuki"\nmax_output=256\nroot_slots=12\nmax_input_units=1400000\nmerge_ms=1\nvision_input={str(vision).lower()}\n[qq]\nnamespace="compact"\ndesignated_ingress="100"\n[[qq.bots]]\nself_id="100"\nstage="{o.bot["assignment"]}"\ntoken_env="YUKI_MEDIA_FIXTURE"\n'
        )


def frame(mid, segments):
    return {
        "self_id": 100,
        "user_id": 200,
        "post_type": "message",
        "message_type": "private",
        "sub_type": "friend",
        "message_id": mid,
        "time": 100,
        "message": segments,
    }


def ack(peer, action, data):
    peer.send({"echo": action["echo"], "status": "ok", "retcode": 0, "data": data})


def profile_cases(ctx, env, original_png):
    from time import sleep

    encoded = base64.b64encode(original_png).decode()
    observations = {}
    for visual in (False, True):
        protocol = "anthropic_messages" if visual else "chat"
        o = Owner(ctx, "media-" + protocol, 20)
        with Recorder("/v1/messages" if visual else "/v1/chat/completions") as http:
            configuration(o, http, visual, protocol)
            with o.svc.running(environment=env), closing(
                Peer(o.svc.address, "100", env["YUKI_MEDIA_FIXTURE"])
            ) as peer:
                segments = [
                    {"type": "text", "data": {"text": "inspect"}},
                    {"type": "image", "data": {"file": "original-visual"}},
                ]
                peer.send(frame(771, segments))
                _, q = o.wire(http)
                event = o.rows("SELECT event_id FROM event_locators WHERE native_id='771'")[0][0]
                args = {"event_id": event, "attachment_index": 0, "question": "inspect"}
                def messages(blocks, stop):
                    return ({"type":"message", "role":"assistant", "id":"media-profile",
                        "content":blocks, "stop_reason":stop,
                        "usage":{"input_tokens":12,"output_tokens":3}}, 0)
                if visual:
                    tool = {"type":"tool_use","id":"picture","name":"inspect_conversation_attachment","input":args}
                    q.put(messages([tool], "tool_use"))
                    action = peer.receive()
                    ctx.equal("Messages resolves image on original receiver", action["action"], "get_image")
                    ack(peer, action, {"base64":encoded})
                    body, q = o.wire(http)
                    blocks = body["messages"][-1]["content"]
                    ctx.equal("paired Messages tool_result precedes native user image",
                        [b["type"] for b in blocks], ["tool_result","image"])
                    ctx.equal("Messages embeds exact original bytes in typed block",
                        blocks[-1]["source"], {"type":"base64","media_type":"image/png","data":encoded})
                    ctx.write("paid-messages-image.json", body)
                    q.put(messages([{"type":"text","text":"done"}],"end_turn"))
                else:
                    reply(q, calls=[call("picture","inspect_conversation_attachment",args)])
                    body, q = o.wire(http)
                    receipt = pair(body, "picture")["result"]
                    ctx.equal("without vision the actual result stays unread",
                        (receipt["state"], receipt["reason"]), ("unread","profile_has_no_vision"))
                    ctx.check("no image fetch, image bytes or extra paid profile",
                        not select.select([peer.socket],[],[],0.1)[0] and encoded not in str(body))
                    reply(q, "done")
                o.finished(event)
                ctx.equal("only two normal model requests", o.rows("SELECT COUNT(*) FROM model_attempts"), [(2,)])
                observations[protocol] = {"attempts":2, "material":"native_image" if visual else "unread"}
                if visual:
                    peer.send(frame(772, segments))
                    _, q = o.wire(http)
                    lost = o.rows("SELECT event_id FROM event_locators WHERE native_id='772'")[0][0]
                    tool = dict(tool, input=dict(args,event_id=lost))
                    q.put(messages([tool],"tool_use"))
                    lookup = peer.receive()
                    ctx.equal("second native acquire in flight", lookup["action"], "get_image")
                    o.op("privacy","erase","erase-during-fetch",o.actor,json.dumps([lost]))
                    ack(peer, lookup, {"base64":base64.b64encode(b"erased-image-bytes").decode()})
                    sleep(0.25)
                    ctx.equal("late QQ bytes cannot revive forgotten source", o.rows(
                        "SELECT state,digest,origin FROM media_sources WHERE event=?", lost), [("erased",None,None)])
                    ctx.equal("no saved object or paid continuation after erase",
                        (o.rows("SELECT count(*) FROM media_objects WHERE bytes=?",b"erased-image-bytes"),
                         o.rows("SELECT COUNT(*) FROM model_attempts")), ([(0,)],[(3,)]))
                    observations["erased_while_receiving"] = lost
    return observations


def run(ctx):
    gold = json.loads((ctx.root / "fixtures/media/v4-index.json").read_bytes())
    ctx.equal(
        "fixed v4 excludes text, mention and face but reserves unsupported slots",
        [(r[0], r[1]) for r in gold["direct_positions"]],
        [(1, 3), (2, 5), (4, 7)],
    )
    o = Owner(ctx, "media", 20)
    original_png = png()
    image_sha = hashlib.sha256(original_png).hexdigest()
    inputs = o.svc.data / "media-inputs"
    outside = o.svc.data / "outside"
    outside.mkdir()
    (outside / "private").write_text("OUTSIDE-FILE-CANARY")
    (inputs / "report.txt").write_bytes(b"ORIGINAL version")
    (inputs / "incoming.txt").write_text("source file content", encoding="utf-8")
    if os.name == "nt":
        subprocess.run(
            ["cmd", "/d", "/c", "mklink", "/J", str(inputs / "escape"), str(outside)],
            check=True,
            capture_output=True,
        )
    else:
        (inputs / "escape").symlink_to(outside, target_is_directory=True)
    (inputs / "hard").hardlink_to(outside / "private")
    env = {**os.environ, "YUKI_MEDIA_FIXTURE": "fixture-media"}
    with Recorder() as http:
        configuration(o, http, True)
        with (
            o.svc.running(environment=env),
            closing(Peer(o.svc.address, "100", env["YUKI_MEDIA_FIXTURE"])) as peer,
        ):
            packet = frame(
                501,
                [
                    {"type": "text", "data": {"text": "read the image"}},
                    {"type": "at", "data": {"qq": "100"}},
                    {"type": "forward", "data": {"id": "native-forward"}},
                    {"type": "image", "data": {"file": "qq-source-image"}},
                    {"type": "face", "data": {"id": "12"}},
                    {"type": "record", "data": {"file": "qq-source-audio"}},
                    {"type": "custom", "data": {}},
                    {"type": "file", "data": {"file": "qq-source-file"}},
                ],
            )
            peer.send(packet)
            initial, q = o.wire(http)
            event = o.rows(
                "SELECT e.id FROM events e JOIN event_locators l ON l.event_id=e.id WHERE l.native_id='501'"
            )[0][0]
            exec = o.rows("SELECT id FROM executions WHERE source=?", event)[0][0]
            items = o.op("media", "metadata", exec, o.actor, event)["attachments"]
            ctx.equal(
                "native incoming slots preserve v4 positions",
                [(v["attachment_index"], v["segment_index"]) for v in items],
                [(v[0], v[1]) for v in gold["direct_positions"]],
            )
            ctx.check(
                "first native main sees indices without acquiring or emitting image bytes",
                "attachment_index" in str(initial)
                and "base64," not in str(initial)
                and not select.select([peer.socket], [], [], 0.05)[0],
            )
            reply(
                q,
                calls=[
                    call(
                        "inspect",
                        "inspect_conversation_attachment",
                        {
                            "event_id": event,
                            "attachment_index": 1,
                            "question": "inspect original",
                        },
                    )
                ],
            )
            lookup = peer.receive()
            ctx.equal(
                "request goes to original image token on receiving stage",
                (lookup["action"], lookup["params"]),
                ("get_image", {"file": "qq-source-image"}),
            )
            ack(peer, lookup, {"base64": base64.b64encode(original_png).decode()})
            visible, q = o.wire(http)
            receipt = pair(visible, "inspect")
            ctx.equal(
                "immutable native image receipt binds the digest",
                (
                    receipt["state"],
                    receipt["result"]["sha256"],
                    receipt["result"]["state"],
                ),
                ("confirmed", image_sha, "native_image"),
            )
            pics = [
                p["image_url"]["url"]
                for m in visible["messages"]
                if isinstance(m.get("content"), list)
                for p in m["content"]
                if p.get("type") == "image_url"
            ]
            ctx.equal(
                "exact original image bytes reach paid normal Chat only after model inspection",
                pics,
                ["data:image/png;base64," + base64.b64encode(original_png).decode()],
            )
            ctx.write("paid-image-wire.json", visible)
            # Freeze a real file descriptor; change the input and replay the original operation.
            version = o.op(
                "media",
                "snapshot",
                exec,
                o.actor,
                "export-report",
                "report.txt",
                "report.txt",
                "file",
            )
            preview = o.rows(
                "SELECT peer_reference FROM social_media_bindings WHERE id=?",
                version["artifact_id"],
            )[0][0]
            (inputs / "report.txt").write_bytes(b"REPLACEMENT")
            same = o.op(
                "media",
                "snapshot",
                exec,
                o.actor,
                "export-report",
                "report.txt",
                "report.txt",
                "file",
            )
            ctx.equal(
                "retry reads the selected version, not changed root bytes",
                same,
                version,
            )
            ctx.equal(
                "immutable transfer copied original bytes",
                Path(preview).read_bytes(),
                b"ORIGINAL version",
            )
            for name in [
                "../outside/private",
                "escape/private",
                "hard",
                str(outside / "private"),
            ]:
                o.svc.command(
                    "operator",
                    "media",
                    "snapshot",
                    exec,
                    o.actor,
                    "invalid-" + str(len(name)),
                    name,
                    "from-input",
                    "file",
                    accepted=False,
                )
            ctx.equal(
                "outside bytes never became objects",
                o.rows(
                    "SELECT count(*) FROM media_objects WHERE bytes=?",
                    b"OUTSIDE-FILE-CANARY",
                ),
                [(0,)],
            )
            o.svc.command(
                "operator",
                "media",
                "cache-file",
                exec,
                o.actor,
                str(event),
                "4",
                "wrong-stage",
                "qq-source-file",
                "incoming.txt",
                accepted=False,
            )
            source = o.op(
                "media",
                "cache-file",
                exec,
                o.actor,
                event,
                4,
                o.bot["assignment"],
                "qq-source-file",
                "incoming.txt",
            )
            pinned = o.op(
                "media",
                "pin",
                exec,
                o.actor,
                event,
                4,
                "keep-file",
                "source.txt",
                "file",
            )
            ctx.equal(
                "explicit delivery pins the same immutable source",
                pinned["sha256"],
                source["digest"],
            )
            reply(
                q,
                calls=[
                    call(
                        "send-file",
                        "send",
                        {
                            "artifact_id": version["artifact_id"],
                            "attachment_kind": "file",
                            "name": "report.txt",
                        },
                    )
                ],
            )
            upload = peer.receive()
            ctx.equal(
                "native peer receives original file after path substitution",
                (upload["action"], Path(upload["params"]["file"]).read_bytes()),
                ("upload_private_file", b"ORIGINAL version"),
            )
            ack(peer, upload, {})
            final, q = o.wire(http)
            ctx.equal(
                "physical delivery succeeded",
                pair(final, "send-file")["state"],
                "confirmed",
            )
            reply(q, "done")
            o.finished(event)
            # The next input actually quotes the recorded native locator.
            peer.send(
                frame(
                    502,
                    [
                        {"type": "text", "data": {"text": "reply"}},
                        {"type": "reply", "data": {"id": "501"}},
                        {"type": "file", "data": {"file": "new-token"}},
                    ],
                )
            )
            _, q = o.wire(http)
            reply(q, "ok")
            quoted = o.rows(
                "SELECT event_id FROM event_locators WHERE native_id='502'"
            )[0][0]
            o.finished(quoted)
            quoted_items = o.op("media", "metadata", exec, o.actor, quoted)[
                "attachments"
            ]
            ctx.check(
                "reply presentation retains original source and shifted slot",
                any(
                    v["source_event"] == event
                    and v["attachment_index"] == 2
                    and v["segment_index"] == 3
                    for v in quoted_items
                ),
            )
        # Time-shift only the stored lease in an isolated offline data directory.
        with closing(sqlite3.connect(o.svc.data / "core.sqlite")) as db:
            db.execute(
                "UPDATE media_sources SET expires=1 WHERE event=? AND state='cached'",
                (event,),
            )
            db.commit()
        with (
            o.svc.running(environment=env),
            closing(Peer(o.svc.address, "100", env["YUKI_MEDIA_FIXTURE"])) as peer,
        ):
            o.op("media", "gc")
            expired = o.op("media", "metadata", exec, o.actor, event)["attachments"]
            ctx.check(
                "metadata survives expiry",
                all(
                    v["state"] == "expired"
                    for v in expired
                    if v["attachment_index"] in (1, 4)
                ),
            )
            o.svc.command(
                "operator",
                "media",
                "cache-file",
                exec,
                o.actor,
                str(event),
                "4",
                o.bot["assignment"],
                "qq-source-file",
                "incoming.txt",
                accepted=False,
            )
            ctx.equal(
                "unreferenced inbound image removed, referenced delivery stays",
                (
                    o.rows(
                        "SELECT count(*) FROM media_objects WHERE digest=?", image_sha
                    ),
                    o.rows(
                        "SELECT count(*) FROM media_objects WHERE digest=?",
                        pinned["sha256"],
                    ),
                ),
                ([(0,)], [(1,)]),
            )
            ctx.check(
                "no request on original peer after expired lease",
                not select.select([peer.socket], [], [], 0.08)[0],
            )
            o.op("privacy", "erase", "forget-media", o.actor, json.dumps([event]))
            o.op("media", "gc")
            ctx.check(
                "erased version and managed transfer lose access",
                not Path(preview).exists()
                and not o.rows(
                    "SELECT digest FROM media_versions WHERE digest IS NOT NULL"
                ),
            )
    return {
        "profiles": profile_cases(ctx, env, original_png),
        "event": event,
        "direct": items,
        "reply": quoted_items,
        "native_sha256": image_sha,
        "file": version,
        "pinned": pinned,
        "expired": expired,
        "source_commit": gold["source_commit"],
    }
