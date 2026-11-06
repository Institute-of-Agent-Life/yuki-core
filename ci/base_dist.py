"""Assemble a signed 1.0 base from the same release binary and SDKs accepted by native CI."""

import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import zipfile

from source import digest
from verify_dist import verify, verify_signature

from verify_base import NAMESPACE, verify_base

ROOT = Path(__file__).resolve().parents[1]


def package(
    candidate, candidate_key, candidate_sha, acceptance, acceptance_sha, signer, out
):
    raw = acceptance.read_bytes()
    if digest(raw) != acceptance_sha:
        raise ValueError("unrecognized verification receipt")
    receipt = json.loads(raw)
    source = receipt["source"]
    if (
        receipt["card"] != "CORE-044"
        or receipt["status"] != "verified"
        or source["dirty"]
    ):
        raise ValueError("a verified clean CORE-044 receipt is required")

    def evidence(name):
        data = (acceptance.parent / name).read_bytes()
        if digest(data) != receipt["artifacts"][name]:
            raise ValueError("changed evidence: " + name)
        return data

    frozen = json.loads(evidence("frozen-wire.json"))
    with zipfile.ZipFile(candidate) as archive:
        meta = json.loads(archive.read("candidate.json"))
        target = meta["os"], meta["arch"]
        validated = verify(
            candidate,
            candidate_key,
            source["commit"],
            candidate_sha,
            source["files"]["fixtures/host/boundaries.json"],
            target=target,
        )
        if (
            validated["dirty"]
            or validated["source_tree_sha256"] != source["tree_sha256"]
        ):
            raise ValueError("native package and SDKs come from different source trees")
        if (
            frozen["source_tree_sha256"] != source["tree_sha256"]
            or frozen["source_commit"] != source["commit"]
        ):
            raise ValueError("freeze receipt comes from different source")
        suffix = ".exe" if target[0] == "windows" else ""
        binary = "bin/yuki-base" + suffix
        files = {binary: archive.read("bin/yuki-core" + suffix)}
        if digest(files[binary]) != frozen["host_sha256"]:
            raise ValueError("CORE-044 tested a different release executable")
        for n in archive.namelist():
            if n.startswith("licenses/") or n in (
                "Cargo.lock",
                "sbom.json",
                "NOTICE",
                "LICENSE",
            ):
                files[n] = archive.read(n)
    for name, sha in frozen["support_files"].items():
        if Path(name).name != name:
            raise ValueError("unsafe support filename")
        data = evidence("support/" + name)
        if digest(data) != sha:
            raise ValueError("unbound support file")
        files["support/" + name] = data
    files["support/support-manifest.json"] = evidence("support/support-manifest.json")
    files["frozen-wire.json"] = evidence("frozen-wire.json")
    for path in (
        "LICENSE",
        "NOTICE",
        "docs/protocol-v1.md",
        "ci/verify_base.py",
        "ci/verify_dist.py",
        "protocol/plugin/stable/1.0/freeze.json",
    ):
        data = (ROOT / path).read_bytes()
        if digest(data) != source["files"][path]:
            raise ValueError("packager source differs: " + path)
        files[path] = data
    manifest = {
        "format": "yuki-plugin-base-v1",
        "version": "1.0.0",
        "source_commit": source["commit"],
        "source_tree_sha256": source["tree_sha256"],
        "platform": target,
        "native_candidate_sha256": candidate_sha,
        "acceptance_sha256": acceptance_sha,
        "methods": frozen["wire_methods"],
        "binary": binary,
        "signature_namespace": NAMESPACE,
        "members": {n: digest(v) for n, v in sorted(files.items())},
    }
    raw = (
        json.dumps(manifest, ensure_ascii=False, sort_keys=True, indent=2) + "\n"
    ).encode()
    out.mkdir(parents=True, exist_ok=True)
    package_path = out / ("yuki-plugin-base-1.0.0-" + "-".join(target) + ".zip")
    with tempfile.TemporaryDirectory() as temp:
        p = Path(temp) / "base.json"
        p.write_bytes(raw)
        subprocess.run(
            ["ssh-keygen", "-Y", "sign", "-f", str(signer), "-n", NAMESPACE, str(p)],
            check=True,
            capture_output=True,
        )
        signature = p.with_suffix(".json.sig").read_bytes()
    verify_signature(raw, signature, str(signer) + ".pub", namespace=NAMESPACE)
    files.update({"base.json": raw, "base.json.sig": signature})
    with zipfile.ZipFile(package_path, "x") as z:
        for n, v in sorted(files.items()):
            entry = zipfile.ZipInfo(n)
            entry.create_system = 3
            entry.external_attr = (0o100755 if n == binary else 0o100644) << 16
            entry.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(entry, v)
    verify_base(
        package_path,
        str(signer) + ".pub",
        digest(package_path.read_bytes()),
        source["commit"],
        target,
    )
    return package_path


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    for name in ("candidate", "candidate-key", "acceptance", "signer", "out"):
        p.add_argument("--" + name, type=Path, required=True)
    for name in ("candidate-sha", "acceptance-sha"):
        p.add_argument("--" + name, required=True)
    result = package(**vars(p.parse_args()))
    print(digest(result.read_bytes()), result)
