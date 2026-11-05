"""Verify an OCI candidate before docker load, using a separately pinned signing key."""

import argparse
import hashlib
import json
import re
import tarfile
from pathlib import Path
from verify_dist import verify_signature


def hash_file(path):
    with path.open("rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def verify_layout(path):
    with tarfile.open(path) as tar:
        if json.load(tar.extractfile("oci-layout"))["imageLayoutVersion"] != "1.0.0":
            raise ValueError("expected OCI layout 1.0")
        leaves = []

        def visit(ref):
            algorithm, value = ref["digest"].split(":")
            if algorithm != "sha256" or len(value) != 64:
                raise ValueError("unsupported OCI digest")
            member = tar.getmember("blobs/sha256/" + value)
            if not member.isfile() or member.size != ref["size"]:
                raise ValueError("OCI descriptor size differs")
            with tar.extractfile(member) as stream:
                if hashlib.file_digest(stream, "sha256").hexdigest() != value:
                    raise ValueError("OCI blob digest differs")
            kind = ref.get("mediaType", "")
            if "manifest" in kind or "index" in kind:
                obj = json.load(tar.extractfile(member))
                for child in (
                    obj.get("manifests", [])
                    + obj.get("layers", [])
                    + ([obj["config"]] if "config" in obj else [])
                ):
                    visit(child)
            leaves.append(ref["digest"])

        roots = json.load(tar.extractfile("index.json"))["manifests"]
        if not roots:
            raise ValueError("empty OCI index")
        for root in roots:
            visit(root)
        return {
            "roots": [r["digest"] for r in roots],
            "verified_blobs": len(set(leaves)),
        }


def verify_oci(package, manifest, public_key, commit, package_sha, platform):
    package, manifest = Path(package), Path(manifest)
    raw = manifest.read_bytes()
    verify_signature(raw, Path(str(manifest) + ".sig").read_bytes(), public_key)
    m = json.loads(raw)
    if (
        not re.fullmatch(r"[0-9a-f]{40}", commit)
        or m["source_commit"] != commit
        or m["platform"] != platform
        or m["sha256"] != package_sha
        or hash_file(package) != package_sha
    ):
        raise ValueError("OCI source, platform or archive pin mismatch")
    if verify_layout(package) != m["oci"]:
        raise ValueError("signed OCI graph differs")
    return m


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    for name in (
        "package",
        "manifest",
        "public-key",
        "commit",
        "package-sha",
        "platform",
    ):
        p.add_argument("--" + name, required=True)
    print(json.dumps(verify_oci(**vars(p.parse_args()))))
