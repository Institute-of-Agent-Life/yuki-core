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


def build(manifest_path, out, development=False):
    base = manifest_path.parent
    m = json.loads(manifest_path.read_bytes())
    source = source_identity(ROOT)
    if (
        m["card"] not in {"CORE-032", "CORE-033", "CORE-034", "CORE-038", "CORE-050", "CORE-036"}
        or m["status"] != "verified"
        or m["exit_code"] != 0
        or not m["assertions"]
        or not all(a["passed"] for a in m["assertions"])
    ):
        raise ValueError("passing host capability manifest required")
    if m["source"] != source or (source["dirty"] and not development):
        raise ValueError("candidate needs matching source identity and clean commit")

    def receipt(name):
        raw = (base / name).read_bytes()
        if digest(raw) != m["artifacts"].get(name):
            raise ValueError("unverified member " + name)
        return raw

    personal = m["card"] == "CORE-033"
    media = m["card"] == "CORE-038"
    participation = m["card"] == "CORE-036"
    auxiliary = m["card"] == "CORE-050"
    composition = m["card"] == "CORE-034"
    proof_name = {
        "CORE-032": "host-delegation.json",
        "CORE-033": "personal-handoff.json",
        "CORE-038": "media-handle-bridge.json",
        "CORE-050": "aux-attempts.json",
        "CORE-036": "promotion-vs-self.json",
        "CORE-034": "nested-boundary.json",
    }[m["card"]]
    proof = receipt(proof_name)
    data = json.loads(proof)
    sdk = json.loads(receipt("sdk-distributions/candidate.json"))
    binary = (
        ROOT / "target/debug" / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
    )
    raw = binary.read_bytes()
    if digest(raw) != data["binary_sha256"]:
        raise ValueError("binary differs from exercised host")
    idl_path = (
        "sdk/rust/src/host-preview.json"
        if personal or media or composition or auxiliary or participation
        else "protocol/plugin/gateway/host-preview.json"
    )
    idl = (ROOT / idl_path).read_bytes()
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
        "evidence/" + proof_name: proof,
        "LICENSE": (ROOT / "LICENSE").read_bytes(),
        "NOTICE": (ROOT / "NOTICE").read_bytes(),
        "licenses/Vec1.txt": (ROOT / "native/vec1/NOTICE").read_bytes(),
        "licenses/OpenCC.txt": (ROOT / "native/opencc/LICENSES.txt").read_bytes(),
    }
    if participation:
        methods=(ROOT/"protocol/plugin/participation/methods.json").read_bytes()
        if digest(methods)!=data["participation_schema_sha256"] or set(data["negotiated_participation_methods"])!=set(json.loads(methods)["methods"]):
            raise ValueError("participation methods differ from exercised original")
        files["protocol/participation/methods.json"]=methods
    if auxiliary:
        methods=(ROOT/"protocol/plugin/aux-model/methods.json").read_bytes()
        if digest(methods)!=data["aux_schema_sha256"] or set(data["negotiated_aux_methods"])!=set(json.loads(methods)["methods"]):
            raise ValueError("auxiliary methods differ from exercised original")
        files["protocol/aux-model/methods.json"]=methods
    if composition:
        methods=(ROOT/"protocol/plugin/composition/methods.json").read_bytes()
        if digest(methods)!=data["composition_schema_sha256"] or set(data["negotiated_composition_methods"])!=set(json.loads(methods)["methods"]):
            raise ValueError("composition methods differ from exercised original")
        files["protocol/composition/methods.json"]=methods
        files["evidence/vm-recovery-cuts.json"]=receipt("vm-recovery-cuts.json")
    if media:
        methods = (ROOT / "protocol/plugin/media/methods.json").read_bytes()
        if digest(methods) != data["media_schema_sha256"] or set(
            data["negotiated_media_methods"]
        ) != set(json.loads(methods)["methods"]):
            raise ValueError("media methods differ from exercised original")
        files["protocol/media/methods.json"] = methods
    if personal:
        methods = (ROOT / "protocol/plugin/personal/methods.json").read_bytes()
        if digest(methods) != data["personal_schema_sha256"] or set(
            data["negotiated_methods"]
        ) != set(json.loads(methods)["methods"]):
            raise ValueError("personal interface differs from exercised methods")
        files["protocol/personal/methods.json"] = methods
        for index in (
            "protocol/personal/source.json",
            "protocol/connector/4/source.json",
        ):
            index_raw = (ROOT / index).read_bytes()
            files[index] = index_raw
            for member in json.loads(index_raw)["members"]:
                name = member["path"]
                blob = (ROOT / name).read_bytes()
                if digest(blob) != member["sha256"]:
                    raise ValueError("fixed wire changed: " + name)
                files[name] = blob
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
            "PATH": (
                os.path.join(os.environ.get("SystemRoot", "C:\\Windows"), "System32")
                if os.name == "nt"
                else "/usr/bin:/bin"
            )
        }
        if os.name == "nt":
            env["SystemRoot"] = os.environ["SystemRoot"]
        directory = Path(sandbox) / "data"
        for command in ("init", "doctor"):
            result = subprocess.run(
                [str(exe), "--data", str(directory), command, *(["--memory-mode","lexical-only"] if command=="init" else [])],
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
    target = (
        (
            {
                "CORE-032": "yuki-host-preview-",
                "CORE-033": "yuki-personal-preview-",
                "CORE-038": "yuki-media-preview-",
                "CORE-050": "yuki-aux-model-preview-",
                "CORE-036": "yuki-participation-preview-",
                "CORE-034": "yuki-code-preview-",
            }[m["card"]]
        )
        + source["commit"][:12]
        + "-"
        + os_name
        + "-"
        + arch
    )
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
            "card": m["card"],
            "platform": m["platform"],
            "assertions": len(m["assertions"]),
            "manifest_sha256": digest(manifest_path.read_bytes()),
            "clean_path_launch": "verified",
            "consumer": "synthetic SDK subprocesses",
            "stable": False,
        },
        "members": members,
    }
    if personal:
        candidate["personal_schema_revision"] = 1
        candidate["personal_schema_sha256"] = data["personal_schema_sha256"]
        candidate["client_api_revision"] = 9
        candidate["connector_wire_revision"] = 4
        candidate["validated_personal_methods"] = sorted(data["negotiated_methods"])
        candidate["validated_http_routes"] = data["validated_routes"]
    if media:
        candidate["media_schema_revision"] = 1
        candidate["media_schema_sha256"] = data["media_schema_sha256"]
        candidate["validated_media_methods"] = sorted(data["negotiated_media_methods"])
    if participation:
        candidate["participation_schema_revision"]=1
        candidate["participation_schema_sha256"]=data["participation_schema_sha256"]
        candidate["validated_participation_methods"]=data["negotiated_participation_methods"]
        candidate["validation"]["real_extension_consumer"]="pending EXT-025"
    if auxiliary:
        candidate["aux_schema_revision"]=1
        candidate["aux_schema_sha256"]=data["aux_schema_sha256"]
        candidate["validated_aux_methods"]=data["negotiated_aux_methods"]
        candidate["validated_aux_protocols"]=["chat"]+[item["dialect"] for item in data["wires"]]
        candidate["validated_media_protocols"]=["chat"]
        candidate["validation"]["real_extension_consumers"]="pending EXT-025, EXT-030, EXT-032, EXT-033 and EXT-035"
    if composition:
        candidate["composition_schema_revision"]=1
        candidate["composition_schema_sha256"]=data["composition_schema_sha256"]
        candidate["validated_composition_methods"]=data["negotiated_composition_methods"]
        candidate["validation"]["consumer"]="opaque SDK checkpoint fixtures"
        candidate["validation"]["real_vm_consumer"]="pending EXT-023 and EXT-024"
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
