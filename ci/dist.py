"""Create a signed release host from the same binary, SDKs and IDL exercised by CORE-039."""

import argparse
import json
import os
import subprocess
import tempfile
import tomllib
import zipfile
from pathlib import Path
from host_candidate import tested_contents
from source import source_identity, digest
from verify_dist import NAMESPACE, binary_target

ROOT = Path(__file__).resolve().parents[1]


def json_bytes(obj):
    return (
        json.dumps(obj, ensure_ascii=False, sort_keys=True, indent=2) + "\n"
    ).encode()


def sbom(files, target):
    metadata = json.loads(
        subprocess.check_output(
            [
                "cargo",
                "metadata",
                "--manifest-path",
                str(ROOT / "Cargo.toml"),
                "--format-version",
                "1",
                "--locked",
                "--offline",
                "--filter-platform",
                target,
            ],
            cwd=ROOT,
        )
    )
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    pins = {(p["name"], p["version"]): p for p in lock["package"]}
    resolved = {p["id"] for p in metadata["resolve"]["nodes"]}
    packages = []
    for p in sorted(
        (p for p in metadata["packages"] if p["id"] in resolved),
        key=lambda p: (p["name"], p["version"]),
    ):
        pin = pins[p["name"], p["version"]]
        source = Path(p["manifest_path"]).parent
        notices = (
            set(source.glob("LICENSE*"))
            | set(source.glob("COPYING*"))
            | set(source.glob("NOTICE*"))
        )
        if p.get("license_file"):
            notices.add(source / p["license_file"])
        names = []
        for f in sorted(notices):
            if f.is_file():
                name = f"licenses/cargo/{p['name']}-{p['version']}/{f.name}"
                files[name] = f.read_bytes()
                names.append(name)
        packages.append(
            {
                "name": p["name"],
                "version": p["version"],
                "license": p["license"],
                "notices": names,
                "source": pin.get("source", "candidate source commit"),
                "checksum": pin.get("checksum"),
            }
        )
    return {
        "format": "yuki-cargo-build-inventory-v1",
        "scope": "locked target dependency graph including build inputs",
        "target": target,
        "cargo_lock_sha256": digest((ROOT / "Cargo.lock").read_bytes()),
        "packages": packages,
        "OpenCC": json.loads((ROOT / "native/opencc/provenance.json").read_bytes()),
    }


def archive(out, evidence, key, source, *, development=False, validation=None):
    if source["dirty"] and not development:
        raise ValueError("release requires a clean source commit")
    files, system, arch, host, sdk, idl = tested_contents(evidence, "release")
    binary = next(n for n in files if n.startswith("bin/"))
    if binary_target(files[binary]) != (system, arch):
        raise ValueError("actual release executable is for a different target")
    for name, src in {
        "fixtures/host-boundaries.json": "fixtures/host/boundaries.json",
        "protocol/method-index.json": "protocol/plugin/method-index.json",
        "fixtures/python-peer.py": "tests/peers/host/peer.py",
        "fixtures/rust-peer.rs": "tests/peers/host/rust.rs",
        "Cargo.lock": "Cargo.lock",
        "verify_dist.py": "ci/verify_dist.py",
    }.items():
        files[name] = (ROOT / src).read_bytes()
    files["sdk/distributions.json"] = evidence("sdk-distributions/candidate.json")
    files["sbom.json"] = json_bytes(sbom(files, host))
    files["source-files.json"] = json_bytes(source["files"])
    entries = {
        name: {"sha256": digest(data), "bytes": len(data)}
        for name, data in sorted(files.items())
    }
    m = {
        "format": 1,
        "status": "unpublished-development-candidate",
        "dirty": source["dirty"],
        "source_commit": source["commit"],
        "source_tree_sha256": source["tree_sha256"],
        "os": system,
        "arch": arch,
        "rust_target": host,
        "binary_profile": "release",
        "handshake_schema_revision": 1,
        "host_schema_revision": 2,
        "host_schema_sha256": digest(idl),
        "sdk_versions": sdk["sdk_versions"],
        "implemented_methods": sorted(json.loads(idl)["methods"]),
        "reader_version": max(
            int(f.name.split("_")[0])
            for f in (ROOT / "migrations").glob("[0-9]*_*.sql")
        ),
        "validation": {
            "card": "CORE-039",
            "underlying": "CORE-032 native release conformance",
            "stable": False,
            "verification": validation or {"scope": "in-process conformance trial"},
        },
        "members": entries,
    }
    raw = json_bytes(m)
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "candidate.json"
        path.write_bytes(raw)
        subprocess.run(
            [
                "ssh-keygen",
                "-Y",
                "sign",
                "-q",
                "-f",
                str(key),
                "-n",
                NAMESPACE,
                str(path),
            ],
            check=True,
            capture_output=True,
        )
        files["candidate.json.sig"] = (path.parent / "candidate.json.sig").read_bytes()
    files["candidate.json"] = raw
    out = Path(out)
    out.mkdir(parents=True, exist_ok=False)
    path = out / f"yuki-core-{source['commit'][:12]}-{system}-{arch}.zip"
    with zipfile.ZipFile(path, "x") as z:
        for name, data in sorted(files.items()):
            i = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
            i.create_system = 3
            i.external_attr = (0o100755 if name == binary else 0o100644) << 16
            i.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(i, data)
    (out / "candidate.json").write_bytes(raw)
    (out / "candidate.json.sig").write_bytes(files["candidate.json.sig"])
    (out / "SHA256SUMS").write_text(
        f"{digest(path.read_bytes())}  {path.name}\n", encoding="ascii"
    )
    return path, m


def signed_native(manifest, out, key):
    m = json.loads(manifest.read_bytes())
    source = source_identity(ROOT)
    if (
        m["card"] != "CORE-039"
        or m["status"] != "verified"
        or m["source"] != source
        or m["exit_code"] != 0
        or not m["assertions"]
        or not all(a["passed"] for a in m["assertions"])
    ):
        raise ValueError("verified release conformance from this exact source required")

    def receipt(name):
        content = (manifest.parent / name).read_bytes()
        if digest(content) != m["artifacts"].get(name):
            raise ValueError("unverified conformance artifact " + name)
        return content

    return archive(
        out,
        receipt,
        key,
        source,
        validation={
            "manifest_sha256": digest(manifest.read_bytes()),
            "passed_assertions": len(m["assertions"]),
            "platform": m["platform"],
        },
    )


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--manifest", type=Path, required=True)
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--signing-key", type=Path, required=True)
    a = p.parse_args()
    path, m = signed_native(a.manifest, a.out, a.signing_key)
    print(
        json.dumps(
            {
                "archive": str(path),
                "sha256": digest(path.read_bytes()),
                "commit": m["source_commit"],
            }
        )
    )
