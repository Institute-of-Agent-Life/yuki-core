"""Check a base archive against an independently obtained archive digest, key and source."""

import argparse
import json
from pathlib import Path
import stat
import zipfile

from verify_dist import binary_target, digest, native_target, verify_signature

NAMESPACE = "yuki-plugin-base-1.0"


def verify_base(package, public_key, sha256, commit, target=None):
    if digest(package.read_bytes()) != sha256:
        raise ValueError("base archive SHA-256 mismatch")
    with zipfile.ZipFile(package) as archive:
        raw = archive.read("base.json")
        verify_signature(
            raw, archive.read("base.json.sig"), public_key, namespace=NAMESPACE
        )
        manifest = json.loads(raw)
        platform = target or native_target()
        if (
            manifest["format"] != "yuki-plugin-base-v1"
            or manifest["version"] != "1.0.0"
            or manifest["source_commit"] != commit
            or tuple(manifest["platform"]) != platform
        ):
            raise ValueError("unexpected base version, source or platform")
        expected = set(manifest["members"]) | {"base.json", "base.json.sig"}
        names = archive.namelist()
        if set(names) != expected or len(names) != len({n.casefold() for n in names}):
            raise ValueError("invalid base inventory")
        for name, sha in manifest["members"].items():
            if any(
                p in ("", ".", "..") or ":" in p or "\\" in p for p in name.split("/")
            ) or not stat.S_ISREG(archive.getinfo(name).external_attr >> 16):
                raise ValueError("unsafe base member")
            if digest(archive.read(name)) != sha:
                raise ValueError("changed base member: " + name)
        binary = "bin/yuki-base" + (".exe" if platform[0] == "windows" else "")
        if (
            manifest["binary"] != binary
            or binary_target(archive.read(binary)) != platform
        ):
            raise ValueError("unexpected base executable")
        freeze = json.loads(archive.read("protocol/plugin/stable/1.0/freeze.json"))
        schema = json.loads(archive.read("support/host-schema.json"))
        support = json.loads(archive.read("support/support-manifest.json"))
        if (
            support["source_commit"] != commit
            or support["source_tree_sha256"] != manifest["source_tree_sha256"]
            or support["source_dirty"]
        ):
            raise ValueError("unexpected SDK source")
        if (
            schema["status"] != "witnessed-base"
            or sorted(schema["methods"]) != sorted(freeze["methods"])
            or sorted(manifest["methods"]) != sorted(["initialize", *freeze["methods"]])
            or support["methods"] != manifest["methods"]
        ):
            raise ValueError("stable contract is inconsistent")
        for name, schema_method in schema["methods"].items():
            canonical = json.dumps(
                schema_method, ensure_ascii=False, sort_keys=True, separators=(",", ":")
            ).encode()
            if digest(canonical) != freeze["methods"][name]:
                raise ValueError("frozen method semantics differ: " + name)
        for name, sha in support["files"].items():
            if manifest["members"].get("support/" + name) != sha:
                raise ValueError("stable support inventory differs")
        return manifest


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("--public-key", required=True, type=Path)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--commit", required=True)
    print(json.dumps(verify_base(**vars(parser.parse_args())), indent=2))
