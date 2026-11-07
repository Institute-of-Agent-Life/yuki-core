"""Exercise a selected native Host with installed SDKs and isolated local owners."""

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import traceback

ROOT = Path(__file__).resolve().parents[3]
sys.path[:0] = [str(ROOT), str(ROOT / "ci")]


def exercise(ctx, binary, installed, profile):
    from surface import read, sha
    from tests.harness.service import Service

    descriptor = json.loads(
        subprocess.check_output([binary, "plugin-surface"], timeout=15)
    )
    ctx.equal(
        "compiled selected surface digest",
        descriptor["surface_sha256"],
        profile["surface_sha256"],
    )
    ctx.equal(
        "compiled exact method inventory",
        descriptor["methods"],
        sorted(["initialize", *profile["surface"]["methods"]]),
    )
    ctx.check(
        "selected binary defaults to the restricted wire",
        descriptor["selected_by_default"],
    )
    owner = Service(ctx, ctx.working / "owner")
    owner.binary = binary
    owner.argv[0] = str(binary)
    storage = owner.command("init", "--memory-mode", "lexical-only")
    ctx.equal(
        "ordinary init persists the stable wire",
        (owner.data / "plugin-base-1.0").read_text().strip(),
        "1.0.0",
    )
    with owner.running():
        bot = owner.command(
            "operator",
            "account",
            "open",
            "telegram",
            "selected",
            "1001",
            "1",
            "--start",
            "1",
        )
    configuration = owner.data / "yuki.toml"
    original = configuration.read_text()
    runs = []
    for cycle in ["normal", "cold-restart"]:
        records = {
            name: ctx.working / (name + ".json")
            for name in ["baseline", "selected", "blocked"]
        }
        for record in records.values():
            record.unlink(missing_ok=True)
        definitions = []
        for name, grants in [
            ("baseline", ["tool.call"]),
            ("selected", ["gateway.inbound", "gateway.activity"]),
            ("blocked", ["forget.owner"]),
        ]:
            definition = dict(
                id=name,
                namespace=name,
                program=sys.executable,
                args=[
                    str(ROOT / "tests/peers/base/probe.py"),
                    str(installed / "python-install"),
                    str(records[name]),
                    "initialize",
                ],
                grants=grants,
            )
            if name == "selected":
                definition.update(platform="telegram", bot_stage=bot["assignment"])
                definition["args"].append("original-inbound")
            definitions.append(
                "\n[[plugins]]\n"
                + "\n".join(
                    key + "=" + json.dumps(value) for key, value in definition.items()
                )
                + "\n"
            )
        configuration.write_text(original + "".join(definitions), encoding="utf-8")
        with owner.running():
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline and not all(
                records[n].exists() for n in ["baseline", "selected"]
            ):
                time.sleep(0.05)
            ctx.check(
                cycle + " admitted both peers",
                all(records[n].exists() for n in ["baseline", "selected"]),
            )
            status = owner.command("operator", "host", "status")
            ctx.equal(
                cycle + " restricts installed grants",
                {p["plugin"]: p["online"] for p in status["plugins"]},
                {"baseline": True, "selected": True},
            )
            ctx.check(
                cycle + " never launches unselected namespace",
                not records["blocked"].exists(),
            )
            for name in ["baseline", "selected"]:
                result = read(records[name])
                granted = (
                    {"tool.call"}
                    if name == "baseline"
                    else {"gateway.inbound", "gateway.activity"}
                )
                expected = sorted(
                    [
                        "initialize",
                        *[
                            method
                            for method, spec in profile["surface"]["methods"].items()
                            if spec["grant"] in granted
                        ],
                    ]
                )
                ctx.equal(
                    cycle + " " + name + " exact negotiated methods",
                    sorted(
                        m["name"]
                        for m in result["initialize"]["result"]["method_revisions"]
                    ),
                    expected,
                )
                ctx.equal(
                    cycle + " " + name + " refuses outside callback",
                    result["preview"]["error"]["code"],
                    -32601,
                )
                ctx.write(cycle + "-" + name + ".json", result)
                if name == "selected":
                    admitted = result["inbound"].get("result", {})
                    ctx.check(
                        cycle + " admits the original namespace",
                        type(admitted.get("event")) is int
                        and admitted["event"] > 0
                        and admitted["accepted"] is False,
                    )
                    if runs:
                        ctx.equal(
                            "cold replay returns the original event",
                            admitted["event"],
                            runs[0]["event"],
                        )
            info = owner.command("operator", "database-info")
            ctx.equal(
                cycle + " actual storage reader",
                info["minimum_reader"],
                storage["schema_version"],
            )
            runs.append(
                {
                    "case": cycle,
                    "plugins": status["plugins"],
                    "storage": info,
                    "event": admitted["event"],
                }
            )
    return {
        "status": "PASS",
        "scope": "native-selected-profile-admission",
        "real_capabilities_promoted": [],
        "published": False,
        "paid_calls": 0,
        "binary_sha256": sha(binary.read_bytes()),
        "profile": descriptor,
        "cases": runs,
    }


def main():
    from contract import crosswire, install
    from surface import encoded, read, sdk_surface, sha
    from tests.harness import Context

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", type=Path, required=True)
    parser.add_argument("--kit", type=Path, required=True)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    args.out = args.out.resolve()
    args.out.mkdir(parents=True, exist_ok=False)
    ctx = Context(ROOT, args.out, read(ROOT / "ci/checks.json"))
    result = {"status": "failed", "assertions": ctx.assertions}
    try:
        with tempfile.TemporaryDirectory(prefix="yuki-profile-") as temporary:
            ctx.working = Path(temporary)
            installed = ctx.working / "sdk"
            installed.mkdir()
            kit = read(args.kit / "support-manifest.json")
            profile = read(args.plan)
            for role, key in [("python-sdk", "python_sdk"), ("rust-sdk", "rust_sdk")]:
                ctx.equal(
                    role + " contains exactly the selected surface",
                    sdk_surface(
                        (args.kit / kit[key]).read_bytes(), role, profile["version"]
                    ),
                    profile["surface"],
                )
            commands = install(
                args.kit.resolve(), installed, kit["python_sdk"], kit["rust_sdk"]
            )
            wire = crosswire(commands, installed)
            golden = [
                json.loads(line)
                for line in (args.kit / "handshake.ndjson").read_bytes().splitlines()
            ]
            for language, frames in wire.items():
                ctx.equal(language + " installed selected SDK", frames, golden)
            ctx.write("sdk-wire.json", wire)
            result.update(
                exercise(ctx, args.host.resolve(), installed, read(args.plan))
            )
            result["plan_sha256"] = sha(args.plan.read_bytes())
            result["sdk_sha256"] = {
                key: sha((args.kit / kit[key]).read_bytes())
                for key in ["python_sdk", "rust_sdk"]
            }
    except Exception:
        result["status"] = "failed"
        result["failure"] = traceback.format_exc()
    (args.out / "native-profile.json").write_bytes(encoded(result))
    print(
        json.dumps(
            {
                "status": result["status"],
                "assertions": len(ctx.assertions),
                "output": str(args.out),
            }
        )
    )
    return 0 if result["status"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
