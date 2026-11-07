"""Exercise original account annotations through Main, both Host page methods and durable receipts."""

import hashlib
import json
import sqlite3
import sys
from contextlib import closing
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def run(ctx, sdk, profile="debug"):
    owner = Owner(ctx, "gateway-mentions", 56, profile)
    peer = ctx.working / "mention-peer"
    peer.mkdir()
    (peer / "mode").write_text("mention-names")
    with owner.svc.running():
        bot = owner.op(
            "account", "open", "feishu", "mentions", "cli_original", 1, "--start", 1
        )
        users = {
            native: owner.op(
                "account",
                "open",
                "feishu",
                "mentions",
                native,
                1,
                "--person",
                owner.actor,
                "--start",
                1,
            )
            for native in ["ou_trigger", "ou_first", "ou_second"]
        }
        foreign = owner.op(
            "account",
            "open",
            "feishu",
            "foreign-app",
            "ou_foreign",
            1,
            "--person",
            owner.actor,
            "--start",
            1,
        )
    definition = dict(
        id="gateway-mentions",
        namespace="mentions",
        platform="feishu",
        bot_stage=bot["assignment"],
        program=sys.executable,
        args=[str(ctx.root / "tests/peers/host-text/peer.py"), str(peer), str(sdk)],
        grants=["gateway.inbound", "gateway.send", "forget.owner"],
    )
    with (owner.svc.data / "yuki.toml").open("a") as config:
        config.write(
            "\n[[plugins]]\n"
            + "\n".join(k + "=" + json.dumps(v) for k, v in definition.items())
            + "\n"
        )

    def probe(method, params):
        (peer / "probed.json").unlink(missing_ok=True)
        temporary = peer / "probe.tmp"
        temporary.write_text(json.dumps({"method": method, "params": params}))
        temporary.replace(peer / "probe.json")
        owner.until(lambda: (peer / "probed.json").exists())
        return json.loads((peer / "probed.json").read_bytes())

    def incoming(
        name,
        native="ou_trigger",
        *,
        accept=False,
        occurred=100,
        nickname=None,
        segments=None,
        private=False,
    ):
        params = {
            "source": name,
            "native_id": "om_" + name,
            "sender": native,
            "space": None if private else "oc_original",
            "occurred": occurred,
            "kind": "message",
            "accept": accept,
            "segments": segments
            or [{"type": "text", "text": "original input " + name}],
        }
        if nickname:
            params["sender_profile"] = {"nickname": nickname}
        return probe("gateway.inbound", params)["result"]["event"]

    with owner.svc.running():
        owner.until(lambda: (peer / "ready.json").exists())
        first_source = incoming("first-name", "ou_first", occurred=90, nickname="小雪")
        incoming("second-name", "ou_second", occurred=80, nickname="小雪")
        incoming(
            "long-name",
            occurred=70,
            segments=[{"type": "mention", "native": "ou_second", "label": "@小雪子"}],
        )
    observations = {}
    cases = [
        ("mention-names", "你好 @小雪 @小雪子", "confirmed"),
        ("mention-private", "私聊 @小雪 @外部", "confirmed"),
        (
            "mention-exact",
            "@{account:" + users["ou_second"]["account"] + "}",
            "confirmed",
        ),
        (
            "mention-literal",
            r"\@小雪 https://example.test/@小雪 a@example.test @未知 @{account:"
            + foreign["account"]
            + "}",
            "confirmed",
        ),
        ("mention-large", ("@小雪 " + "雪" * 90 + "\n") * 500, "confirmed"),
        ("mention-dense", "@小雪 " * 11_000, "confirmed"),
        ("mention-unknown", ("@小雪 " + "雪" * 90 + "\n") * 100, "unknown"),
        ("mention-cut", "你好 @小雪", "failed"),
        ("mention-replaced", "你好 @小雪", "failed"),
        ("mention-gap", "你好 @小雪", "failed"),
        ("mention-authority", "你好 @小雪", "confirmed"),
        ("mention-stage", "你好 @小雪子", None),
        ("mention-erased", "你好 @小雪", None),
    ]
    with Recorder() as model:
        owner.config(
            model,
            extra="root_tool_calls=32\ncompact_soft_tokens=4000000\n",
            binding=False,
        )
        for index, (mode, text, expected) in enumerate(cases):
            (peer / "mode").write_text(mode)
            (peer / "ready.json").unlink(missing_ok=True)
            with owner.svc.running():
                owner.until(lambda: (peer / "ready.json").exists())
                event = incoming(
                    mode,
                    accept=True,
                    occurred=100 + index,
                    private=mode == "mention-private",
                    nickname="小雪" if mode == "mention-private" else None,
                    segments=(
                        [
                            {"type": "mention", "native": "ou_first", "label": "@外部"},
                            {"type": "text", "text": "send private"},
                        ]
                        if mode == "mention-private"
                        else None
                    ),
                )
                _, waiting = owner.wire(model)
                reply(waiting, calls=[call("send", "send", {"text": text})])
                if mode in {"mention-authority", "mention-erased", "mention-stage"}:
                    owner.until(lambda: (peer / ("reading-" + mode + ".json")).exists())
                    held = json.loads(
                        (peer / ("reading-" + mode + ".json")).read_bytes()
                    )
                    if mode == "mention-authority":
                        ctx.check(
                            "original name source is held before annotation pages",
                            owner.rows(
                                "SELECT source FROM source_holds WHERE owner=? AND item=? AND source=?",
                                "gateway-mentions",
                                held["handle"],
                                first_source,
                            ),
                        )
                        forged = probe(
                            "gateway.read_mentions",
                            {"handle": held["handle"], "offset": 0, "length": 1,
                             "sha256": "0" * 64},
                        )
                        ctx.check("annotation digest comes only from admission", "error" in forged)
                        original = owner.rows(
                            "SELECT grants,revision,generation FROM host_installations WHERE plugin=?",
                            "gateway-mentions",
                        )[0]
                        faults = [
                            ("grants", json.dumps(["gateway.inbound", "forget.owner"])),
                            ("revision", "foreign"),
                            ("generation", "foreign"),
                        ]
                        for column, value in faults:
                            with closing(
                                sqlite3.connect(owner.svc.data / "core.sqlite")
                            ) as db:
                                db.execute(
                                    "UPDATE host_installations SET "
                                    + column
                                    + "=? WHERE plugin=?",
                                    (value, "gateway-mentions"),
                                )
                                db.commit()
                            denied = probe(
                                "gateway.read_mentions",
                                {"handle": held["handle"], "offset": 0, "length": 1},
                            )
                            ctx.check(
                                "annotation page rejects changed " + column,
                                "error" in denied,
                            )
                            with closing(
                                sqlite3.connect(owner.svc.data / "core.sqlite")
                            ) as db:
                                db.execute(
                                    "UPDATE host_installations SET grants=?,revision=?,generation=? WHERE plugin=?",
                                    (*original, "gateway-mentions"),
                                )
                                db.commit()
                    elif mode == "mention-stage":
                        # Use the supported operator to end the original target binding.
                        owner.op(
                            "account", "end", users["ou_second"]["assignment"], 200
                        )
                    else:
                        owner.op(
                            "privacy",
                            "erase",
                            "erase-original-name",
                            owner.actor,
                            json.dumps([first_source]),
                        )
                        owner.until(lambda: (peer / "forgotten.json").exists())
                    (peer / ("resume-" + mode)).touch()
                    if expected is None:
                        if mode == "mention-erased":
                            owner.until(
                                lambda: (peer / ("fault-" + mode + ".json")).exists()
                            )
                        owner.until(
                            lambda: owner.rows(
                                "SELECT id FROM executions WHERE source=? AND state='held'",
                                event,
                            )
                        )
                        if mode == "mention-erased":
                            ctx.equal(
                                "original mention dependencies redact stored Host material",
                                owner.rows(
                                    "SELECT count(*) FROM host_calls WHERE args<>'{}' AND execution IN(SELECT execution FROM main_dependencies WHERE event=?)",
                                    first_source,
                                )[0][0],
                                0,
                            )
                        ctx.check(
                            mode + " emits zero physical effects",
                            not list(peer.glob("sent-" + mode + "-*.json")),
                        )
                        observations[mode] = {
                            "event": event,
                            "original_authority_revoked": True,
                            "physical": 0,
                        }
                        continue
                wire, waiting = owner.wire(model, timeout=30)
                result = json.loads(
                    next(
                        message["content"]
                        for message in wire["messages"]
                        if message.get("tool_call_id") == "send"
                    )
                )
                ctx.equal(mode + " original result", result["state"], expected)
                planned = json.loads(
                    (peer / ("planned-" + mode + ".json")).read_bytes()
                )
                sent = sorted(
                    [
                        json.loads(path.read_bytes())
                        for path in peer.glob("sent-" + mode + "-*.json")
                    ],
                    key=lambda item: item["index"],
                )
                if expected == "failed":
                    ctx.equal(mode + " zero physical effects", len(sent), 0)
                if expected == "confirmed":
                    ctx.equal(
                        mode + " full original rendered text",
                        "".join(part["text"] for part in sent),
                        planned["text"],
                    )
                ctx.write(
                    mode + "-observed.json",
                    {"planned": planned, "sent": sent, "result": result},
                )
                if mode == "mention-names":
                    ctx.equal(
                        "same-name uses original conversation occurred order",
                        [m["native"] for m in planned["mentions"]],
                        ["ou_first", "ou_second"],
                    )
                if mode == "mention-private":
                    ctx.equal(
                        "private names resolve only original participants",
                        [m["native"] for m in planned["mentions"]],
                        ["ou_trigger"],
                    )
                if mode == "mention-exact":
                    ctx.equal(
                        "exact core account uses original native ID",
                        planned["mentions"][0]["native"],
                        "ou_second",
                    )
                if mode == "mention-literal":
                    ctx.equal(
                        "unknown, foreign, address and escaped names remain literal",
                        planned["mentions"],
                        [],
                    )
                    ctx.check(
                        "literal escape is removed",
                        planned["text"].startswith("@小雪 "),
                    )
                offset = 0
                projected = []
                for part in sent:
                    projected.extend(
                        [
                            {
                                **m,
                                "start": m["start"] + offset,
                                "end": m["end"] + offset,
                            }
                            for m in part["mentions"]
                        ]
                    )
                    offset += len(part["text"].encode())
                if expected == "confirmed":
                    ctx.equal(
                        mode + " each physical annotation is original and rebased",
                        projected,
                        planned["mentions"],
                    )
                operation = owner.rows(
                    "SELECT namespace,operation_id FROM operations WHERE target='send' ORDER BY rowid DESC LIMIT 1"
                )[0]
                reply(waiting, text="finished")
                owner.finished(event)
                observations[mode] = {
                    "event": event,
                    "state": result["state"],
                    "parts": len(sent),
                    "mentions": len(planned["mentions"]),
                }
            if expected is not None:
                with owner.svc.running():
                    original = owner.op(
                        "effect", "read", operation[0], operation[1], owner.actor
                    )
                    ctx.equal(
                        mode + " cold original receipt", original["state"], expected
                    )
                    ctx.equal(
                        mode + " cold zero additional effects",
                        len(list(peer.glob("sent-" + mode + "-*.json"))),
                        len(sent),
                    )
    frames = [
        json.loads(line)["frame"]
        for line in (peer / "wire.jsonl").read_bytes().splitlines()
    ]
    maximum = max(
        len(json.dumps(frame, separators=(",", ":"), ensure_ascii=False).encode())
        for frame in frames
    )
    ctx.check("annotation and text frames remain bounded", maximum <= 65536)
    ctx.check(
        "original annotation pages exercised",
        any(frame.get("method") == "gateway.read_mentions" for frame in frames),
    )
    return {
        "status": "PASS",
        "cases": observations,
        "max_frame_bytes": maximum,
        "wire_sha256": hashlib.sha256((peer / "wire.jsonl").read_bytes()).hexdigest(),
    }
