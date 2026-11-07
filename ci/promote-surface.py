"""Local immutable surface promotion. Public release upload is a separate authorized operation."""

import argparse
from pathlib import Path
import tempfile

from surface import (
    build_native,
    canonical,
    encoded,
    initialize,
    plan,
    promote,
    read,
    validate_plan,
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    seed = commands.add_parser("init")
    seed.add_argument("--registry", type=Path, required=True)
    seed.add_argument("--base", type=Path, required=True)
    seed.add_argument("--sha256", required=True)
    seed.add_argument("--synthetic", action="store_true")
    prepare = commands.add_parser("plan")
    prepare.add_argument("--registry", type=Path, required=True)
    prepare.add_argument("--request", type=Path, required=True)
    prepare.add_argument("--out", type=Path, required=True)
    qualify = commands.add_parser("promote")
    qualify.add_argument("--registry", type=Path, required=True)
    qualify.add_argument("--plan", type=Path, required=True)
    qualify.add_argument("--evidence", type=Path, required=True)
    sdk = commands.add_parser("build-sdks")
    sdk.add_argument("--registry", type=Path, required=True)
    sdk.add_argument("--plan", type=Path, required=True)
    sdk.add_argument("--out", type=Path, required=True)
    host = commands.add_parser("build-host")
    host.add_argument("--registry", type=Path, required=True)
    host.add_argument("--plan", type=Path, required=True)
    host.add_argument("--out", type=Path, required=True)
    host.add_argument("--target-dir", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "init":
        initialize(args.registry, args.base, args.sha256, synthetic=args.synthetic)
        return
    root = Path(__file__).resolve().parents[1]
    idl, files = canonical(root)
    if args.command == "plan":
        result = plan(args.registry, read(args.request), idl, files)
        with args.out.open("xb") as target:
            target.write(encoded(result))
    elif args.command == "build-host":
        result = build_native(
            root,
            args.registry,
            read(args.plan),
            args.out.resolve(),
            args.target_dir.resolve(),
        )
    elif args.command == "build-sdks":
        from compat import projected_kit
        from source import source_identity

        selected = read(args.plan)
        validate_plan(args.registry, selected, idl, files)
        identity = source_identity(root)
        if not read(args.registry / "registry.json")["synthetic"]:
            if identity["dirty"] or identity["commit"] != selected["candidate_source"]:
                raise ValueError("build SDKs from the exact clean candidate source")
        with tempfile.TemporaryDirectory(prefix="yuki-surface-sdk-") as temporary:
            result = projected_kit(
                args.out.resolve(),
                Path(temporary),
                selected["surface"],
                selected["version"],
                root,
                status="unpublished-selected-surface",
                description="Yuki selected candidate protocol SDK",
            )
        (args.out / "support-manifest.json").write_bytes(encoded(result))
    else:
        result = promote(args.registry, read(args.plan), args.evidence, idl, files)
    print(encoded(result).decode(), end="")


if __name__ == "__main__":
    main()
