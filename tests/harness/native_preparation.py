"""A native upload receipt and its visible child retain separate physical outcomes."""

from contextlib import closing
import hashlib
import json
import runpy
import sqlite3
import sys

from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def run(ctx, sdk, profile="debug"):
    owner = Owner(ctx, "native-preparation", 56, profile)
    peer = ctx.working / "preparation-peer"
    peer.mkdir()
    (peer / "native-preparation").touch()
    (peer / "foreign.json").write_text(
        json.dumps({"artifact_id": "foreign", "version": "0" * 64})
    )
    png = runpy.run_path(str(ctx.root / "tests/cases/CORE-025.py"))["png"]()
    for n in range(2):
        (peer / f"image-{n}.png").write_bytes(png)
    payload = b"Original native upload\n" + bytes(range(251)) * 101
    identity = {"application": "cli_original", "tenant": "original-tenant"}
    with owner.svc.running():
        bot = owner.op(
            "account",
            "open",
            "feishu",
            "native-preparation",
            "cli_original",
            1,
            "--start",
            1,
        )["assignment"]
        owner.op(
            "account",
            "open",
            "feishu",
            "native-preparation",
            "ou_original",
            1,
            "--person",
            owner.actor,
            "--start",
            1,
        )
    definition = dict(
        id="native-preparation",
        namespace="native-preparation",
        platform="feishu",
        bot_stage=bot,
        program=sys.executable,
        args=[str(ctx.root / "tests/peers/host-media/peer.py"), str(peer), str(sdk)],
        grants=[
            "gateway.inbound",
            "gateway.media",
            "gateway.send",
            "media.open",
            "media.publish",
            "forget.owner",
            "tool.call",
        ],
    )
    with (owner.svc.data / "yuki.toml").open("a") as config:
        config.write(
            "\n[[plugins]]\n"
            + "\n".join(k + "=" + json.dumps(v) for k, v in definition.items())
            + "\n"
        )
        config.write(
            "[plugins.gateway_media]\n"
            + "\n".join(k + "=" + json.dumps(v) for k, v in identity.items())
            + "\n"
        )
        config.write(
            '[[plugins.tools]]\nname="make_images"\ndescription="Create original artifacts"\ncacheable=false\nparallel_read=false\ncode_direct=false\nschema={type="object",properties={},additionalProperties=false}\n'
        )

    def submit(params):
        (peer / "probed.json").unlink(missing_ok=True)
        staged = peer / "probe.tmp"
        staged.write_text(json.dumps({"method": "gateway.inbound", "params": params}))
        staged.replace(peer / "probe.json")
        owner.until(lambda: (peer / "probed.json").exists())
        response = json.loads((peer / "probed.json").read_bytes())
        ctx.check("native preparation source is admitted", not response["denied"])
        return response["result"]

    def answer(wire, key):
        return json.loads(
            next(m["content"] for m in wire["messages"] if m.get("tool_call_id") == key)
        )

    observations = {}
    sources = []
    cases = [
        ("normal", "confirmed", 1, 2, 2),
        ("original-name", "confirmed", 1, 2, 2),
        ("quoted", "confirmed", 1, 2, 2),
        ("file-failed", "failed", 1, 1, 0),
        ("caption-failed", "failed", 1, 2, 1),
        ("prepare-unknown", "unknown", 1, 0, 0),
        ("missing-resource", "unknown", 1, 0, 0),
        ("file-unknown", "unknown", 1, 1, 0),
        ("forward-reference", "failed", 0, 0, 0),
        ("reused-resource", "failed", 0, 0, 0),
        ("prepare-lost", "unknown", 1, 0, 0),
    ]
    with Recorder() as model:
        owner.config(model, extra="root_tool_calls=32\n", binding=False)
        with owner.svc.running():
            owner.until(lambda: (peer / "ready.json").exists())
            for ordinal, (mode, expected, uploads, deliveries, visible) in enumerate(
                cases
            ):
                (peer / "preparation-case").write_text(mode)
                (peer / "paper.dat").write_bytes(payload)
                params = {
                    "source": mode,
                    "native_id": "om_" + mode,
                    "sender": "ou_original",
                    "occurred": ordinal + 10,
                    "kind": "message",
                    "accept": True,
                    "media_identity": identity,
                    "segments": [{"type": "text", "text": "prepare the original file"}],
                }
                event = submit(params)["event"]
                sources.append((event, params))
                _, queue = owner.wire(model)
                reply(queue, calls=[call("make", "make_images", {})])
                wire, queue = owner.wire(model)
                created = answer(wire, "make")["result"]["data"]["document"]
                reply(
                    queue,
                    calls=[
                        call(
                            "send",
                            "send",
                            {
                                "artifact_id": created["artifact_id"],
                                "attachment_kind": "file",
                                **(
                                    {}
                                    if mode == "original-name"
                                    else {"name": "original.bin"}
                                ),
                                "text": "caption",
                                **(
                                    {"reply_to_event_id": event}
                                    if mode == "quoted"
                                    else {}
                                ),
                            },
                        )
                    ],
                )
                wire, queue = owner.wire(model)
                result = answer(wire, "send")
                ctx.equal(
                    mode + " retains the original send outcome",
                    result["state"],
                    expected,
                )
                execution = owner.rows(
                    "SELECT id FROM executions WHERE source=?", event
                )[0][0]
                planned_name = owner.rows(
                    "SELECT json_extract(args,'$.message.name') FROM host_calls WHERE execution=? AND method='gateway.plan_send'",
                    execution,
                )[0][0]
                ctx.equal(
                    mode + " plans the selected original or explicit filename",
                    planned_name,
                    "paper.bin" if mode == "original-name" else "original.bin",
                )
                if mode == "original-name":
                    original_payload = json.loads(
                        owner.rows(
                            "SELECT payload FROM operations WHERE namespace=? AND target='send' AND parent IS NULL",
                            execution,
                        )[0][0]
                    )
                    ctx.equal(
                        mode + " preserves the admitted send payload",
                        original_payload.get("name"),
                        None,
                    )
                    ctx.equal(
                        mode + " preserves missing versus null name",
                        "name" in original_payload,
                        False,
                    )
                children = owner.rows(
                    "SELECT c.method,c.state,c.result,o.ordinal FROM host_calls c JOIN operations o ON o.namespace=c.execution AND o.operation_id=c.operation WHERE c.execution=? AND c.method IN('gateway.prepare_part','gateway.dispatch_part') ORDER BY o.ordinal",
                    execution,
                )
                ctx.equal(
                    mode + " has separately counted physical steps",
                    [
                        sum(r[0] == m for r in children)
                        for m in ["gateway.prepare_part", "gateway.dispatch_part"]
                    ],
                    [uploads, deliveries],
                )
                ctx.equal(
                    mode + " publishes only confirmed visible messages",
                    owner.rows(
                        "SELECT count(*) FROM social_outbound WHERE namespace=?",
                        execution,
                    ),
                    [(visible,)],
                )
                prepared = [
                    json.loads(p.read_bytes())
                    for p in peer.glob("prepare-*.json")
                    if json.loads(p.read_bytes())["mode"] == mode
                ]
                ctx.equal(
                    "preparation route retains the canonical private scope",
                    [p["arguments"]["route"]["scope"] for p in prepared],
                    ["private"] * uploads,
                )
                ctx.equal(
                    mode + " reads only original bytes",
                    [p["sha256"] for p in prepared],
                    [hashlib.sha256(payload).hexdigest()] * uploads,
                )
                if uploads and mode not in {
                    "prepare-unknown",
                    "missing-resource",
                    "prepare-lost",
                }:
                    ctx.equal(
                        mode + " keeps the confirmed upload",
                        children[0][1],
                        "confirmed",
                    )
                sent = [
                    json.loads(p.read_bytes())
                    for p in peer.glob("visible-*.json")
                    if json.loads(p.read_bytes())["mode"] == mode
                ]
                sent.sort(key=lambda item: item["arguments"]["part_ordinal"])
                for part in sent:
                    if "prepared" in part["arguments"]:
                        proof = part["arguments"]["prepared"]
                        ctx.check(
                            "visible child uses the exact original resource",
                            proof["resource"]["key"] == "file-" + proof["operation"]
                            and proof["media"]
                            == {
                                "artifact_id": created["artifact_id"],
                                "version": created["version"],
                            },
                        )
                if mode == "quoted":
                    ctx.check(
                        "upload preparation carries no reply quote",
                        "reply" not in prepared[0]["arguments"],
                    )
                    ctx.equal(
                        "first visible child retains original quote",
                        sent[0]["arguments"]["reply"],
                        {"event_id": event, "native_id": params["native_id"]},
                    )
                    ctx.check(
                        "caption omits the quote", "reply" not in sent[1]["arguments"]
                    )
                observations[mode] = {
                    "event": event,
                    "execution": execution,
                    "result": result,
                    "steps": children,
                    "visible": visible,
                }
                reply(queue)
                owner.finished(event)
        (peer / "ready.json").unlink()
        with owner.svc.running():
            owner.until(lambda: (peer / "ready.json").exists())
            for mode in ["changed-deployment", "revoked-media", "erased-version"]:
                (peer / "preparation-case").write_text(mode)
                (peer / "preparation-waiting.json").unlink(missing_ok=True)
                (peer / "paper.dat").write_bytes(payload)
                params = {
                    **sources[-1][1],
                    "source": mode,
                    "native_id": "om_" + mode,
                }
                event = submit(params)["event"]
                _, queue = owner.wire(model)
                reply(queue, calls=[call("make", "make_images", {})])
                wire, queue = owner.wire(model)
                created = answer(wire, "make")["result"]["data"]["document"]
                (peer / "pause-preparation").touch()
                reply(
                    queue,
                    calls=[
                        call(
                            "send",
                            "send",
                            {
                                "artifact_id": created["artifact_id"],
                                "attachment_kind": "file",
                                "name": "original.bin",
                                "text": "caption",
                            },
                        )
                    ],
                )
                owner.until(lambda: (peer / "preparation-waiting.json").exists())
                original_approval = owner.rows(
                    "SELECT value FROM core_meta WHERE key='plugin-approval:native-preparation'"
                )[0][0]
                execution = owner.rows(
                    "SELECT id FROM executions WHERE source=?", event
                )[0][0]
                original_grants = owner.rows(
                    "SELECT grants FROM host_installations WHERE plugin='native-preparation'"
                )[0][0]
                with closing(
                    sqlite3.connect(owner.svc.data / "core.sqlite")
                ) as database:
                    if mode == "changed-deployment":
                        database.execute(
                            "UPDATE core_meta SET value=json_set(value,'$.deployment','replacement') WHERE key='plugin-approval:native-preparation'"
                        )
                    elif mode == "revoked-media":
                        grants = [
                            g for g in json.loads(original_grants) if g != "media.open"
                        ]
                        database.execute(
                            "UPDATE host_installations SET grants=? WHERE plugin='native-preparation'",
                            (json.dumps(grants),),
                        )
                    else:
                        database.execute(
                            "UPDATE media_versions SET digest=NULL WHERE id=? AND execution=?",
                            (created["artifact_id"], execution),
                        )
                    database.commit()
                (peer / "pause-preparation").unlink()
                owner.until(
                    lambda: owner.rows(
                        "SELECT id FROM executions WHERE source=? AND state='held'",
                        event,
                    )
                )
                ctx.equal(
                    mode + " receives no prepared native resource",
                    len(
                        [
                            p
                            for p in peer.glob("visible-*.json")
                            if json.loads(p.read_bytes())["mode"] == mode
                        ]
                    ),
                    0,
                )
                observations[mode] = {
                    "event": event,
                    "new_visible_requests": 0,
                    "execution_state": "held",
                }
                with closing(
                    sqlite3.connect(owner.svc.data / "core.sqlite")
                ) as database:
                    database.execute(
                        "UPDATE core_meta SET value=? WHERE key='plugin-approval:native-preparation'",
                        (original_approval,),
                    )
                    database.execute(
                        "UPDATE host_installations SET grants=? WHERE plugin='native-preparation'",
                        (original_grants,),
                    )
                    database.commit()
            (peer / "preparation-case").write_text("missing-origin")
            params = {
                **sources[0][1],
                "source": "missing-origin",
                "native_id": "om_unbound",
            }
            params.pop("media_identity")
            event = submit(params)["event"]
            _, queue = owner.wire(model)
            reply(queue, calls=[call("make", "make_images", {})])
            wire, queue = owner.wire(model)
            created = answer(wire, "make")["result"]["data"]["document"]
            reply(
                queue,
                calls=[
                    call(
                        "send",
                        "send",
                        {
                            "artifact_id": created["artifact_id"],
                            "attachment_kind": "file",
                        },
                    )
                ],
            )
            owner.until(
                lambda: owner.rows(
                    "SELECT id FROM executions WHERE source=? AND state='held'", event
                )
            )
            ctx.equal(
                "unbound original source performs no upload",
                [
                    p.name
                    for p in peer.glob("prepare-*.json")
                    if json.loads(p.read_bytes())["mode"] == "missing-origin"
                ],
                [],
            )
            ctx.equal(
                "new identity on redelivery returns the original source",
                submit({**params, "media_identity": identity})["event"],
                event,
            )
            ctx.equal(
                "redelivery cannot backfill original application authority",
                owner.rows(
                    "SELECT count(*) FROM gateway_media_origins WHERE event=?", event
                ),
                [(0,)],
            )
            observations["missing-origin"] = {
                "event": event,
                "execution_state": "held",
                "new_uploads": 0,
                "rebound_origins": 0,
            }
        before = len(list(peer.glob("prepare-*.json"))) + len(
            list(peer.glob("visible-*.json"))
        )
        before_model = len(model.header_shapes)
        (peer / "ready.json").unlink()
        with owner.svc.running():
            owner.until(lambda: (peer / "ready.json").exists())
            for event, params in sources:
                ctx.equal(
                    "cold replay returns the original logical source",
                    submit(params)["event"],
                    event,
                )
            ctx.equal(
                "cold replay performs no native upload or delivery",
                len(list(peer.glob("prepare-*.json")))
                + len(list(peer.glob("visible-*.json"))),
                before,
            )
            ctx.equal(
                "cold replay creates no new model request",
                len(model.header_shapes),
                before_model,
            )
    result = {
        "status": "PASS",
        "synthetic_consumers": True,
        "paid_calls": 0,
        "cases": observations,
    }
    ctx.write("native-preparation-ledger.json", result)
    return result
