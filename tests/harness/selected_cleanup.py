"""Replay original cleanup results through a selected Host without forget.ack."""

import json
import sqlite3
import sys
import time
from contextlib import closing

from tests.harness.service import Service


def exercise(ctx, binary, sdk):
    cases = []
    for fault in ["normal", "malformed-reply", "stale-reply", "lose-ack"]:
        folder = ctx.working / ("cleanup-" + fault)
        folder.mkdir()
        owner = Service(ctx, folder / "owner")
        owner.binary = binary
        owner.argv[0] = str(binary)
        owner.command("init", "--memory-mode", "lexical-only")
        actor = "00000000-0000-4000-8000-000000000022"
        key = "selected-" + fault

        def op(*args):
            return owner.command("operator", *map(str, args))

        def phases():
            log = folder / "phases.jsonl"
            return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []

        def wait(predicate):
            deadline = time.monotonic() + 12
            while time.monotonic() < deadline:
                if predicate():
                    return
                time.sleep(0.05)
            raise AssertionError(f"{fault}: cleanup did not settle: {phases()}")

        with owner.running():
            op("person", "create", actor)
            user = op("account", "open", "telegram", "cleanup", "200", 1,
                      "--person", actor, "--start", 1)
            bot = op("account", "open", "telegram", "cleanup", "100", 1, "--start", 1)
            event = op("ingress", "record", json.dumps({
                "source": key, "account": user["account"], "bot_stage": bot["assignment"],
                "reply_bot": "100", "occurred": 1, "observed": 1, "kind": "message",
                "sender": {"nickname": "Fixture"},
                "segments": [{"type": "text", "text": "owned material"}],
            }), "--accept")["event"]
            op("privacy", "owner", "retained", "true")
            op("privacy", "acquire", "retained", "private.item", event)
            (folder / "private.item").write_text("owned material")
            op("privacy", "owner", "retained", "false")
            ctx.equal(fault + " creates original cleanup obligation",
                      op("privacy", "forget", key, actor)["owners_pending"], ["retained"])
        if fault != "normal":
            (folder / fault).touch()
        (folder / "probe-ack").touch()
        peer = str(ctx.root / "tests/peers/forget/peer.py")
        fields = {
            "id": "retained", "namespace": "selected-cleanup", "program": sys.executable,
            "args": [peer, "normal", str(folder), str(sdk)],
            "erasure_args": [peer, "cleanup", str(folder), str(sdk)],
            "grants": ["forget.owner"],
        }
        with (owner.data / "yuki.toml").open("a") as config:
            config.write("\n[[plugins]]\n" + "".join(f"{k}={json.dumps(v)}\n" for k, v in fields.items()))
        if fault != "normal":
            with owner.running():
                wait(lambda: any(p["event"] == "erased" for p in phases()))
                wait(lambda: all(not p["online"] for p in op("host", "status")["plugins"]))
                ctx.equal(fault + " retains original obligation",
                          [p["key"] for p in op("privacy", "pending", "retained")["obligations"]], [key])
                ctx.check(fault + " cannot start business", not any(p["mode"] == "normal" for p in phases()))
            (folder / fault).unlink(missing_ok=True)
        for cycle in ["settled", "cold-restart"]:
            previous = sum(p["event"] == "ack-rejected" for p in phases())
            with owner.running():
                wait(lambda: sum(p["event"] == "ack-rejected" for p in phases()) > previous)
                ctx.equal(fault + " " + cycle + " persists cleanup result",
                          op("privacy", "pending", "retained")["obligations"], [])
                ctx.check(fault + " " + cycle + " starts business",
                          any(p["online"] for p in op("host", "status")["plugins"]))
                ctx.check(fault + " rejects unselected explicit ack",
                          all(p["response"]["error"]["code"] == -32601
                              for p in phases() if p["event"] == "ack-rejected"))
        log = phases()
        erased = [p for p in log if p["event"] == "erased"]
        ctx.equal(fault + " replays only unsettled original cleanup", len(erased), 1 if fault == "normal" else 2)
        ctx.check(fault + " keeps original watermark and proof",
                  len({(p["key"], p["watermark"], p["proof"]) for p in erased}) == 1)
        ctx.check(fault + " negotiates only selected cleanup method",
                  all(p["methods"] == ["forget.request"] for p in log if p["event"] == "ready"))
        ctx.check(fault + " removes private file before business",
                  not (folder / "private.item").exists()
                  and all(not p["has_private"] for p in log if p["mode"] == "normal"))
        with closing(sqlite3.connect(owner.data / "core.sqlite")) as db:
            receipt = db.execute("SELECT watermark,proof FROM owner_erasures WHERE owner='retained'").fetchone()
        ctx.equal(fault + " stores original result", list(receipt), [erased[-1]["watermark"], erased[-1]["proof"]])
        ctx.write("cleanup-" + fault + ".json", log)
        cases.append({"case": fault, "cleanup_attempts": len(erased), "stored": list(receipt)})
    return cases
