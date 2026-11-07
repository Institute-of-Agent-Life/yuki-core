"""Immutable local minor promotion, using the witnessed base and labelled synthetic additions."""

import copy
import io
import json
import subprocess
import sys
import tarfile
import zipfile


def evidence(folder, planned, *, synthetic=True, sdk_files=None):
    from surface import encoded, sha

    folder.mkdir()

    def write(name, data):
        (folder / name).write_bytes(data)
        return {"path": name, "sha256": sha(data)}

    assets = {
        "host-"
        + system: write("host-" + system, ("synthetic native host " + system).encode())
        for system in ["macos", "linux", "windows"]
    }
    with zipfile.ZipFile(folder / "sdk.whl", "x") as archive:
        archive.writestr(
            "yuki_plugin_sdk/host-preview.json", encoded(planned["surface"])
        )
        archive.writestr(
            "yuki_plugin_sdk.dist-info/METADATA",
            "Version: " + planned["version"] + "\n",
        )
    with tarfile.open(folder / "sdk.crate", "w:gz") as archive:
        for name, data in {
            "sdk/src/host-preview.json": encoded(planned["surface"]),
            "sdk/Cargo.toml": (
                '[package]\nversion = "' + planned["version"] + '"\n'
            ).encode(),
        }.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
    for role, name in [("rust-sdk", "sdk.crate"), ("python-sdk", "sdk.whl")]:
        if sdk_files is not None:
            (folder / name).write_bytes(sdk_files[role].read_bytes())
        assets[role] = {"path": name, "sha256": sha((folder / name).read_bytes())}
    plan_digest = sha(encoded(planned))
    report = {
        "synthetic": synthetic,
        "plan_sha256": plan_digest,
        "candidate_source": planned["candidate_source"],
        "assets": assets,
        "platforms": {},
    }
    for system in ["macos", "linux", "windows"]:
        hashes = {
            role: assets[role]["sha256"]
            for role in ["host-" + system, "rust-sdk", "python-sdk"]
        }
        consumers = {}
        for consumer in planned["consumers"]:
            package = write(
                consumer + ".bin", ("synthetic consumer " + consumer).encode()
            )
            cases = {}
            frames = []
            for method, choice in planned["selection"].items():
                frame = {"method": method, "id": 1}
                for pointer in choice.get("fields", []):
                    value = frame
                    keys = pointer[::2]
                    for key in keys[:-1]:
                        value = value.setdefault(key, {})
                    value[keys[-1]] = "synthetic observation"
                frames.append({"direction": "from_plugin", "frame": frame})
            for case in ["normal", "denied", "restart"]:
                name = system + "-" + consumer + "-" + case
                transcript = write(
                    name + ".jsonl",
                    b"".join(json.dumps(frame).encode() + b"\n" for frame in frames),
                )
                cases[case] = write(
                    name + ".json",
                    encoded(
                        {
                            "status": "PASS",
                            "assertions": {case: True},
                            "plan_sha256": plan_digest,
                            "artifacts": hashes,
                            "consumer_sha256": package["sha256"],
                            "advertised_methods": sorted(
                                ["initialize", *planned["surface"]["methods"]]
                            ),
                            "methods": list(planned["selection"]),
                            "transcript": transcript,
                        }
                    ),
                )
            consumers[consumer] = {
                "source_commit": "b" * 40,
                "package": package,
                "cases": cases,
            }
        report["platforms"][system] = {"artifacts": hashes, "consumers": consumers}
    (folder / "evidence.json").write_bytes(encoded(report))
    (folder / "unrelated.txt").write_text(
        "unrelated material must not enter the package"
    )
    return report


def run(ctx):
    sys.path.insert(0, str(ctx.root / "ci"))
    from compat import audit, projected_kit
    from surface import (
        canonical,
        encoded,
        initialize,
        latest,
        plan,
        project,
        promote,
        sha,
        sdk_surface,
    )

    freeze, surface, counts = audit(ctx.root)
    ctx.equal(
        "base originates in the real B0 consumer witnesses",
        set(counts),
        {"telegram", "web-fetch"},
    )
    idl, files = canonical(ctx.root)
    base = {
        "version": freeze["version"],
        "surface": surface,
        "consumers": ["telegram", "web-fetch"],
        "min_reader": "1.0.0",
        "witness_sha256": freeze["witness_sha256"],
        "handshake_sha256": idl["handshake_sha256"],
    }
    base_path = ctx.working / "base.json"
    base_path.write_bytes(encoded(base))
    digest = sha(base_path.read_bytes())
    registry = ctx.working / "registry"
    initialize(registry, base_path, digest, synthetic=True)
    idl = copy.deepcopy(idl)
    name = "gateway.inbound"
    for field in ["synthetic_a", "synthetic_b", "unwitnessed"]:
        idl["methods"][name]["params"]["properties"][field] = {"type": "string"}

    def request(field, consumer):
        return {
            "parent_stable_digest": digest,
            "candidate_source": "a" * 40,
            "consumers": [consumer],
            "selection": {name: {"fields": [["params", "properties", field]]}},
        }

    a_request, b_request = request("synthetic_a", "consumer-a"), request(
        "synthetic_b", "consumer-b"
    )
    a, b = plan(registry, a_request, idl, files), plan(registry, b_request, idl, files)
    ctx.rejects(
        "initialize semantics stay on the original stable contract",
        ValueError,
        lambda: plan(registry, a_request, {**idl, "handshake_sha256": "0" * 64}, files),
    )
    ctx.equal(
        "both independent proposals initially claim the same minor",
        (a["version"], b["version"]),
        ("1.1.0", "1.1.0"),
    )
    ctx.check(
        "unselected additions are excluded",
        "unwitnessed" not in a["surface"]["methods"][name]["params"]["properties"],
    )
    ctx.equal(
        "existing methods stay in the union",
        set(a["surface"]["methods"]),
        set(surface["methods"]),
    )
    sdks = ctx.output / "synthetic-selected-sdks"
    built = projected_kit(
        sdks,
        ctx.working / "sdk-build",
        a["surface"],
        a["version"],
        ctx.root,
        status="synthetic-release-workflow-control",
        description="Synthetic promotion workflow control",
    )
    for kind, member in [
        ("rust-sdk", built["rust_sdk"]),
        ("python-sdk", built["python_sdk"]),
    ]:
        ctx.equal(
            kind + " is generated from exactly the selected union",
            sdk_surface((sdks / member).read_bytes(), kind, a["version"]),
            a["surface"],
        )
    ctx.write(
        "synthetic-sdk-manifest.json",
        {**built, "synthetic": True, "real_capabilities_promoted": []},
    )
    for mutation, change in [
        ("changed grant", lambda method: method.update(grant="more-privileged")),
        (
            "new required field",
            lambda method: method["params"]["required"].append("synthetic_a"),
        ),
        (
            "changed old value meaning",
            lambda method: method["params"]["properties"]["source"].update(minLength=2),
        ),
    ]:
        changed = copy.deepcopy(idl)
        change(changed["methods"][name])
        ctx.rejects(
            mutation,
            ValueError,
            lambda: project(surface, changed, a_request["selection"]),
        )
    changed = copy.deepcopy(idl)
    changed["methods"][name]["params"]["properties"]["synthetic_a"][
        "default"
    ] = "enabled"
    ctx.rejects(
        "new optional defaults cannot silently add behavior",
        ValueError,
        lambda: project(surface, changed, a_request["selection"]),
    )

    a_folder, b_folder = ctx.working / "a", ctx.working / "b"
    a_proof = evidence(a_folder, a)
    evidence(b_folder, b)
    real = ctx.working / "real-registry"
    initialize(real, base_path, digest)
    ctx.rejects(
        "synthetic control cannot create a release-ready package",
        ValueError,
        lambda: promote(real, a, a_folder, idl, files),
    )
    ctx.equal("formal registry remains empty", list((real / "versions").iterdir()), [])
    for failure in ["consumer", "business", "host", "wire", "plan"]:
        broken = copy.deepcopy(a_proof)
        if failure == "consumer":
            del broken["platforms"]["linux"]["consumers"]["web-fetch"]
        elif failure == "business":
            del broken["platforms"]["macos"]["consumers"]["consumer-a"]["cases"][
                "denied"
            ]
        elif failure == "host":
            broken["platforms"]["windows"]["artifacts"]["host-windows"] = "0" * 64
        elif failure == "wire":
            broken["platforms"]["macos"]["consumers"]["consumer-a"]["cases"]["normal"][
                "sha256"
            ] = ("0" * 64)
        else:
            broken["plan_sha256"] = "0" * 64
        (a_folder / "evidence.json").write_bytes(encoded(broken))
        ctx.rejects(
            "reject missing or mismatched " + failure,
            ValueError,
            lambda: promote(registry, a, a_folder, idl, files),
        )
    (a_folder / "evidence.json").write_bytes(encoded(a_proof))

    for mutation in [
        "missing-field-wire",
        "extra-advertised-method",
        "failed-business-assertion",
    ]:
        folder = ctx.working / mutation
        broken = evidence(folder, a)
        reference = broken["platforms"]["macos"]["consumers"]["consumer-a"]["cases"][
            "normal"
        ]
        report_path = folder / reference["path"]
        report = json.loads(report_path.read_bytes())
        if mutation == "missing-field-wire":
            wire = folder / report["transcript"]["path"]
            wire.write_bytes(
                encoded(
                    {"direction": "from_plugin", "frame": {"method": name}}
                ).replace(b"\n", b"")
                + b"\n"
            )
            report["transcript"]["sha256"] = sha(wire.read_bytes())
        elif mutation == "extra-advertised-method":
            report["advertised_methods"].append("unwitnessed.effect")
        else:
            report["assertions"]["original_effect"] = False
        report_path.write_bytes(encoded(report))
        reference["sha256"] = sha(report_path.read_bytes())
        (folder / "evidence.json").write_bytes(encoded(broken))
        ctx.rejects(
            mutation, ValueError, lambda: promote(registry, a, folder, idl, files)
        )

    interrupted = ctx.working / "interrupted-registry"
    initialize(interrupted, base_path, digest, synthetic=True)
    crash_inputs = ctx.working / "crash-inputs.json"
    crash_inputs.write_bytes(encoded({"plan": a, "idl": idl, "files": files}))
    crash = """import os,sys
from pathlib import Path
sys.path.insert(0,sys.argv[1])
import surface
data=surface.read(Path(sys.argv[2]))
original=surface.deterministic_zip
def die(*args):
 original(*args)
 os._exit(7)
surface.deterministic_zip=die
surface.promote(Path(sys.argv[3]),data['plan'],Path(sys.argv[4]),data['idl'],data['files'])
"""
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            crash,
            str(ctx.root / "ci"),
            str(crash_inputs),
            str(interrupted),
            str(a_folder),
        ],
        capture_output=True,
        timeout=30,
    )
    ctx.equal(
        "process exits at the complete ZIP before publication", result.returncode, 7
    )
    ctx.equal(
        "interrupted staging never becomes a stable version",
        latest(interrupted)[1],
        digest,
    )
    recovered = promote(interrupted, a, a_folder, idl, files)
    ctx.equal(
        "retry after interrupted staging qualifies the original minor",
        recovered["version"],
        "1.1.0",
    )

    # Competing processes exercise the OS lock and the final compare-and-rename.
    fixture = ctx.working / "inputs.json"
    fixture.write_bytes(encoded({"idl": idl, "files": files, "a": a, "b": b}))
    child = """import json,sys,time
from pathlib import Path
sys.path.insert(0,sys.argv[1])
import surface
from surface import promote,read,encoded
data=read(Path(sys.argv[2])); registry=Path(sys.argv[3]); choice=sys.argv[4]
original=surface.qualify
def together(*args):
 result=original(*args)
 (registry.parent/('ready-'+choice)).write_text('qualified')
 deadline=time.monotonic()+15
 while not all((registry.parent/('ready-'+name)).exists() for name in ['a','b']):
  if time.monotonic()>deadline:raise RuntimeError('qualification barrier timeout')
  time.sleep(0.02)
 return result
surface.qualify=together
try:
 result=promote(registry,data[choice],registry.parent/choice,data['idl'],data['files'])
 print(result['version'])
except ValueError as error:
 print(str(error));sys.exit(3)
"""
    processes = [
        subprocess.Popen(
            [
                sys.executable,
                "-c",
                child,
                str(ctx.root / "ci"),
                str(fixture),
                str(registry),
                choice,
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        for choice in ["a", "b"]
    ]
    outcomes = []
    for proc in processes:
        stdout, stderr = proc.communicate(timeout=30)
        outcomes.append(
            {
                "exit_code": proc.returncode,
                "stdout": stdout.decode(),
                "stderr": stderr.decode(),
            }
        )
    ctx.equal(
        "one competing minor succeeds and the stale one must requalify",
        sorted(r["exit_code"] for r in outcomes),
        [0, 3],
    )
    winner, winner_digest = latest(registry)
    before = (registry / "versions/1.1.0/package.zip").read_bytes()
    losing_request = b_request if "consumer-a" in winner["consumers"] else a_request
    losing_proof = b_folder if losing_request == b_request else a_folder
    rebased = plan(registry, losing_request, idl, files)
    ctx.check(
        "latest stable forces a new version and visible rebase",
        rebased["rebased"] and rebased["version"] == "1.2.0",
    )
    ctx.equal(
        "rebased parent binds the winning immutable manifest",
        rebased["parent_stable_digest"],
        winner_digest,
    )
    ctx.rejects(
        "earlier replay cannot be reused after rebase",
        ValueError,
        lambda: promote(registry, rebased, losing_proof, idl, files),
    )
    fresh = ctx.working / "rebased"
    evidence(fresh, rebased)
    final = promote(registry, rebased, fresh, idl, files)
    ctx.equal(
        "the first minor remains byte-identical",
        (registry / "versions/1.1.0/package.zip").read_bytes(),
        before,
    )
    properties = final["surface"]["methods"][name]["params"]["properties"]
    ctx.check(
        "rebase retains both independent qualified fields",
        "synthetic_a" in properties and "synthetic_b" in properties,
    )
    ctx.check(
        "unwitnessed canonical field remains excluded", "unwitnessed" not in properties
    )
    ctx.equal(
        "all old and new consumer obligations survive",
        final["consumers"],
        ["consumer-a", "consumer-b", "telegram", "web-fetch"],
    )
    with zipfile.ZipFile(registry / "versions/1.2.0/package.zip") as archive:
        ctx.check(
            "unrelated files never enter a release",
            not any(n.endswith("unrelated.txt") for n in archive.namelist()),
        )
        ctx.check(
            "original wire and business receipts are packaged",
            any(n.endswith(".jsonl") for n in archive.namelist()),
        )
    ctx.equal(
        "local registry has no partial staging directory",
        sorted(p.name for p in registry.iterdir()),
        [".lock", "base.json", "registry.json", "versions"],
    )
    cli_registry = ctx.working / "cli-registry"

    def cli(*args):
        result = subprocess.run(
            [sys.executable, str(ctx.root / "ci/promote-surface.py"), *map(str, args)],
            capture_output=True,
            text=True,
            timeout=45,
        )
        ctx.equal("CLI " + args[0] + " succeeds", result.returncode, 0)
        return json.loads(result.stdout) if result.stdout else None

    cli(
        "init",
        "--registry",
        cli_registry,
        "--base",
        base_path,
        "--sha256",
        digest,
        "--synthetic",
    )
    request_file = ctx.working / "cli-request.json"
    request_file.write_bytes(
        encoded(
            {
                "parent_stable_digest": digest,
                "candidate_source": "a" * 40,
                "consumers": ["consumer-activity"],
                "selection": {"gateway.activity": {"new_method": True}},
            }
        )
    )
    cli_plan_file = ctx.output / "synthetic-cli-plan.json"
    cli_plan = cli(
        "plan",
        "--registry",
        cli_registry,
        "--request",
        request_file,
        "--out",
        cli_plan_file,
    )
    cli_sdks = ctx.output / "synthetic-cli-sdks"
    kit = cli(
        "build-sdks",
        "--registry",
        cli_registry,
        "--plan",
        cli_plan_file,
        "--out",
        cli_sdks,
    )
    import contract

    installed = ctx.working / "installed-selected-sdks"
    installed.mkdir()
    commands = contract.install(cli_sdks, installed, kit["python_sdk"], kit["rust_sdk"])
    wire = contract.crosswire(commands, installed)
    golden = [
        json.loads(line)
        for line in (cli_sdks / "handshake.ndjson").read_bytes().splitlines()
    ]
    for language, frames in wire.items():
        ctx.equal(
            language + " installed selected SDK follows the generated handshake",
            frames,
            golden,
        )
    ctx.write("synthetic-installed-sdk-wire.json", wire)
    cli_evidence = ctx.working / "cli-evidence"
    evidence(
        cli_evidence,
        cli_plan,
        sdk_files={
            "rust-sdk": cli_sdks / kit["rust_sdk"],
            "python-sdk": cli_sdks / kit["python_sdk"],
        },
    )
    cli_manifest = cli(
        "promote",
        "--registry",
        cli_registry,
        "--plan",
        cli_plan_file,
        "--evidence",
        cli_evidence,
    )
    ctx.equal(
        "public CLI keeps synthetic controls labelled",
        cli_manifest["qualification"],
        "synthetic-control",
    )
    ctx.write("synthetic-cli-manifest.json", cli_manifest)
    ctx.write("synthetic-1.1-manifest.json", winner)
    ctx.write("synthetic-1.2-manifest.json", final)
    saved = ctx.output / "synthetic-registry"
    saved.mkdir()
    for member in [
        "base.json",
        "registry.json",
        "versions/1.1.0/manifest.json",
        "versions/1.1.0/package.zip",
        "versions/1.2.0/manifest.json",
        "versions/1.2.0/package.zip",
    ]:
        destination = saved / member
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes((registry / member).read_bytes())
    return {
        "status": "PASS",
        "kind": "release-workflow-synthetic-control",
        "real_base_witness": freeze["witness_sha256"],
        "concurrent_outcomes": outcomes,
        "qualified_synthetic_versions": ["1.1.0", "1.2.0"],
        "real_capabilities_promoted": [],
        "published": False,
        "paid_calls": 0,
        "package_sha256": final["package_sha256"],
        "min_reader": final["min_reader"],
    }
