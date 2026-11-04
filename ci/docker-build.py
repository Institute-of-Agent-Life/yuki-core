"""Build a regular-file-only context without copying host filesystem xattrs."""

import argparse
import io
import json
import subprocess
import tarfile
from pathlib import Path

from source import digest, source_identity


ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    source = source_identity(ROOT)
    tag = "yuki-core:core-005-" + source["tree_sha256"][:12]
    archive = args.out / "build-context.tar"
    explicit = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "build.rs", "LICENSE",
                "NOTICE", "deploy/core.Dockerfile", ".dockerignore", "character/SOUL.md",
                "config/qq_face_map.json"}
    paths = [Path(name) for name in source["files"]
             if name in explicit or name.startswith(("src/", "migrations/", "build/", "native/opencc/", "sdk/rust/", "protocol/plugin/gateway/"))]
    with tarfile.open(archive, "w") as bundle:
        for name in sorted(paths):
            payload = (ROOT / name).read_bytes()
            item = tarfile.TarInfo(name.as_posix())
            item.size = len(payload)
            item.mode = 0o644
            bundle.addfile(item, io.BytesIO(payload))
    with archive.open("rb") as incoming, (args.out / "build.log").open("wb") as log:
        result = subprocess.run(["docker", "build", "--file", "deploy/core.Dockerfile", "--tag", tag, "-"],
                                stdin=incoming, stdout=log, stderr=subprocess.STDOUT)
    manifest = {"source": source, "tag": tag, "context_sha256": digest(archive.read_bytes()),
                "exit_code": result.returncode, "published": False}
    if not result.returncode:
        manifest["image_id"] = subprocess.check_output(["docker", "image", "inspect", tag,
                                                        "--format", "{{.Id}}"], text=True).strip()
    (args.out / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    print(json.dumps({key: value for key, value in manifest.items() if key != "source"}))
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
