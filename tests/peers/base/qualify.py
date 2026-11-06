"""Replay installed 1.0 and candidate consumers against a verified native base package.

Inputs are the original EXT-067 projects with their offline Cargo vendor directories
and (on Windows) the two immutable, independently witnessed candidate packages.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "ci"))
from verify_base import verify_base
from compat import CASES, ROLES


def sha(path):
    with path.open("rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def vendor(crate, project):
    folder = project / "vendor"
    name = crate.name.removesuffix(".crate")
    with tarfile.open(crate) as z:
        files = {}
        for member in z.getmembers():
            rel = Path(member.name)
            if (
                not member.isfile()
                or rel.parts[0] != name
                or ".." in rel.parts
                or rel.is_absolute()
            ):
                raise ValueError("unexpected SDK member")
            data = z.extractfile(member).read()
            path = folder / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            files[rel.relative_to(name).as_posix()] = hashlib.sha256(data).hexdigest()
    (folder / name / ".cargo-checksum.json").write_text(
        json.dumps({"files": files, "package": sha(crate)}), encoding="utf8"
    )


def build(source, unit, variant, work, kit, witness, dest):
    original = source / unit
    project = work / (unit + "-" + variant)
    shutil.copytree(original, project)
    prefix = "plugins/" + unit + "/"
    inputs = {
        n[len(prefix) :]: v
        for n, v in witness["inputs"].items()
        if n.startswith(prefix)
    }
    for name, digest in inputs.items():
        if sha(project / name) != digest:
            raise ValueError("consumer source changed: " + unit + "/" + name)
    if variant == "base":
        sdk = kit / "yuki-plugin-sdk-1.0.0.crate"
        vendor(sdk, project)
        cargo = project / "Cargo.toml"
        text = cargo.read_text(encoding="utf8")
        if text.count("=1.0.0-alpha.14") != 1:
            raise ValueError("unexpected consumer SDK requirement")
        cargo.write_text(text.replace("=1.0.0-alpha.14", "=1.0.0"), encoding="utf8")
        subprocess.run(
            [
                "cargo",
                "update",
                "--offline",
                "-p",
                "yuki-plugin-sdk",
                "--precise",
                "1.0.0",
            ],
            cwd=project,
            check=True,
            capture_output=True,
        )
        sdk_sha = sha(sdk)
    else:
        sdk_sha = witness["delivery"]["rust_crate_sha256"]
    packages = tomllib.loads((project / "Cargo.lock").read_text(encoding="utf8"))[
        "package"
    ]
    old = tomllib.loads((original / "Cargo.lock").read_text(encoding="utf8"))["package"]
    if [p for p in old if p["name"] != "yuki-plugin-sdk"] != [
        p for p in packages if p["name"] != "yuki-plugin-sdk"
    ]:
        raise ValueError("consumer dependencies drifted")
    if [p.get("checksum") for p in packages if p["name"] == "yuki-plugin-sdk"] != [
        sdk_sha
    ]:
        raise ValueError("SDK checksum differs from package lock")
    target = work / ("target-" + unit)
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), CARGO_INCREMENTAL="0")
    with (dest / "build.log").open("wb") as log:
        for command in (["test"], ["build", "--release"]):
            subprocess.run(
                ["cargo", *command, "--locked", "--offline"],
                cwd=project,
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
                check=True,
            )
    binary = "yuki-" + unit + (".exe" if os.name == "nt" else "")
    for path in (
        project / "Cargo.lock",
        project / "plugin.toml",
        target / "release" / binary,
    ):
        shutil.copy2(path, dest / path.name)
    return {n: sha(dest / n) for n in ("Cargo.lock", "plugin.toml", binary)}


def qualify(args):
    base = verify_base(args.package, args.public_key, args.sha256, args.commit)
    witness = json.loads(
        (ROOT / "fixtures/compat/ext-067/base-consumer-witness.json").read_bytes()
    )
    args.out.mkdir(parents=True, exist_ok=False)
    with tempfile.TemporaryDirectory(prefix="ybq-") as tmp:
        work = Path(tmp)
        kit = work / "support"
        kit.mkdir()
        with zipfile.ZipFile(args.package) as z:
            host = work / Path(base["binary"]).name
            host.write_bytes(z.read(base["binary"]))
            host.chmod(0o755)
            for n in ["host-schema.json", "yuki-plugin-sdk-1.0.0.crate"]:
                (kit / n).write_bytes(z.read("support/" + n))
        for variant in ("candidate", "base"):
            folder = args.out / variant
            folder.mkdir()
            pins = {}
            for unit in ROLES:
                dest = folder / unit
                dest.mkdir()
                if variant == "candidate" and args.candidate_packages:
                    original = args.candidate_packages / unit
                    pins[unit] = json.loads(
                        (args.candidate_packages / "pins.json").read_bytes()
                    )[unit]
                    executable = "yuki-" + unit + ".exe"
                    expected_exe = json.loads(
                        (ROOT / "protocol/plugin/stable/1.0/freeze.json").read_bytes()
                    )["windows_replay"]["executable_sha256"][unit]
                    if os.name != "nt" or pins[unit].get(executable) != expected_exe:
                        raise ValueError(
                            "candidate must match the Windows package observed on the previous Host"
                        )
                    for n in ("Cargo.lock", "plugin.toml"):
                        if (
                            pins[unit].get(n)
                            != witness["inputs"]["plugins/" + unit + "/" + n]
                        ):
                            raise ValueError("original package lock or grants changed")
                    for n, expected in pins[unit].items():
                        if Path(n).name != n or sha(original / n) != expected:
                            raise ValueError("candidate package changed")
                        shutil.copy2(original / n, dest / n)
                else:
                    pins[unit] = build(
                        args.projects, unit, variant, work, kit, witness, dest
                    )
            pin = {
                "packages": pins,
                "host": sha(host),
                "source": base["source_commit"],
                "base_archive": args.sha256,
                "schema": sha(kit / "host-schema.json"),
                "sdk": sha(kit / "yuki-plugin-sdk-1.0.0.crate")
                if variant == "base"
                else witness["delivery"]["rust_crate_sha256"],
            }
            pinfile = folder / "pin.json"
            pinfile.write_text(json.dumps(pin, indent=2), encoding="utf8")

            def replay(case):
                with (folder / (case + ".log")).open("wb") as log:
                    subprocess.run(
                        [
                            sys.executable,
                            Path(__file__).with_name("joint.py"),
                            folder,
                            host,
                            pinfile,
                            folder / case,
                            "--case",
                            case,
                        ],
                        check=True,
                        stdout=log,
                        stderr=subprocess.STDOUT,
                        timeout=160,
                    )
                return case, sha(folder / case / "result.json")

            with ThreadPoolExecutor(max_workers=2) as pool:
                cases = dict(pool.map(replay, sorted(CASES)))
            (folder / "qualified.json").write_text(
                json.dumps({"pin": pin, "cases": cases}, indent=2), encoding="utf8"
            )


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    for n in ("package", "public-key", "projects", "out", "candidate-packages"):
        p.add_argument(
            "--" + n,
            type=lambda s: Path(s).resolve(),
            required=n != "candidate-packages",
        )
    for n in ("sha256", "commit"):
        p.add_argument("--" + n, required=True)
    qualify(p.parse_args())
