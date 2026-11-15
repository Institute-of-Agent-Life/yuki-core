"""Replay the installed participation worker against an exact selected Host."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
from tempfile import TemporaryDirectory
from zipfile import ZipFile


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def replay(extensions, extension_revision, host_bundle, host_sha256, core_revision, library, output):
    def git(*args):
        return subprocess.check_output(["git", "-C", str(extensions), *args], text=True, timeout=20).strip()

    if git("rev-parse", "HEAD") != extension_revision or git("status", "--porcelain"):
        raise ValueError("Wrong or dirty extension checkout")
    if sha(host_bundle) != host_sha256:
        raise ValueError("Selected Host artifact digest differs")
    contract = extensions / "compat/participation-library/source.lock.json"
    library_pin = json.loads(contract.read_bytes())
    if sha(library) != library_pin["source_sha256"]:
        raise ValueError("Original participation library digest differs")
    output.mkdir(parents=True)
    sys.path.insert(0, str(extensions / "ci"))
    from package import build
    from participation_rule import run as replay_rule
    from participation_semantic import run as replay_review

    system = {"Darwin": "macos", "Linux": "linux", "Windows": "windows"}[platform.system()]
    binary_name = "yuki-core.exe" if system == "windows" else "yuki-core"
    with TemporaryDirectory(prefix="yuki-selected-replay-") as directory, ZipFile(host_bundle) as archive:
        scratch = Path(directory).resolve()
        host = scratch / binary_name
        metadata = json.loads(archive.read(system + "/host-build.json"))
        identity = metadata["source"]
        if identity["commit"] != core_revision or identity["dirty"] or metadata["published"] or metadata["synthetic"]:
            raise ValueError("Unexpected selected Host provenance")
        host.write_bytes(archive.read(system + "/" + binary_name))
        host.chmod(0o700)
        if sha(host) != metadata["binary"]["sha256"]:
            raise ValueError("Selected Host executable digest differs")
        surface = json.loads(subprocess.check_output([host, "plugin-surface"], timeout=20))
        if surface != metadata["profile"] or not surface["selected_by_default"]:
            raise ValueError("Executable differs from the restricted surface")
        if surface["version"] != "1.3.0" or "observer.event" not in surface["methods"]:
            raise ValueError("Selected participation contract required")

        packages = output / "packages"
        unit = build(extensions, "participation", packages)
        worker = packages / ("yuki-participation.exe" if os.name == "nt" else "yuki-participation")
        rule = output / "native-rule"
        rule.mkdir()
        rule_result = replay_rule(scratch / "rule", host, worker, rule)
        review = output / "native-semantic"
        review.mkdir()
        review_result = replay_review(scratch / "review", host, worker, packages / "semantic_bridge.py", library, review)

    report = {
        "status": "PASS", "published": False, "platform": system,
        "core_revision": core_revision, "extension_revision": extension_revision,
        "host_archive_sha256": host_sha256, "host_sha256": metadata["binary"]["sha256"],
        "surface": surface, "unit_version": unit["version"], "worker_sha256": sha(worker),
        "package_manifest_sha256": sha(packages / "package-manifest.json"),
        "library": library_pin, "rule": rule_result, **review_result,
    }
    (output / "ordinary-replay.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(report["platform"], report["host_sha256"], len(report["accepted"]), report["review_attempts"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("extensions", "host-bundle", "library", "out"):
        parser.add_argument("--" + name, required=True, type=lambda value: Path(value).resolve())
    for name in ("extension-revision", "host-sha256", "core-revision"):
        parser.add_argument("--" + name, required=True)
    args = parser.parse_args()
    replay(args.extensions, args.extension_revision, args.host_bundle, args.host_sha256,
           args.core_revision, args.library, args.out)


if __name__ == "__main__":
    main()
