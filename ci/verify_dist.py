"""Standalone candidate verification. Trust pins and platform come from the caller."""

import argparse
import hashlib
import json
import os
import platform
import re
import stat
import struct
import subprocess
import tempfile
import zipfile
from pathlib import Path, PurePosixPath

NAMESPACE = "yuki-core-candidate-v1"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def native_target():
    arch = platform.machine().lower()
    return platform.system().lower(), {"amd64": "x86_64", "aarch64": "arm64"}.get(
        arch, arch
    )


def binary_target(data):
    if data.startswith(b"MZ") and len(data) >= 64:
        pe = struct.unpack_from("<I", data, 60)[0]
        if data[pe : pe + 4] != b"PE\0\0":
            raise ValueError("invalid PE signature")
        return "windows", {0x8664: "x86_64", 0xAA64: "arm64"}[
            struct.unpack_from("<H", data, pe + 4)[0]
        ]
    if data.startswith(b"\x7fELF\x02\x01"):
        return "linux", {62: "x86_64", 183: "arm64"}[
            struct.unpack_from("<H", data, 18)[0]
        ]
    if data.startswith(b"\xcf\xfa\xed\xfe"):
        return "darwin", {0x1000007: "x86_64", 0x100000C: "arm64"}[
            struct.unpack_from("<I", data, 4)[0]
        ]
    raise ValueError("unknown native release executable")


def verify_signature(raw, signature_bytes, public_key):
    with tempfile.TemporaryDirectory() as signing:
        signature = Path(signing) / "receipt.sig"
        signature.write_bytes(signature_bytes)
        pub = Path(public_key).read_text(encoding="ascii").split()
        if (
            len(pub) < 2
            or pub[0] != "ssh-ed25519"
            or not re.fullmatch(r"[A-Za-z0-9+/=]+", pub[1])
        ):
            raise ValueError("expected externally pinned Ed25519 public key")
        allowed = Path(signing) / "allowed"
        allowed.write_text(
            "yuki-core " + pub[0] + " " + pub[1] + "\n", encoding="ascii"
        )
        check = subprocess.run(
            [
                "ssh-keygen",
                "-Y",
                "verify",
                "-f",
                str(allowed),
                "-I",
                "yuki-core",
                "-n",
                NAMESPACE,
                "-s",
                str(signature),
            ],
            input=raw,
            capture_output=True,
        )
        if check.returncode:
            raise ValueError("candidate signature differs from trusted key")


def verify(
    package,
    public_key,
    commit,
    package_sha,
    fixtures_sha,
    *,
    target=None,
    destination=None,
):
    package = Path(package)
    if (
        not re.fullmatch("[0-9a-f]{40}", commit)
        or digest(package.read_bytes()) != package_sha
    ):
        raise ValueError("source pin or archive digest mismatch")
    with zipfile.ZipFile(package) as z:
        infos = z.infolist()
        names = [i.filename for i in infos]
        if len(names) != len(set(names)) or len({n.casefold() for n in names}) != len(
            names
        ):
            raise ValueError("duplicate archive member")
        for info in infos:
            path = PurePosixPath(info.filename)
            if (
                not path.parts
                or path.is_absolute()
                or any(
                    part in {".", ".."} or ":" in part or "\\" in part
                    for part in path.parts
                )
                or path.as_posix() != info.filename
                or not stat.S_ISREG(info.external_attr >> 16)
            ):
                raise ValueError("unsafe archive member")
        raw = z.read("candidate.json")
        verify_signature(raw, z.read("candidate.json.sig"), public_key)
        m = json.loads(raw)
        system, arch = target or native_target()
        if (
            (m["os"], m["arch"]) != (system, arch)
            or m["source_commit"] != commit
            or m["binary_profile"] != "release"
        ):
            raise ValueError("target, release profile or commit mismatch")
        if set(names) != set(m["members"]) | {"candidate.json", "candidate.json.sig"}:
            raise ValueError("signed archive inventory mismatch")
        for name, meta in m["members"].items():
            if (
                z.getinfo(name).file_size != meta["bytes"]
                or digest(z.read(name)) != meta["sha256"]
            ):
                raise ValueError("changed member: " + name)
        if m["members"]["fixtures/host-boundaries.json"]["sha256"] != fixtures_sha:
            raise ValueError("fixture pin mismatch")
        idl = z.read("protocol/host-preview.json")
        if (
            digest(idl) != m["host_schema_sha256"]
            or sorted(json.loads(idl)["methods"]) != m["implemented_methods"]
        ):
            raise ValueError("signed capabilities differ from the actual IDL")
        sdk = json.loads(z.read("sdk/distributions.json"))
        if sdk["sdk_versions"] != m["sdk_versions"]:
            raise ValueError("SDK version mismatch")
        for role in ("python-sdk", "rust-sdk", "golden", "verification"):
            item = sdk["members"][role]
            if digest(z.read("sdk/" + item["name"])) != item["sha256"]:
                raise ValueError("SDK fixture or package digest mismatch")
        exe = "bin/yuki-core" + (".exe" if system == "windows" else "")
        if binary_target(z.read(exe)) != (system, arch):
            raise ValueError("executable header differs from signed target")
        if destination is not None:
            destination = Path(destination)
            destination.mkdir(parents=True, exist_ok=False)
            for name in names:
                path = destination.joinpath(*PurePosixPath(name).parts)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(z.read(name))
            (destination / exe).chmod(0o755)
        return m


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    for field in ("package", "public-key", "commit", "package-sha", "fixtures-sha"):
        parser.add_argument("--" + field, required=True)
    parser.add_argument("--destination", type=Path)
    print(json.dumps(verify(**vars(parser.parse_args())), ensure_ascii=False))
