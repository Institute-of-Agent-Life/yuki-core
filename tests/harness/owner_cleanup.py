import json, subprocess, sys
from tests.harness.compact import Owner


def exercise(ctx):
    sdk = ctx.working / "sdk"
    built = subprocess.run(
        [
            sys.executable,
            str(ctx.root / "ci/publish-contract.py"),
            "build",
            "--out",
            str(ctx.output / "sdk-distributions"),
            "--verify-work",
            str(sdk),
        ],
        text=True,
        capture_output=True,
    )
    if built.returncode:
        ctx.write("sdk-error.json", {"stderr": built.stderr})
    ctx.equal("packaged actual SDKs for private-data owner", built.returncode, 0)
    o = Owner(ctx, "cleanup", 25)
    folder = ctx.working / "retained"
    folder.mkdir()

    def phases():
        log = folder / "phases.jsonl"
        return (
            [json.loads(line) for line in log.read_text().splitlines()]
            if log.exists()
            else []
        )

    with o.svc.running():
        event = o.human("owned material")
        o.op("privacy", "owner", "retained", "true")
        o.op("privacy", "acquire", "retained", "private.item", event)
        (folder / "private.item").write_bytes(b"owned material")
        (folder / "lose-ack").touch()
        o.op("privacy", "owner", "retained", "false")
        logical = o.op("privacy", "forget", "cold-cleanup", o.actor)
        ctx.equal(
            "absent owner is assigned a durable obligation",
            logical["owners_pending"],
            ["retained"],
        )
    config = o.svc.data / "yuki.toml"
    with config.open("a") as f:
        f.write(
            '\n[[plugins]]\nid="retained"\nnamespace="controlled-cleanup"\nprogram='
            + json.dumps(sys.executable)
            + "\nargs="
            + json.dumps(
                [
                    str(ctx.root / "tests/peers/forget/peer.py"),
                    "normal",
                    str(folder),
                    str(sdk / "python-install"),
                ]
            )
            + '\ngrants=["forget.owner","observe.metadata"]\n'
        )
    with o.svc.running():
        ctx.equal(
            "unregistered cleanup entry cannot start normal application on pending private data",
            o.svc.command(
                "operator", "host", "restart", "retained", accepted=False
            ).find("cleanup")
            >= 0,
            True,
        )
        ctx.equal("blocked owner never touched its data directory", phases(), [])
    with config.open("a") as f:
        f.write(
            "erasure_args="
            + json.dumps(
                [
                    str(ctx.root / "tests/peers/forget/peer.py"),
                    "cleanup",
                    str(folder),
                    str(sdk / "python-install"),
                ]
            )
            + "\n"
        )
    with o.svc.running():
        o.until(lambda: any(p["event"] == "erased" for p in phases()))
        ctx.equal(
            "crash after managed removal before ACK retains obligation",
            o.op("privacy", "pending", "retained")["obligations"][0]["key"],
            "cold-cleanup",
        )
        ctx.check(
            "cold program erased its owned file before any business started",
            not (folder / "private.item").exists()
            and not any(p["mode"] == "normal" for p in phases()),
        )
    with o.svc.running():
        o.until(
            lambda: any(
                p["event"] == "ready" and p["mode"] == "normal" for p in phases()
            )
        )
        log = phases()
        cleanup = [p for p in log if p["event"] == "erased"]
        normal = [p for p in log if p["event"] == "start" and p["mode"] == "normal"]
        ctx.check(
            "restarted clean process repeats only the same original watermark and proof",
            len(cleanup) == 2
            and cleanup[0]["proof"] == cleanup[1]["proof"]
            and cleanup[0]["watermark"] == cleanup[1]["watermark"],
        )
        ctx.check(
            "business starts only after actual acknowledgment and no private file",
            len(normal) == 1 and not normal[0]["has_private"],
        )
        ctx.check(
            "cleanup connection negotiates only erase methods",
            all(
                set(p["methods"]) == {"forget.request", "forget.ack"}
                for p in log
                if p["event"] == "ready" and p["mode"] == "erasure_only"
            ),
        )
        ctx.equal(
            "owner settlement survives cold restart",
            o.op("privacy", "forget-status", "cold-cleanup")["owners_pending"],
            [],
        )
    return log
