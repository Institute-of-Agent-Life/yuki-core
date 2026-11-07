"""Project and exercise the witnessed base from the canonical wire definitions."""

import argparse
from collections import Counter, defaultdict
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile

import contract
from source import digest, source_identity

ROOT = Path(__file__).resolve().parents[1]
FREEZE = ROOT / "protocol/plugin/stable/1.0/freeze.json"
INPUT = ROOT / "fixtures/compat/ext-067"
ROLES = {
    "telegram": {
        "initialize",
        "gateway.inbound",
        "gateway.plan_send",
        "gateway.dispatch_part",
    },
    "web-fetch": {"initialize", "tool.call", "tool.status"},
}
CASES = {
    "confirmed",
    "lost-second",
    "revoked",
    "erased-readback",
    "swapped",
    "wrong-handle",
    "missing-method",
}


def require(ok, message):
    if not ok:
        raise ValueError(message)


def load(path):
    return json.loads(path.read_bytes())


def schema_digest(value):
    return digest(
        json.dumps(
            value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
        ).encode()
    )


def methods_from_witness(witness, gateway, freeze):
    methods = {}
    require(
        set(witness["required_method_witnesses"]) == ROLES.keys(),
        "both real consumers required",
    )
    for plugin, required in ROLES.items():
        observations = witness["required_method_witnesses"][plugin]
        require(
            observations.keys() == required,
            f"{plugin}: missing or unwitnessed B0 method",
        )
        for name, proof in observations.items():
            if name == "initialize":
                require(
                    proof["schema_sha256"] == freeze["handshake_sha256"]
                    and proof["direction"] == "from_plugin",
                    "initialize changed",
                )
                continue
            spec = gateway["methods"][name]
            direction = {
                "from_host": "host_to_plugin",
                "from_plugin": "plugin_to_host",
            }[proof["direction"]]
            require(
                spec["direction"] == direction and spec["grant"] == proof["grant"],
                f"{name}: grant or direction changed",
            )
            require(
                schema_digest(spec) == freeze["methods"].get(name),
                f"{name}: witnessed wire contract changed",
            )
            require(
                proof["schema_sha256"] == witness["delivery"]["schema_sha256"],
                f"{name}: different candidate IDL",
            )
            methods[name] = spec
    require(
        methods.keys() == freeze["methods"].keys(),
        "freeze includes unwitnessed methods",
    )
    return {
        **gateway,
        "status": "witnessed-base",
        "methods": dict(sorted(methods.items())),
    }


def audit(root=ROOT):
    freeze = load(root / FREEZE.relative_to(ROOT))
    folder = root / INPUT.relative_to(ROOT)
    manifest = load(folder / "manifest.json")
    require(
        digest((folder / "manifest.json").read_bytes())
        == freeze["acceptance_manifest_sha256"],
        "wrong acceptance manifest",
    )
    require(
        manifest["source_commit"] == freeze["consumer_source"]
        and manifest["status"] == "PASS",
        "wrong consumer acceptance",
    )
    for name, sha in manifest["files"].items():
        require(
            Path(name).name == name and digest((folder / name).read_bytes()) == sha,
            f"wrong consumer bytes: {name}",
        )
    require(
        manifest["files"]["base-consumer-witness.json"] == freeze["witness_sha256"],
        "wrong witness",
    )
    witness = load(folder / "base-consumer-witness.json")
    require(
        set(witness["cases"]) == CASES
        and not witness["privacy"]["persistent_source_private_data"],
        "different privacy branch or missing case",
    )
    require(
        witness["delivery"]["source_commit"] == freeze["producer_candidate"],
        "different witnessed host",
    )
    handshake = load(root / "protocol/plugin/handshake.schema.json")
    handshake["x-yuki"]["sdk_versions"] = {
        "rust": "1.0.0-alpha.14",
        "python": "1.0.0a14",
    }
    witnessed_handshake = (
        json.dumps(handshake, ensure_ascii=False, indent=2) + "\n"
    ).encode()
    require(
        digest(witnessed_handshake) == freeze["handshake_sha256"],
        "candidate handshake changed",
    )
    from surface import additive, encoded, sha

    profile = load(root / "protocol/plugin/stable/1.0/profile.json")
    require(
        profile["version"] == freeze["version"]
        and profile["witness_sha256"] == freeze["witness_sha256"]
        and sha(encoded(profile["surface"])) == profile["surface_sha256"],
        "base projection differs from its original witness",
    )
    gateway = profile["surface"]
    projected = methods_from_witness(witness, gateway, freeze)
    require(
        projected == gateway, "base profile includes unwitnessed methods or metadata"
    )
    current = load(root / "protocol/plugin/gateway/host-preview.json")
    for name, spec in projected["methods"].items():
        additive(spec, current["methods"][name])
    actual = defaultdict(Counter)
    for case, entry in witness["cases"].items():
        require(
            manifest["files"][entry["zip"]] == entry["sha256"],
            f"{case}: archive changed",
        )
        with zipfile.ZipFile(folder / entry["zip"]) as archive:
            require(
                len(archive.namelist()) == len(entry["members"])
                and set(archive.namelist()) == entry["members"].keys(),
                f"{case}: wrong members",
            )
            for name, sha in entry["members"].items():
                require(
                    digest(archive.read(name)) == sha,
                    f"{case}/{name}: wrong frame bytes",
                )
            result = json.loads(archive.read("result.json"))
            require(
                result == entry["result"]
                and result["status"] == "PASS"
                and result["post_restart_effects"] == 0
                and result["redelivered_old_update"],
                f"{case}: restart or replay",
            )
            require(
                result["host"]
                == witness["delivery"]["platforms"]["windows-x86_64"]["binary_sha256"]
                and result["sdk"] == witness["delivery"]["rust_crate_sha256"],
                "unbound host or SDK",
            )
            for plugin in ROLES:
                frames = [
                    json.loads(line)
                    for line in archive.read(plugin + ".jsonl").splitlines()
                ]
                counts = Counter(
                    r["frame"]["method"]
                    for r in frames
                    if r["direction"] in ("from_host", "from_plugin")
                    and "method" in r["frame"]
                )
                expected = {
                    n: v["cases"][case]
                    for n, v in witness["required_method_witnesses"][plugin].items()
                    if case in v["cases"]
                }
                require(
                    counts == expected and counts["initialize"] >= 2,
                    f"{case}/{plugin}: transcript differs",
                )
                actual[plugin].update(counts)
            native = result["native_parts"]
            expected = []
            if case == "confirmed":
                expected = [
                    {"state": "confirmed", "locator": f"8:{i}"} for i in (701, 702, 703)
                ]
            elif case == "lost-second":
                expected = [
                    {"state": "confirmed", "locator": "8:701"},
                    {"state": "unknown"},
                ]
            require(native == expected, f"{case}: original part settlement changed")
            private = result["private_directories_after_kill"]
            require(
                not private["state-telegram"]
                and not private["state-web-fetch"]
                and set(private["cursor"]) == {"101.lock", "101.offset"},
                "persistent private source requires another release profile",
            )
    return freeze, projected, dict(actual)


def support_kit(out, work, root=ROOT):
    freeze, host, counts = audit(root)
    return {
        **projected_kit(out, work, host, freeze["version"], root),
        "profile_argument": "--plugin-base-1",
        "method_counts": counts,
        "witness_sha256": freeze["witness_sha256"],
    }


def projected_kit(
    out,
    work,
    host,
    version,
    root=ROOT,
    *,
    status="witnessed-base",
    description="Yuki witnessed base protocol SDK",
):
    """Build both SDKs from one selected projection using the existing packagers."""
    stage = work / "source"
    stage.mkdir(parents=True)
    for name in ("rust", "python"):
        shutil.copytree(
            root / "sdk" / name,
            stage / "sdk" / name,
            ignore=shutil.ignore_patterns("target", "._*", "__pycache__"),
        )
    for name in ("LICENSE", "NOTICE"):
        shutil.copy2(root / name, stage / name)
    preview = load(root / "protocol/plugin/handshake.schema.json")["x-yuki"][
        "sdk_versions"
    ]
    for path, old in [
        (stage / "sdk/rust/Cargo.toml", preview["rust"]),
        (stage / "sdk/python/pyproject.toml", preview["python"]),
    ]:
        source = path.read_text(encoding="utf-8")
        needle = f'version = "{old}"'
        require(source.count(needle) == 1, "unrecognized SDK input")
        path.write_text(
            source.replace(needle, f'version = "{version}"').replace(
                "Unpublished Yuki plugin stdio handshake preview",
                description,
            ),
            encoding="utf-8",
            newline="\n",
        )
    subprocess.run(
        [
            "cargo",
            "update",
            "--offline",
            "--manifest-path",
            str(stage / "sdk/rust/Cargo.toml"),
            "-p",
            "yuki-plugin-sdk",
        ],
        env=contract.rust_env(),
        check=True,
        capture_output=True,
    )
    schema_dir = stage / "protocol/plugin"
    schema_dir.mkdir(parents=True)
    handshake = load(root / "protocol/plugin/handshake.schema.json")
    meta = handshake["x-yuki"]
    meta.update(status=status, sdk_versions={"rust": version, "python": version})
    # The generated Python SDK exposes positional constructors. Preserve the
    # canonical declaration and field order when changing release metadata.
    (schema_dir / "handshake.schema.json").write_bytes(
        (json.dumps(handshake, ensure_ascii=False, indent=2) + "\n").encode()
    )
    module = importlib.util.spec_from_file_location(
        "yuki_wire_generator", root / "protocol/plugin/generate.py"
    )
    generator = importlib.util.module_from_spec(module)
    module.loader.exec_module(generator)
    for path, data in generator.generate(stage, host).items():
        path.write_bytes(data)
    out.mkdir(parents=True, exist_ok=False)
    identity = source_identity(root)
    wheel = contract.wheel(out, meta, stage, description)
    rust = contract.crate(out, work, meta, identity, stage)
    for name in ("handshake.schema.json", "handshake.ndjson", "method-index.json"):
        shutil.copy2(schema_dir / name, out / name)
    shutil.copy2(stage / "sdk/rust/src/host-preview.json", out / "host-schema.json")
    require(
        (out / "host-schema.json").read_bytes()
        == (stage / "sdk/python/yuki_plugin_sdk/host-preview.json").read_bytes(),
        "SDK projections differ",
    )
    return {
        "version": version,
        "source_commit": identity["commit"],
        "source_tree_sha256": identity["tree_sha256"],
        "source_dirty": identity["dirty"],
        "published": False,
        "methods": ["initialize", *host["methods"]],
        "files": {
            f.name: digest(f.read_bytes())
            for f in sorted(out.iterdir())
            if not f.name.startswith("._")
        },
        "python_sdk": wheel,
        "rust_sdk": rust,
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as tmp:
        report = support_kit(args.out.resolve(), Path(tmp))
    contract.write_json(args.out / "support-manifest.json", report)
