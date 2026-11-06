"""Link the native owner's exact SQLite object into a private library for the v4 benchmark process."""

import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import subprocess


def build(archive, out, cc=None):
    out.mkdir(parents=True, exist_ok=False)
    if platform.system() == "Darwin":
        name = "libsqlite3.0.dylib"
        args = [
            "-dynamiclib",
            "-install_name",
            "@rpath/" + name,
            "-compatibility_version",
            "9.0.0",
            "-current_version",
            "9.6.0",
            "-Wl,-force_load," + str(archive),
        ]
    elif os.name == "nt":
        name = "sqlite3.dll"
        args = [
            "-shared",
            "-Wl,--export-all-symbols",
            "-Wl,--whole-archive",
            str(archive),
            "-Wl,--no-whole-archive",
            "-ladvapi32",
        ]
    else:
        name = "libsqlite3.so.0"
        args = [
            "-shared",
            "-Wl,-soname," + name,
            "-Wl,--whole-archive",
            str(archive),
            "-Wl,--no-whole-archive",
            "-lpthread",
            "-ldl",
            "-lm",
        ]
    lib = out / name
    subprocess.run(
        [cc or os.environ.get("CC", "cc"), *args, "-o", str(lib)], check=True
    )
    manifest = {
        "archive": archive.name,
        "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
        "library": name,
        "sha256": hashlib.sha256(lib.read_bytes()).hexdigest(),
    }
    (out / "sqlite-build.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return lib


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("archive", type=Path)
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--cc")
    a = p.parse_args()
    print(build(a.archive.resolve(), a.out.resolve(), a.cc))
