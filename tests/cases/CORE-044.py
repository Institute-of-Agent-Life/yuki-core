"""Qualify the witnessed base in isolation, then exercise two installed SDKs and a real owner."""

import copy
import json
import os
import shutil
import sys
import time

from tests.harness.service import Service


def run(ctx):
    sys.path.insert(0, str(ctx.root / "ci"))
    import compat
    import contract

    kit = ctx.output / "support"
    work = ctx.working
    report = compat.support_kit(kit, work / "pack", ctx.root)
    contract.write_json(kit / "support-manifest.json", report)
    freeze, host, counts = compat.audit(ctx.root)
    witness = compat.load(
        ctx.root / compat.INPUT.relative_to(compat.ROOT) / "base-consumer-witness.json"
    )
    previous = compat.load(ctx.root / "protocol/plugin/gateway/host-preview.json")
    missing = copy.deepcopy(witness)
    del missing["required_method_witnesses"]["telegram"]["gateway.dispatch_part"]
    ctx.rejects(
        "missing dispatch witness prevents projection",
        ValueError,
        lambda: compat.methods_from_witness(missing, previous, freeze),
    )
    current = copy.deepcopy(previous)
    current["methods"]["gateway.dispatch_part"]["revision"] += 1
    ctx.rejects(
        "changed original dispatch semantics prevent projection",
        ValueError,
        lambda: compat.methods_from_witness(witness, current, freeze),
    )
    install = work / "installed"
    install.mkdir()
    commands = contract.install(kit, install, report["python_sdk"], report["rust_sdk"])
    transcript = contract.crosswire(commands, install)
    golden = [
        json.loads(line)
        for line in (kit / "handshake.ndjson").read_bytes().splitlines()
    ]
    for lang, frames in transcript.items():
        ctx.equal(lang + " installed peer follows the 1.0 golden", frames, golden)

    prior = Service(ctx, work / "existing", "release")
    prior.command("init", "--memory-mode", "lexical-only")
    owner = Service(ctx, work / "base", "release")
    name = "yuki-base.exe" if os.name == "nt" else "yuki-base"
    exe = shutil.copy2(owner.binary, work / name)
    prior.argv[0] = str(exe)
    prior.command("migrate", accepted=False)
    owner.argv[0] = str(exe)
    owner.command("init", "--memory-mode", "lexical-only")
    ctx.equal(
        "named distribution selects stable on first launch",
        (owner.data / "plugin-base-1.0").read_text().strip(),
        "1.0.0",
    )
    # Reopen using the ordinary executable. The data owner retains the selected base.
    owner.argv[0] = str(owner.binary)
    cfg = owner.data / "yuki.toml"
    script = str(ctx.root / "tests/peers/base/probe.py")
    for name, feature, grants in [
        ("good", "initialize", ["tool.call"]),
        ("newer", "unwitnessed", []),
        ("blocked", "initialize", ["forget.owner"]),
    ]:
        args = [
            script,
            str(install / "python-install"),
            str(work / (name + ".json")),
            feature,
        ]
        with cfg.open("a", encoding="utf-8") as f:
            f.write(
                "\n[[plugins]]\n"
                + "\n".join(
                    k + "=" + json.dumps(v)
                    for k, v in dict(
                        id=name,
                        namespace=name,
                        program=sys.executable,
                        args=args,
                        grants=grants,
                    ).items()
                )
                + "\n"
            )
    with owner.running():
        ctx.write("sqlite-build.json", owner.command("operator", "database-info"))
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline and not all(
            (work / (n + ".json")).exists() for n in ("good", "newer")
        ):
            time.sleep(0.05)
        good = compat.load(work / "good.json")
        rejected = compat.load(work / "newer.json")
        online = owner.command("operator", "host", "status")["plugins"]
        ctx.equal(
            "only compatible plugin serves",
            {p["plugin"]: p["online"] for p in online},
            {"good": True, "newer": False},
        )
        ctx.equal(
            "healthy offer exposes only its granted base",
            sorted(m["name"] for m in good["initialize"]["result"]["method_revisions"]),
            sorted(compat.ROLES["web-fetch"]),
        )
        ctx.equal(
            "unnegotiated callback refused", good["preview"]["error"]["code"], -32601
        )
        ctx.check(
            "unsupported feature rejected at handshake",
            "error" in rejected["initialize"],
        )
        ctx.check(
            "unwitnessed grant never launched", not (work / "blocked.json").exists()
        )
    return {
        "version": report["version"],
        "witness_sha256": report["witness_sha256"],
        "source_commit": report["source_commit"],
        "source_tree_sha256": report["source_tree_sha256"],
        "published": False,
        "wire_methods": report["methods"],
        "host_sha256": compat.digest(owner.binary.read_bytes()),
        "consumer_method_counts": counts,
        "schema_sha256": report["files"]["host-schema.json"],
        "support_files": report["files"],
        "installed_handshake": transcript,
        "healthy": good,
        "unavailable": rejected,
        "privacy": freeze["data_profile"],
    }
