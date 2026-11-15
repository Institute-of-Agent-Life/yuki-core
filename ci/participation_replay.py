"""Replay the installed participation worker against an exact selected Host."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import runpy
import subprocess
import sys
import tarfile
from tempfile import TemporaryDirectory
from zipfile import ZipFile

from source import source_identity


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def replay_self(extensions, scratch, output, host, worker, library, commit):
    source = scratch / "library"
    with tarfile.open(library) as original:
        original.extractall(source, filter="data")
    sys.path.insert(0, str(source / ("Yuki-Semantic-Participation-" + commit) / "src"))
    case = runpy.run_path(str(extensions / "tests/cases/EXT-026.py"))
    adapter = worker.with_name("semantic_bridge.py")
    report = {"fixed_clock": case["replay_clock"](scratch, adapter, library)}
    for label, options in (("intrinsic", {}), ("confirmed", {"delivery": True}),
                           ("conversation", {"conversation": True})):
        evidence = output / label
        evidence.mkdir()
        report[label] = case["exercise"](extensions, evidence, scratch / label, host, worker,
                                         library, **options)
    return report


def replay_released(extensions, archive, system, scratch, output, host, library):
    from maintenance_release_check import unpack_worker
    from participation_rule import run as replay_rule
    from participation_semantic import run as replay_review

    release = json.loads((extensions / "compat/participation-0.1.0/release.lock.json").read_bytes())
    asset = release["platforms"][system]
    if (release["status"] != "published-and-downloaded" or release["tag"] != "plugin/participation/v0.1.0"
            or sha(archive) != asset["sha256"]):
        raise ValueError("Wrong released participation compatibility archive")
    output.mkdir()
    packages = output / "packages"
    package = unpack_worker(archive.read_bytes(), packages, system, "0.1.0", unit_id="participation")
    if package["source"]["release_inputs_sha256"] != asset["release_inputs_sha256"]:
        raise ValueError("Released participation inputs differ from their receipt")
    worker = packages / ("yuki-participation.exe" if system == "windows" else "yuki-participation")
    if sha(worker) != asset["worker_sha256"]:
        raise ValueError("Released participation executable differs")
    rule, review = output / "native-rule", output / "native-semantic"
    rule.mkdir()
    review.mkdir()
    report = replay_review(scratch / "review", host, worker, packages / "semantic_bridge.py", library, review)
    report.update(status="PASS", rule=replay_rule(scratch / "rule", host, worker, rule),
                  plugin_archive_sha256=asset["sha256"], plugin_version="0.1.0",
                  worker_sha256=asset["worker_sha256"], host_sha256=sha(host))
    (output / "released-replay.json").write_text(
        json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8",
    )


def replay(extensions, extension_revision, host_bundle, host_sha256, core_revision, library, output,
           *, previous_participation=None):
    identity = source_identity(extensions)
    if identity["commit"] != extension_revision or identity["dirty"]:
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
        version = surface["version"]
        if version not in {"1.3.0", "1.4.0"} or "observer.event" not in surface["methods"]:
            raise ValueError("Selected participation contract required")
        if previous_participation is not None and version != "1.4.0":
            raise ValueError("The released participation regression requires the SELF candidate")
        if version == "1.4.0":
            required = {"participation.sources", "participation.self_propose", "participation.feedback"}
            if not required.issubset(surface["methods"]):
                raise ValueError("Selected SELF methods are missing")
            kit = json.loads(archive.read("sdk/support-manifest.json"))
            if kit["version"] != version or kit["source_commit"] != core_revision or kit["published"]:
                raise ValueError("Selected SDKs belong to another candidate")
            for role in ("rust_sdk", "python_sdk"):
                name = kit[role]
                if hashlib.sha256(archive.read("sdk/" + name)).hexdigest() != kit["files"][name]:
                    raise ValueError("Selected SDK bytes differ from their package")

        packages = output / "packages"
        unit = build(extensions, "participation", packages)
        worker = packages / ("yuki-participation.exe" if os.name == "nt" else "yuki-participation")
        rule = output / "native-rule"
        rule.mkdir()
        rule_result = replay_rule(scratch / "rule", host, worker, rule)
        review = output / "native-semantic"
        review.mkdir()
        review_result = replay_review(scratch / "review", host, worker, packages / "semantic_bridge.py", library, review)
        if version == "1.4.0":
            owner = output / "self"
            owner.mkdir()
            self_result = replay_self(extensions, scratch, owner, host, worker, library, library_pin["source_commit"])
            (owner / "self-tick-diff.json").write_text(
                json.dumps(self_result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8",
            )
        if previous_participation is not None:
            replay_released(extensions, previous_participation, system, scratch / "released",
                            output / "released", host, library)

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
    parser.add_argument("--previous-participation", type=Path)
    args = parser.parse_args()
    replay(args.extensions, args.extension_revision, args.host_bundle, args.host_sha256,
           args.core_revision, args.library, args.out, previous_participation=args.previous_participation)


if __name__ == "__main__":
    main()
