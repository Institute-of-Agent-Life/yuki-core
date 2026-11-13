"""Large original text stays bounded on the wire and retains each physical result."""

import hashlib
import json
import sys
import sqlite3
from contextlib import closing
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def recorded_wire(peer):
    return b"".join(path.read_bytes() + b"\n" for path in sorted((peer / "wire").glob("*.json")))


def run(ctx, sdk, profile="debug"):
    owner = Owner(ctx, "gateway-text", 56, profile)
    peer = ctx.working / "text-peer"
    peer.mkdir()
    (peer / "mode").write_text("inline")
    with owner.svc.running():
        stage = owner.op(
            "account",
            "open",
            "feishu",
            "text-original",
            "cli_original",
            1,
            "--start",
            1,
        )["assignment"]
        owner.op(
            "account",
            "open",
            "feishu",
            "text-original",
            "ou_original",
            1,
            "--person",
            owner.actor,
            "--start",
            1,
        )
    definition = dict(
        id="gateway-text",
        namespace="text-original",
        platform="feishu",
        bot_stage=stage,
        program=sys.executable,
        args=[str(ctx.root / "tests/peers/host-text/peer.py"), str(peer), str(sdk)],
        grants=["gateway.inbound", "gateway.send", "forget.owner"],
    )
    with (owner.svc.data / "yuki.toml").open("a") as configuration:
        configuration.write(
            "\n[[plugins]]\n"
            + "\n".join(k + "=" + json.dumps(v) for k, v in definition.items())
            + "\n"
        )
    observations = {}
    cases = [
        ("inline", "confirmed", 1),
        ("normal", "confirmed", 3),
        ("authority", "confirmed", 3),
        ("quoted", "confirmed", 3),
        ("unknown", "unknown", 2),
        ("gap", "failed", 0),
        ("overlap", "failed", 0),
        ("digest", "failed", 0),
        ("missing-tail", "failed", 0),
        ("utf8", "failed", 0),
        ("lost", "unknown", 2),
        ("erased", None, 0),
    ]
    with Recorder() as model:
        owner.config(
            model,
            extra="root_tool_calls=32\ncompact_soft_tokens=4000000\n",
            binding=False,
        )
        for mode, expected, physical in cases:
            (peer / "mode").write_text(mode)
            (peer / "ready.json").unlink(missing_ok=True)
            with owner.svc.running():
                owner.until(lambda: (peer / "ready.json").exists())
                original = {
                    "source": mode,
                    "native_id": "om_" + mode,
                    "sender": "ou_original",
                    "occurred": 10 + len(observations),
                    "kind": "message",
                    "accept": True,
                    "segments": [{"type": "text", "text": "send original text"}],
                }
                (peer / "probed.json").unlink(missing_ok=True)
                temporary = peer / "probe.tmp"
                temporary.write_text(
                    json.dumps({"method": "gateway.inbound", "params": original})
                )
                temporary.replace(peer / "probe.json")
                owner.until(lambda: (peer / "probed.json").exists())
                event = json.loads((peer / "probed.json").read_bytes())["result"][
                    "event"
                ]
                _, queue = owner.wire(model)
                body = "old inline text" if mode == "inline" else "雪" * 120000
                message = {"text": body}
                if mode == "quoted":
                    message["reply_to_event_id"] = event
                reply(queue, calls=[call("send", "send", message)])
                if mode in {"authority", "erased"}:
                    owner.until(lambda: (peer / ("reading-" + mode + ".json")).exists())
                    held = json.loads(
                        (peer / ("reading-" + mode + ".json")).read_bytes()
                    )
                    ctx.check(
                        "text bytes register the original source hold",
                        owner.rows(
                            "SELECT source FROM source_holds WHERE owner='gateway-text' AND item=? AND source=?",
                            held["handle"],
                            event,
                        ),
                    )
                    if mode == "authority":
                        original = owner.rows(
                            "SELECT grants,revision,generation FROM host_installations WHERE plugin='gateway-text'"
                        )[0]
                        faults = [
                            (
                                "grants",
                                json.dumps(
                                    [g for g in json.loads(original[0]) if g != grant]
                                ),
                            )
                            for grant in ["gateway.send", "forget.owner"]
                        ] + [("revision", "replacement"), ("generation", "replacement")]
                        denied = []
                        for column, value in faults:
                            with closing(
                                sqlite3.connect(owner.svc.data / "core.sqlite")
                            ) as database:
                                database.execute(
                                    "UPDATE host_installations SET "
                                    + column
                                    + "=? WHERE plugin='gateway-text'",
                                    (value,),
                                )
                                database.commit()
                            (peer / "probed.json").unlink()
                            temporary.write_text(
                                json.dumps(
                                    {
                                        "method": "gateway.read_text",
                                        "params": {
                                            "handle": held["handle"],
                                            "offset": held["offset"],
                                            "length": 12288,
                                        },
                                    }
                                )
                            )
                            temporary.replace(peer / "probe.json")
                            owner.until(lambda: (peer / "probed.json").exists())
                            result = json.loads((peer / "probed.json").read_bytes())
                            ctx.check(
                                "original page rejects changed " + column,
                                "error" in result and "result" not in result,
                            )
                            denied.append({"field": column, "reply": result})
                            with closing(
                                sqlite3.connect(owner.svc.data / "core.sqlite")
                            ) as database:
                                database.execute(
                                    "UPDATE host_installations SET grants=?,revision=?,generation=? WHERE plugin='gateway-text'",
                                    original,
                                )
                                database.commit()
                        ctx.write("gateway-text-authority-denied.json", denied)
                    else:
                        erased = owner.op(
                            "privacy",
                            "erase",
                            "erase-paged-original",
                            owner.actor,
                            json.dumps([event]),
                        )
                        owner.until(lambda: (peer / "forgotten.json").exists())
                        (peer / ("resume-" + mode)).touch()
                        owner.until(
                            lambda: (peer / ("fault-" + mode + ".json")).exists()
                        )
                        ctx.check(
                            "erased original emits no physical text",
                            not list(peer.glob("sent-erased-*.json")),
                        )
                        owner.until(
                            lambda: owner.rows(
                                "SELECT id FROM executions WHERE source=? AND state='held'",
                                event,
                            )
                        )
                        ctx.write(
                            "gateway-text-erased.json",
                            {
                                "erasure": erased,
                                "fault": json.loads(
                                    (peer / "fault-erased.json").read_bytes()
                                ),
                            },
                        )
                        observations[mode] = {
                            "event": event,
                            "physical": [],
                            "source_erased": True,
                        }
                        continue
                    (peer / ("resume-" + mode)).touch()
                wire, queue = owner.wire(model)
                ctx.write("gateway-text-" + mode + "-model.json", wire)
                ctx.write(
                    "gateway-text-" + mode + "-peer.json",
                    {
                        path.name: path.read_text()
                        for path in peer.iterdir()
                        if path.suffix in {".json", ".jsonl"}
                    },
                )
                result = json.loads(
                    next(
                        v["content"]
                        for v in wire["messages"]
                        if v.get("tool_call_id") == "send"
                    )
                )
                ctx.equal(mode + " original text result", result["state"], expected)
                sent = [
                    json.loads(p.read_bytes())
                    for p in peer.glob("sent-" + mode + "-*.json")
                ]
                sent.sort(key=lambda x: x["index"])
                ctx.equal(mode + " actual physical text steps", len(sent), physical)
                if expected == "confirmed":
                    ctx.equal(
                        mode + " all original bytes accounted",
                        sum(x["bytes"] for x in sent),
                        len(body.encode()),
                    )
                if mode == "quoted":
                    ctx.equal(
                        "original quote stays on first visible text",
                        sent[0]["reply"],
                        {"event_id": event, "native_id": "om_quoted"},
                    )
                    ctx.check(
                        "later text omits original quote",
                        all(x["reply"] is None for x in sent[1:]),
                    )
                if mode != "inline":
                    planned = json.loads(
                        (peer / ("planned-" + mode + ".json")).read_bytes()
                    )
                    ctx.equal(
                        mode + " complete original text digest",
                        planned["sha256"],
                        hashlib.sha256(body.encode()).hexdigest(),
                    )
                observations[mode] = {
                    "event": event,
                    "receipt": result,
                    "physical": sent,
                }
                reply(queue)
                owner.finished(event)
            with owner.svc.running():
                reopened = owner.op(
                    "effect", "read", result["namespace"], result["id"], owner.actor
                )
                ctx.equal(
                    mode + " original outcome survives cold restart", reopened, result
                )
                ctx.equal(
                    mode + " cold restart adds no physical calls",
                    len(list(peer.glob("sent-" + mode + "-*.json"))),
                    physical,
                )
    wire = recorded_wire(peer)
    frames = [json.loads(line)["frame"] for line in wire.splitlines()]
    sizes = [
        len(json.dumps(frame, separators=(",", ":"), ensure_ascii=False).encode())
        for frame in frames
    ]
    ctx.check("every SDK frame is bounded at 64 KiB", max(sizes) <= 65536)
    ctx.check(
        "real paged text callbacks were received",
        any(frame.get("method") == "gateway.read_text" for frame in frames),
    )
    return {
        "status": "PASS",
        "cases": observations,
        "max_frame_bytes": max(sizes),
        "wire_sha256": hashlib.sha256(wire).hexdigest(),
    }
