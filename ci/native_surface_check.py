"""After the ordinary suite exits, build and replay one synthetic native surface."""

import argparse
from pathlib import Path
import subprocess
import sys

from source import source_identity
from surface import build_native, canonical, encoded, initialize, plan, read, sha


def run(root, output, target):
    output.mkdir(parents=True, exist_ok=False)
    source = source_identity(root)
    base = read(root / "protocol/plugin/stable/1.0/profile.json")
    base["consumers"] = ["telegram", "web-fetch"]
    base_path = output / "base.json"
    base_path.write_bytes(encoded(base))
    digest = sha(base_path.read_bytes())
    registry = output / "registry"
    initialize(registry, base_path, digest, synthetic=True)
    idl, files = canonical(root)
    selected = plan(
        registry,
        {
            "candidate_source": source["commit"],
            "parent_stable_digest": digest,
            "consumers": ["native-profile-control"],
            "selection": {"gateway.activity": {"new_method": True}},
        },
        idl,
        files,
    )
    plan_path = output / "plan.json"
    plan_path.write_bytes(encoded(selected))
    kit = output / "sdk"
    with (output / "sdk-build.log").open("wb") as log:
        subprocess.run(
            [
                sys.executable,
                str(root / "ci/promote-surface.py"),
                "build-sdks",
                "--registry",
                str(registry),
                "--plan",
                str(plan_path),
                "--out",
                str(kit),
            ],
            cwd=root,
            stdout=log,
            stderr=subprocess.STDOUT,
            check=True,
        )
    native = output / "native"
    build = build_native(root, registry, selected, native, target)
    with (output / "admission.log").open("wb") as log:
        subprocess.run(
            [
                sys.executable,
                str(root / "tests/peers/base/profile_check.py"),
                "--host",
                str(native / build["binary"]["path"]),
                "--kit",
                str(kit),
                "--plan",
                str(plan_path),
                "--out",
                str(output / "admission"),
            ],
            cwd=root,
            stdout=log,
            stderr=subprocess.STDOUT,
            check=True,
        )
    admission = read(output / "admission/native-profile.json")
    if (
        source_identity(root) != source
        or admission["binary_sha256"] != build["binary"]["sha256"]
    ):
        raise ValueError("qualification source or native binary changed")
    receipt = {
        "status": "PASS",
        "source": source,
        "synthetic": True,
        "real_capabilities_promoted": [],
        "published": False,
        "native_binary_sha256": build["binary"]["sha256"],
        "storage": build["storage"],
        "assertions": admission["assertions"],
        "files": {
            p.relative_to(output).as_posix(): sha(p.read_bytes())
            for p in sorted(output.rglob("*"))
            if p.is_file()
            and not any(part.startswith("._") for part in p.relative_to(output).parts)
        },
    }
    (output / "validation.json").write_bytes(encoded(receipt))
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    run(root, args.out.resolve(), args.target_dir.resolve())


if __name__ == "__main__":
    main()
