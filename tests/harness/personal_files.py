"""Drive immutable Client9 artifacts through a real QQ send and two authenticated phones."""

import hashlib
import json
import os
import uuid
from contextlib import closing
from pathlib import Path
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import Request, urlopen
from tests.harness.compact import Owner, call, reply
from tests.harness.personal import matches_reply
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer


def observe(ctx, specs, routes, wire):
    o = Owner(ctx, "delivered-files", 21)
    content = b"ORIGINAL work file\n"
    file = o.svc.data / "media-inputs" / "source.txt"
    file.write_bytes(content)
    name = "茶 具.txt"
    digest = hashlib.sha256(content).hexdigest()
    stranger = str(uuid.uuid4())
    env = {**os.environ, "YUKI_FILE_TEST_QQ": "synthetic-receiver"}
    with Recorder() as model:
        with (o.svc.data / "yuki.toml").open("a") as f:
            f.write(
                "\n[media]\ngateway_transfer_directory="
                + json.dumps(str(o.svc.data / "media-transfer"))
                + "\n"
            )
            f.write(
                f'\n[main]\nendpoint="{model.url}"\nmodel="fixture"\nname="Yuki"\nmax_output=256\nroot_slots=12\nmax_input_units=1400000\nmerge_ms=1\n[qq]\nnamespace="compact"\ndesignated_ingress="100"\n[[qq.bots]]\nself_id="100"\nstage="{o.bot["assignment"]}"\ntoken_env="YUKI_FILE_TEST_QQ"\n'
            )

        def get(path, token, status=200):
            req = Request(
                "http://" + o.svc.address + path,
                headers={"Authorization": "Bearer " + token},
            )
            try:
                response = urlopen(req, timeout=5)
            except HTTPError as err:
                response = err
            with response:
                raw = response.read()
                ctx.equal("artifact HTTP " + path, response.status, status)
                if (
                    response.status == 200
                    and response.headers.get_content_type()
                    == "application/octet-stream"
                ):
                    routes.add("GET /client/v1/artifacts/{identity}")
                    return raw, dict(response.headers)
                v = json.loads(raw)
                if response.status == 200:
                    ctx.check(
                        "artifact JSON agrees with fixed Client9",
                        matches_reply(specs, "GET", path, v),
                    )
                    from tests.harness.personal import template

                    routes.add("GET " + template(specs, path))
                return v.get("data", v)

        with o.svc.running(environment=env), closing(
            Peer(o.svc.address, "100", env["YUKI_FILE_TEST_QQ"])
        ) as peer:

            def pair(owner):
                code = o.op("personal", "issue", owner, str(uuid.uuid4()), "chat")[
                    "code"
                ]
                status, v = wire(
                    o.svc.address,
                    "/client/v1/pair",
                    body={"code": code, "device_label": "controlled file witness"},
                )
                ctx.equal("paired file witness", status, 200)
                return v["data"]

            o.op("person", "create", stranger)
            a, b, foreign = pair(o.actor), pair(o.actor), pair(stranger)
            event = o.human("deliver original workspace document")
            _, q = o.wire(model)
            reply(
                q,
                calls=[
                    call(
                        "accept",
                        "task_control",
                        {"action": "accept", "goal": "return original file"},
                    )
                ],
            )
            body, q = o.wire(model)
            work = json.loads(
                next(
                    m["content"]
                    for m in body["messages"]
                    if m.get("tool_call_id") == "accept"
                )
            )["result"]["work_id"]
            ex = o.rows("SELECT id FROM executions WHERE source=?", event)[0][0]
            frozen = o.op(
                "media",
                "snapshot",
                ex,
                o.actor,
                "original-file",
                "source.txt",
                name,
                "file",
            )
            fid = frozen["artifact_id"]
            path = "/client/v1/artifacts/" + quote(fid, safe="") + "?version=" + digest
            get(path, a["token"], 404)
            file.write_bytes(b"replacement bytes at the old path")
            reply(
                q,
                calls=[
                    call(
                        "physical-file",
                        "send",
                        {"artifact_id": fid, "attachment_kind": "file", "name": name},
                    )
                ],
            )
            sent = peer.receive()
            ctx.equal(
                "one real native upload contains pinned original bytes",
                (sent["action"], Path(sent["params"]["file"]).read_bytes()),
                ("upload_private_file", content),
            )
            get(path, a["token"], 404)
            from tests.harness.client_stream import observe as observe_stream
            ctx.write("client-outbound-stream.json",observe_stream(ctx,o,wire,pair,a,foreign,work,event,
                lambda:peer.send({"echo": sent["echo"], "status": "ok", "retcode": 0, "data": {}})))
            routes.add("GET /client/v1/stream")
            _, q = o.wire(model)
            listing = get("/client/v1/artifacts?kind=document&limit=1", a["token"])
            row = listing["items"][0]
            ctx.equal(
                "native ACK without locator binds immutable Client9 reference",
                (
                    row["artifact_id"],
                    row["version"],
                    row["work_id"],
                    row["published_path"],
                ),
                (fid, digest, work, None),
            )
            out_event = row["ref"]["event_id"]
            ctx.check(
                "original delivery event has no invented platform locator",
                not o.rows(
                    "SELECT native_id FROM event_locators WHERE event_id=?", out_event
                ),
            )
            for device in (a, b):
                raw, headers = get(path, device["token"])
                h = {k.lower(): v for k, v in headers.items()}
                ctx.equal(
                    "device receives original bytes and headers",
                    (raw, h["etag"], h["x-yuki-sha256"], h["content-disposition"]),
                    (
                        content,
                        '"' + digest + '"',
                        digest,
                        "attachment; filename*=UTF-8''" + quote(name, safe=""),
                    ),
                )
            get(path, foreign["token"], 404)
            get(path.replace(digest, "0" * 64), b["token"], 404)
            ctx.equal(
                "unrelated personal catalog stays empty",
                get("/client/v1/artifacts", foreign["token"])["items"],
                [],
            )
            ctx.equal(
                "file is absent from image-only page",
                get("/client/v1/artifacts?kind=image", a["token"])["items"],
                [],
            )
            work_view = get("/client/v1/works/" + work, b["token"])
            ctx.equal(
                "Work result identifies exact delivered artifact",
                [
                    (v["event_id"], p["artifact_id"], p["version"])
                    for v in work_view["results"]
                    for p in v["artifacts"]
                ],
                [(out_event, fid, digest)],
            )
            history = get(
                "/client/v1/conversations/"
                + quote(row["conversation_id"], safe="")
                + "/events",
                b["token"],
            )
            emitted = [
                item for item in history["items"] if item["event_id"] == out_event
            ]
            ctx.check(
                "canonical personal timeline exposes the delivered version once",
                len(emitted) == 1 and emitted[0]["attachments"][0]["version"] == digest,
            )
            reply(
                q,
                calls=[
                    call(
                        "wait",
                        "task_control",
                        {
                            "action": "need_input",
                            "work_id": work,
                            "reason": "confirm file delivery",
                        },
                    )
                ],
            )
            o.finished(event)
            ctx.equal(
                "work waits before native owner shutdown",
                get("/client/v1/works/" + work, b["token"])["state"],
                "waiting_user",
            )
        with o.svc.running(environment=env), closing(
            Peer(o.svc.address, "100", env["YUKI_FILE_TEST_QQ"])
        ) as peer:
            ctx.equal(
                "cold owner serves same immutable reference",
                get(path, b["token"])[0],
                content,
            )
            status, _ = wire(
                o.svc.address,
                "/client/v1/works/" + work + "/inputs",
                b["token"],
                {
                    "request_id": str(uuid.uuid4()),
                    "text": "send the same file version again",
                },
            )
            ctx.equal("second phone wakes original Work", status, 200)
            _, q = o.wire(model)
            reply(
                q,
                calls=[
                    call(
                        "continued-file",
                        "send",
                        {"artifact_id": fid, "attachment_kind": "file", "name": name},
                    )
                ],
            )
            resend = peer.receive()
            ctx.equal(
                "new Work activation uses same pinned bytes",
                (resend["action"], Path(resend["params"]["file"]).read_bytes()),
                ("upload_private_file", content),
            )
            peer.send(
                {"echo": resend["echo"], "status": "ok", "retcode": 0, "data": {}}
            )
            _, q = o.wire(model)
            reply(q, "done")
            catalog = get("/client/v1/artifacts?limit=1", b["token"])
            after = get(
                "/client/v1/artifacts?limit=1&cursor=" + catalog["next_cursor"],
                b["token"],
            )
            ctx.equal(
                "stable cursor pages both confirmed deliveries of exact same file",
                (after["items"][0]["ref"]["event_id"], after["next_cursor"]),
                (out_event, None),
            )
            ctx.equal(
                "two distinct executions bound to exact original Work",
                o.rows(
                    "SELECT count(DISTINCT d.namespace) FROM personal_artifact_events d WHERE d.id=? AND d.work=?",
                    fid,
                    work,
                ),
                [(2,)],
            )
            sibling = o.human("separate conversation turn")
            _, q = o.wire(model)
            reply(
                q,
                calls=[
                    call(
                        "stolen-version",
                        "send",
                        {"artifact_id": fid, "attachment_kind": "file", "name": name},
                    )
                ],
            )
            msg, q = o.wire(model)
            failed = json.loads(
                next(
                    m["content"]
                    for m in msg["messages"]
                    if m.get("tool_call_id") == "stolen-version"
                )
            )
            ctx.equal(
                "a sibling execution cannot send a private Work version",
                failed["state"],
                "failed",
            )
            reply(q, "done")
            o.finished(sibling)
            o.op(
                "privacy", "erase", "erase-original-file", o.actor, json.dumps([event])
            )
            get(path, b["token"], 404)
            ctx.equal(
                "source erasure also removes the catalog reference",
                get("/client/v1/artifacts", b["token"])["items"],
                [],
            )
        # Keep one HTTP stream open across a real owner shutdown. Graceful drain
        # must release it even if no later database commits are made.
        from tests.harness.client_stream import Stream
        held=None
        try:
            with o.svc.running(environment=env):
                held=Stream(o.svc.address,foreign)
                ctx.check("subscriber is active before owner drain",held.frame()[0]=="sync")
            while held.frame() is not None:
                pass
            ctx.check("owner shutdown closes live SSE connection",True)
        finally:
            if held:held.close()
    return {
        "artifact": fid,
        "version": digest,
        "work": work,
        "event": out_event,
        "ack_has_locator": False,
        "source_erasure_returns": 404,
    }
