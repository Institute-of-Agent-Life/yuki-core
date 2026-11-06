"""Native two-device Work replay matching the fixed v4 six-request continuation."""

import hashlib
import json
import os
import time
import uuid
from contextlib import closing
from pathlib import Path
from urllib.parse import quote
from urllib.request import Request, urlopen


def run(ctx, index, *, withdraw_question=False):
    from tests.harness.compact import Owner, call, reply
    from bench.work_trace import projection
    from tests.peers.chat.recorder import Recorder
    from tests.peers.onebot.peer import Peer

    begin = time.perf_counter()
    o = Owner(ctx, f"long-{index}", 25)
    wires = []
    physical = []
    content = b"original Work file bytes"
    filename = "original-result.txt"
    digest = hashlib.sha256(content).hexdigest()
    env = {**os.environ, "YUKI_LONG_WORK_QQ": "synthetic-offline"}
    config = o.svc.data / "yuki.toml"
    base = config.read_text()

    def http(path, token=None, body=None):
        headers = {"Content-Type": "application/json"}
        if token:
            headers["Authorization"] = "Bearer " + token
        data = json.dumps(body).encode() if body is not None else None
        with urlopen(
            Request("http://" + o.svc.address + path, data=data, headers=headers),
            timeout=8,
        ) as response:
            if response.headers.get_content_type() == "application/octet-stream":
                return response.read()
            return json.load(response)["data"]

    with Recorder() as model:
        runtime = (
            f'\n[main]\nendpoint="{model.url}"\nmodel="fixture"\nname="Yuki"\nmax_output=4096\nroot_slots=32\nmax_input_units=4000000\nmerge_ms=1\n[qq]\nnamespace="compact"\ndesignated_ingress="100"\n[[qq.bots]]\nself_id="100"\nstage="{o.bot["assignment"]}"\ntoken_env="YUKI_LONG_WORK_QQ"\n[media]\ngateway_transfer_directory='
            + json.dumps(str(o.svc.data / "media-transfer"))
            + "\n"
        )

        def configure(limit):
            config.write_text(
                base
                + runtime
                + f"\n[work_segments]\nmodel_requests={limit}\nresume_delay_ms=2000\n",
                encoding="utf-8",
            )

        def wire():
            body, q = o.wire(model)
            wires.append(body)
            return body, q

        def ack(peer, request):
            physical.append(request)
            peer.send(
                {
                    "echo": request["echo"],
                    "status": "ok",
                    "retcode": 0,
                    "data": {"message_id": 5000 + len(physical)}
                    if request["action"] == "send_private_msg"
                    else {},
                }
            )

        configure(24)
        with (
            o.svc.running(environment=env),
            closing(Peer(o.svc.address, "100", env["YUKI_LONG_WORK_QQ"])) as peer,
        ):
            code = o.op("personal", "issue", o.actor, str(uuid.uuid4()), "chat")["code"]
            desktop = http(
                "/client/v1/pair", body={"code": code, "device_label": "desktop"}
            )
            event = o.human("continue the original task on another device")
            _, q = wire()
            reply(
                q,
                calls=[
                    call(
                        "accept",
                        "task_control",
                        {"action": "accept", "goal": "continue original work"},
                    )
                ],
            )
            body, q = wire()
            accepted = json.loads(
                next(
                    m["content"]
                    for m in body["messages"]
                    if m.get("tool_call_id") == "accept"
                )
            )
            work = accepted["result"]["work_id"]
            owner = o.rows("SELECT owner_execution FROM work_nodes WHERE id=?", work)[
                0
            ][0]
            reply(q, calls=[call("question", "send", {"text": "which city?"})])
            sent = peer.receive()
            ctx.equal(
                "first physical question is original QQ send",
                sent["action"],
                "send_private_msg",
            )
            ack(peer, sent)
            _, q = wire()
            reply(
                q,
                calls=[
                    call(
                        "waiting",
                        "task_control",
                        {"action": "need_input", "reason": "city missing"},
                    )
                ],
            )
            o.finished(event)
            original = http("/client/v1/works/" + work, desktop["token"])
            ctx.equal(
                "original Work waits for its own user",
                original["state"],
                "waiting_user",
            )
            ctx.equal(
                "original waiting question delivered",
                original["waiting_question"]["text"],
                "which city?",
            )
            code = http(
                "/client/v1/link-codes",
                desktop["token"],
                {"request_id": str(uuid.uuid4())},
            )["code"]
            phone = http(
                "/client/v1/pair", body={"code": code, "device_label": "phone"}
            )
            original_requests = o.rows(
                "SELECT id,request_json FROM model_requests ORDER BY rowid"
            )
            source = o.rows(
                "SELECT x.id,x.source,x.original_actor,t.window,w.entry,w.budget FROM work_nodes w JOIN executions x ON x.id=w.owner_execution JOIN main_turns t ON t.execution=x.id WHERE w.id=?",
                work,
            )
            file = o.svc.data / "media-inputs" / filename
            file.write_bytes(content)
            artifact = o.op(
                "media",
                "snapshot",
                owner,
                o.actor,
                "original-result",
                filename,
                filename,
                "file",
            )
            request = {
                "request_id": str(uuid.uuid4()),
                "text": "Use Taipei, preserve the original work",
            }
            # Stop at the natural waiting boundary. The next cold owner admits the phone input.
        configure(1)
        with o.svc.running(environment=env):
            answer = http(
                "/client/v1/works/" + work + "/inputs", phone["token"], request
            )
            ctx.equal(
                "phone submission leaves prior paid journal exact",
                o.rows(
                    "SELECT id,request_json FROM model_requests ORDER BY rowid LIMIT 3"
                ),
                original_requests,
            )
            body, q = wire()
            ctx.check(
                "new private input reaches original Work",
                "Use Taipei" in json.dumps(body, ensure_ascii=False),
            )
            if withdraw_question:
                # Withdraw the same acknowledged part after the cold owner received the answer.
                # Hold the next model call even if its previous body still contains the question.
                import sqlite3

                with closing(sqlite3.connect(o.svc.data / "core.sqlite")) as db:
                    changed = db.execute(
                        "UPDATE social_outbound SET recalled=1 WHERE id IN "
                        "(SELECT outbound FROM personal_deliveries WHERE event=?)",
                        (original["waiting_question"]["event_id"],),
                    )
                    ctx.equal(
                        "fault revoked exactly the original question",
                        changed.rowcount,
                        1,
                    )
                    db.commit()
                reply(
                    q,
                    calls=[
                        call(
                            "after-input-read",
                            "find_contacts",
                            {"kind": "person", "subject_ref": "current_speaker"},
                        )
                    ],
                )
                o.until(
                    lambda: o.rows(
                        "SELECT x.id FROM main_turns t JOIN executions x ON x.id=t.execution WHERE t.diagnostic='send_receipt_source_changed' AND x.state='held'"
                    )
                )
                ctx.equal(
                    "withdrawn question prevents a fifth physical attempt",
                    o.rows("SELECT COUNT(*) FROM model_attempts"),
                    [(4,)],
                )
                return {"physical_calls": len(wires), "held": True}
            reply(
                q,
                calls=[
                    call(
                        "after-input-read",
                        "find_contacts",
                        {"kind": "person", "subject_ref": "current_speaker"},
                    )
                ],
            )
            o.until(
                lambda: o.rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='queued'", work
                )
            )
            ctx.equal(
                "one segment slot adds exactly one request",
                o.rows(
                    "SELECT reserved_requests FROM root_budgets WHERE root=?",
                    original["budget"]["root_id"],
                ),
                [(4,)],
            )
            ctx.equal(
                "private mailbox consumed once before cold continuation",
                o.rows(
                    "SELECT count(*) FROM work_mail m JOIN work_consumption c ON c.work=m.work WHERE m.work=? AND m.kind='human_answer' AND m.id<=c.watermark",
                    work,
                ),
                [(1,)],
            )
        configure(24)
        with (
            o.svc.running(environment=env),
            closing(Peer(o.svc.address, "100", env["YUKI_LONG_WORK_QQ"])) as peer,
        ):
            body, q = wire()
            contact = json.loads(
                next(
                    m["content"]
                    for m in body["messages"]
                    if m.get("tool_call_id") == "after-input-read"
                )
            )
            ctx.check(
                "cold segment receives original contact result",
                o.actor in json.dumps(contact, ensure_ascii=False),
            )
            reply(
                q,
                calls=[
                    call(
                        "result",
                        "send",
                        {
                            "artifact_id": artifact["artifact_id"],
                            "attachment_kind": "file",
                            "text": "Taipei result from original Work",
                        },
                    )
                ],
            )
            # The original attachment plan uploads the file before its text part.
            file_part = peer.receive()
            ctx.equal(
                "result uploads exact original bytes",
                (file_part["action"], Path(file_part["params"]["file"]).read_bytes()),
                ("upload_private_file", content),
            )
            ack(peer, file_part)
            text_part = peer.receive()
            ctx.equal(
                "result text uses original QQ destination",
                (text_part["action"], text_part["params"]["user_id"]),
                ("send_private_msg", 200),
            )
            ack(peer, text_part)
            _, q = wire()
            reply(
                q,
                calls=[
                    call(
                        "complete",
                        "task_control",
                        {"action": "complete", "result": "original task finished"},
                    )
                ],
            )
            o.until(
                lambda: o.rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'", work
                )
            )
            ctx.equal(
                "completed Work preserves original source and root",
                o.rows(
                    "SELECT x.id,x.source,x.original_actor,t.window,w.entry,w.budget FROM work_nodes w JOIN executions x ON x.id=w.owner_execution JOIN main_turns t ON t.execution=x.id WHERE w.id=?",
                    work,
                ),
                source,
            )
            ctx.equal(
                "six actual logical and physical model calls",
                (
                    len(wires),
                    o.rows(
                        "SELECT reserved_requests FROM root_budgets WHERE root=?",
                        original["budget"]["root_id"],
                    ),
                ),
                (6, [(6,)]),
            )
            ids = o.rows("SELECT call_id FROM model_tool_intents ORDER BY rowid")
            ctx.equal(
                "six original provider tool IDs",
                ids,
                [
                    (s,)
                    for s in (
                        "accept",
                        "question",
                        "waiting",
                        "after-input-read",
                        "result",
                        "complete",
                    )
                ],
            )
            file.write_bytes(b"later replacement bytes")
            replay = http(
                "/client/v1/works/" + work + "/inputs", phone["token"], request
            )
            ctx.check(
                "same phone input replays original receipt after completion",
                replay["replay"],
            )
            home = http("/client/v1/home", desktop["token"])
            final = next(w for w in home["works"] if w["work_id"] == work)
            ctx.equal(
                "second original device sees completed Work",
                final["state"],
                "completed",
            )
            path = (
                "/client/v1/artifacts/"
                + quote(artifact["artifact_id"], safe="")
                + "?version="
                + digest
            )
            ctx.equal(
                "phone downloads original delivered version after path replacement",
                http(path, phone["token"]),
                content,
            )
            ctx.equal(
                "desktop downloads same original delivered version",
                http(path, desktop["token"]),
                content,
            )
            versions = [
                (r["artifact_id"], r["version"])
                for result in final["results"]
                for r in result["artifacts"]
            ]
            ctx.check(
                "public Work result retains immutable original reference",
                (artifact["artifact_id"], digest) in versions,
            )
            declarations = [
                sorted(t["function"]["name"] for t in w["tools"]) for w in wires
            ]
            ctx.check(
                "private client-only tool is never injected into Main",
                all("read_personal_entry" not in names for names in declarations),
            )
            proof = {
                "index": index,
                "work": work,
                "original_source": source,
                "models": 6,
                "calls": ids,
                "segments": o.rows(
                    "SELECT execution,ordinal,request_limit,state,request_count FROM main_segments ORDER BY rowid"
                ),
                "physical": physical,
                "artifact_sha256": digest,
                "phone_request": answer,
                "final": final,
                "declarations": declarations,
                "original_history_name_present": all(
                    "query_chat_history" in names for names in declarations
                ),
                "elapsed_seconds": time.perf_counter() - begin,
            }
    ctx.write(f"C-long-{index:02}-wire.json", wires)
    ctx.write(f"C-long-{index:02}-proof.json", proof)
    return {
        "requests": projection(wires),
        "models": proof["models"],
        "artifact_sha256": digest,
    }
