"""Build, verify or locally pin the handshake preview. No registry or release side effects."""

import argparse
import json
from pathlib import Path
import tempfile
from contract import build, verify, pin


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    builder = sub.add_parser("build")
    builder.add_argument("--out", type=Path, required=True)
    builder.add_argument("--verify-work", type=Path)
    for name in ("verify", "pin"):
        cmd = sub.add_parser(name)
        cmd.add_argument("directory", type=Path)
        if name == "pin":
            cmd.add_argument("--lock", type=Path, required=True)
            cmd.add_argument("--expected-sha256", required=True)
            cmd.add_argument("--allow-working-tree", action="store_true")
    args = parser.parse_args()
    if args.command == "build":
        out = args.out.resolve()
        with tempfile.TemporaryDirectory(prefix="yuki-sdk-") as scratch:
            build(
                out,
                args.verify_work.resolve()
                if args.verify_work
                else Path(scratch) / "install",
            )
        args.directory = out
    if args.command == "pin":
        pin(
            args.directory,
            args.lock,
            args.expected_sha256,
            allow_working_tree=args.allow_working_tree,
        )
    doc = verify(args.directory)
    print(
        json.dumps(
            {
                k: doc[k]
                for k in (
                    "status",
                    "source_commit",
                    "archive_sha256",
                    "sdk_versions",
                    "schema_sha256",
                )
            }
        )
    )


if __name__ == "__main__":
    main()
