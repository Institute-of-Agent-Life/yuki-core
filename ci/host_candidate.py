"""Pack the exact native host and SDKs exercised by a successful CORE-032 run."""

import argparse
import json
import os
import platform
import subprocess
import tempfile
import zipfile
from pathlib import Path
from source import digest, source_identity

ROOT = Path(__file__).resolve().parents[1]


def tested_contents(receipt, profile):
    proof = receipt("host-delegation.json")
    data = json.loads(proof)
    sdk = json.loads(receipt("sdk-distributions/candidate.json"))
    binary = (
        ROOT
        / "target"
        / profile
        / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
    )
    raw = binary.read_bytes()
    if digest(raw) != data["binary_sha256"]:
        raise ValueError("binary differs from exercised host")
    idl = (ROOT / "protocol/plugin/gateway/host-preview.json").read_bytes()
    if digest(idl) != data["host_idl_sha256"]:
        raise ValueError("host interface differs from tested IDL")
    os_name = platform.system().lower()
    arch = (
        platform.machine()
        .lower()
        .replace("amd64", "x86_64")
        .replace("aarch64", "arm64")
    )
    files = {
        "bin/" + binary.name: raw,
        "protocol/host-preview.json": idl,
        "protocol/handshake.schema.json": (
            ROOT / "protocol/plugin/handshake.schema.json"
        ).read_bytes(),
        "evidence/host-delegation.json": proof,
        "LICENSE": (ROOT / "LICENSE").read_bytes(),
        "NOTICE": (ROOT / "NOTICE").read_bytes(),
        "licenses/OpenCC.txt": (ROOT / "native/opencc/LICENSES.txt").read_bytes(),
    }
    target_info = subprocess.check_output(["rustc", "-vV"], text=True)
    host = next(
        line.split(": ", 1)[1]
        for line in target_info.splitlines()
        if line.startswith("host: ")
    )
    if os.name == "nt" and "windows-gnu" in host:
        runtime = ROOT / "native/windows-runtime"
        prov = json.loads((runtime / "provenance.json").read_bytes())
        compiler = Path(os.environ.get("YUKI_MINGW_BIN", "")) / "g++.exe"
        version = subprocess.check_output(
            [str(compiler), "-dumpfullversion"], text=True
        ).strip()
        if prov["compiler"] != version:
            raise ValueError("missing runtime notices for Windows compiler " + version)
        for name, witness in prov["members"].items():
            raw_notice = (runtime / name).read_bytes()
            if digest(raw_notice) != witness["sha256"]:
                raise ValueError("runtime license digest mismatch")
            files["licenses/GCC/" + name] = raw_notice
        files["licenses/GCC/provenance.json"] = (
            runtime / "provenance.json"
        ).read_bytes()
    for role in ("python-sdk", "rust-sdk", "golden", "verification"):
        item = sdk["members"][role]
        n = item["name"]
        if Path(n).name != n:
            raise ValueError("unsafe distribution member")
        content = receipt("sdk-distributions/" + n)
        if digest(content) != item["sha256"]:
            raise ValueError("SDK digest mismatch")
        files["sdk/" + n] = content
    with tempfile.TemporaryDirectory(prefix="yuki-host-candidate-") as sandbox:
        # The verified binary must also start after transfer without the original compiler or its PATH.
        exe = Path(sandbox) / binary.name
        exe.write_bytes(raw)
        exe.chmod(0o755)
        env = {
            "PATH": os.path.join(
                os.environ.get("SystemRoot", "C:\\Windows"), "System32"
            )
            if os.name == "nt"
            else "/usr/bin:/bin"
        }
        if os.name == "nt":
            env["SystemRoot"] = os.environ["SystemRoot"]
        directory = Path(sandbox) / "data"
        for command in ("init", "doctor"):
            result = subprocess.run(
                [str(exe), "--data", str(directory), command],
                cwd=sandbox,
                env=env,
                capture_output=True,
                timeout=30,
            )
            if result.returncode:
                raise RuntimeError(
                    f"portable host failed {command} ({result.returncode}): "
                    + result.stderr.decode(errors="replace")[:200]
                )
            json.loads(result.stdout)
    return files, os_name, arch, host, sdk, idl


def build(manifest_path, out, development=False):
    base = manifest_path.parent
    m = json.loads(manifest_path.read_bytes())
    source = source_identity(ROOT)
    if (
        m["card"] != "CORE-032"
        or m["status"] != "verified"
        or m["exit_code"] != 0
        or not m["assertions"]
        or not all(a["passed"] for a in m["assertions"])
    ):
        raise ValueError("passing CORE-032 manifest required")
    if m["source"] != source or (source["dirty"] and not development):
        raise ValueError("candidate needs matching source identity and clean commit")

    def receipt(name):
        raw = (base / name).read_bytes()
        if digest(raw) != m["artifacts"].get(name):
            raise ValueError("unverified member " + name)
        return raw

    files, os_name, arch, host, sdk, idl = tested_contents(receipt, "debug")
    target = "yuki-host-preview-" + source["commit"][:12] + "-" + os_name + "-" + arch
    members = {
        n: {"sha256": digest(content), "bytes": len(content)}
        for n, content in sorted(files.items())
    }
    candidate = {
        "status": "unpublished-development-candidate",
        "source_commit": source["commit"],
        "source_tree_sha256": source["tree_sha256"],
        "dirty": source["dirty"],
        "os": os_name,
        "arch": arch,
        "binary_profile": "tested debug",
        "rust_host": host,
        "host_schema_revision": 2,
        "host_schema_sha256": digest(idl),
        "handshake_schema_revision": 1,
        "sdk_versions": sdk["sdk_versions"],
        "implemented_methods": sorted(json.loads(idl)["methods"]),
        "validation": {
            "card": "CORE-032",
            "platform": m["platform"],
            "assertions": len(m["assertions"]),
            "manifest_sha256": digest(manifest_path.read_bytes()),
            "clean_path_launch": "verified",
            "consumer": "synthetic SDK subprocesses",
            "stable": False,
        },
        "members": members,
    }
    files["candidate.json"] = (
        json.dumps(candidate, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    ).encode()
    out.mkdir(parents=True, exist_ok=False)
    zip_path = out / (target + ".zip")
    with zipfile.ZipFile(zip_path, "x") as archive:
        for name, content in sorted(files.items()):
            entry = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
            entry.create_system = 3
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.external_attr = (
                0o100755 if name.startswith("bin/") else 0o100644
            ) << 16
            archive.writestr(entry, content)
    with zipfile.ZipFile(zip_path) as archive:
        if set(archive.namelist()) != files.keys() or any(
            archive.read(n) != value for n, value in files.items()
        ):
            raise ValueError("archive did not round-trip")
    sha = digest(zip_path.read_bytes())
    (out / "candidate.json").write_bytes(files["candidate.json"])
    (out / "SHA256SUMS").write_bytes(f"{sha}  {zip_path.name}\n".encode())
    print(
        json.dumps(
            {"archive": zip_path.name, "sha256": sha, "commit": source["commit"]}
        )
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--development", action="store_true")
    args = parser.parse_args()
    build(args.manifest.resolve(), args.out.resolve(), args.development)
