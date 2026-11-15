"""Recover the released 1.2 registry and plan the participation candidate."""

import argparse
import hashlib
from pathlib import Path
import shutil
import sys
from urllib.request import urlopen

RELEASES = {
    "1.1.0": (
        "28e37974c394cc6ba9caca61175c8c41f7d8dc56f674970d10bab7d948cbcc86",
        "90933f039d2aaa19231f03b7ea12dabd0ebfb09cac282d05e8385a8d33738845",
    ),
    "1.2.0": (
        "3c0e3a9444a3bdeb14c3a2cba8a94870544dcbe40d9f0a1d0cc4a76e1dea35f3",
        "c7d4790b19827e9cbd9a2629fa605c417b42700255060cb7eeeb71f24b8df145",
    ),
}
BASE_SHA256 = "f99094570de061970e3a21112007d51ed98df042f35060617df104ab05f6c496"
METHODS = (
    "observer.event",
    "participation.source",
    "participation.promote",
    "participation.feedback",
    "llm.complete",
    "llm.result",
)


def download(version, name, path, expected):
    url = (
        "https://github.com/Institute-of-Agent-Life/yuki-core/releases/download/"
        f"plugin-base-v{version}/{name}"
    )
    with urlopen(url, timeout=90) as response, path.open("xb") as output:
        shutil.copyfileobj(response, output)
    with path.open("rb") as stream:
        if hashlib.file_digest(stream, "sha256").hexdigest() != expected:
            raise ValueError(f"Published asset differs from its pin: {version}/{name}")


def prepare(source, registry, output, revision):
    sys.path.insert(0, str(source / "ci"))
    from source import source_identity
    from surface import canonical, encoded, initialize, plan, read, require

    identity = source_identity(source)
    require(identity["commit"] == revision and not identity["dirty"], "wrong producer checkout")
    output.mkdir(parents=True)
    base = read(source / "protocol/plugin/stable/1.0/profile.json")
    base.update(
        consumers=["telegram", "web-fetch"],
        published=True,
        original_release_sha256="a98fb1e4d36f895f02db8614b88638ed73339f21d03bd42c76719ab54274dabc",
        original_publication_sha256="2bd0b94fd7350c0e49617d5b8755c05401308f8b9639d5f9476678583a4d0aeb",
        original_source="c6bd0c75e38717abc71373201d31a77982428d5b",
    )
    base_path = output / "base.json"
    base_path.write_bytes(encoded(base))
    initialize(registry, base_path, BASE_SHA256)
    for version, (manifest_sha256, package_sha256) in RELEASES.items():
        directory = registry / "versions" / version
        directory.mkdir()
        download(version, "manifest.json", directory / "manifest.json", manifest_sha256)
        download(version, f"yuki-plugin-base-{version}.zip", directory / "package.zip", package_sha256)

    request = {
        "candidate_source": revision,
        "parent_stable_digest": RELEASES["1.2.0"][0],
        "consumers": ["participation"],
        "selection": {method: {"new_method": True} for method in METHODS},
    }
    planned = plan(registry, request, *canonical(source))
    require(planned["version"] == "1.3.0" and not planned["rebased"] and not planned["published"],
            "candidate must extend the pinned released registry")
    (output / "request.json").write_bytes(encoded(request))
    (output / "plan.json").write_bytes(encoded(planned))
    print(planned["candidate_source"], planned["surface_sha256"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    prepare(args.source.resolve(), args.registry.resolve(), args.out.resolve(), args.revision)


if __name__ == "__main__":
    main()
