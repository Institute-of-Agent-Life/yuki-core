"""Acquire and verify the immutable public inputs for the native consumer matrix."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import tempfile
import tomllib
from urllib.parse import urlsplit
from urllib.request import urlopen
import zipfile

from verify_dist import native_target
from verify_base import verify_base

ROOT = Path(__file__).resolve().parents[1]


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def load(root):
    lock = json.loads((root / "ci/compat-lock.json").read_bytes())
    if lock["format"] != 1 or lock["minimum_protocol_version"] != "1.0":
        raise ValueError("unsupported compatibility lock")
    return lock


def asset_path(root, asset):
    if not re.fullmatch(r"[a-f0-9]{64}", asset["sha256"]):
        raise ValueError("invalid asset digest")
    if Path(asset["name"]).name != asset["name"]:
        raise ValueError("invalid asset name")
    return root / ".cache/compat" / asset["sha256"] / asset["name"]


def verify_asset(path, asset):
    if path.stat().st_size != asset["size"] or sha(path) != asset["sha256"]:
        raise ValueError("immutable input differs: " + asset["name"])
    return path


def prepare_asset(root, asset):
    path = asset_path(root, asset)
    if path.exists():
        return verify_asset(path, asset)
    url = urlsplit(asset["url"])
    fixed_release = (
        url.hostname == "github.com"
        and url.path.startswith("/Institute-of-Agent-Life/")
        and "/releases/download/" in url.path
    )
    fixed_commit = url.hostname == "raw.githubusercontent.com" and re.fullmatch(
        r"/Institute-of-Agent-Life/yuki-extensions/[a-f0-9]{40}/artifacts/EXT-004/original-activity-17/(macos|linux|windows)\.zip",
        url.path,
    )
    if (
        url.scheme != "https"
        or url.username
        or url.password
        or url.query
        or url.fragment
        or not (fixed_release or fixed_commit)
        or not 0 < asset["size"] <= 100_000_000
    ):
        raise ValueError("input must identify a fixed public asset")
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="download-", dir=path.parent) as temporary:
        staged = Path(temporary) / asset["name"]
        with urlopen(asset["url"], timeout=45) as response, staged.open("xb") as target:
            remaining = asset["size"] + 1
            while remaining:
                block = response.read(min(1024 * 1024, remaining))
                if not block:
                    break
                target.write(block)
                remaining -= len(block)
        verify_asset(staged, asset)
        staged.replace(path)
    return path


def prepare(root=ROOT):
    lock = load(root)
    platform = "-".join(native_target())
    selected = lock["platforms"][platform]
    assets = [
        lock["signer"],
        selected["base"],
        selected["stable"],
        selected["candidate"],
    ]
    return {asset["name"]: str(prepare_asset(root, asset)) for asset in assets}


def original_assets(root, work):
    lock = load(root)
    platform = "-".join(native_target())
    selected = lock["platforms"][platform]
    key = verify_asset(asset_path(root, lock["signer"]), lock["signer"])
    base = verify_asset(asset_path(root, selected["base"]), selected["base"])
    manifest = verify_base(base, key, selected["base"]["sha256"], lock["base_commit"])
    host = work / Path(manifest["binary"]).name
    with zipfile.ZipFile(base) as archive:
        host.write_bytes(archive.read(manifest["binary"]))
    host.chmod(0o755)
    packages = {}
    for variant in ("stable", "candidate"):
        asset = selected[variant]
        archive = verify_asset(asset_path(root, asset), asset)
        destination = work / variant
        destination.mkdir()
        prefix = "evidence/native/packages/" if variant == "candidate" else ""
        with zipfile.ZipFile(archive) as bundle:
            package = json.loads(bundle.read(prefix + "package-manifest.json"))
            if package["version"] != lock[variant + "_version"]:
                raise ValueError("unexpected consumer version")
            if not package["commands"] or any(
                c["exit_code"] for c in package["commands"]
            ):
                raise ValueError("consumer has failing build checks")
            for name, expected in package["artifacts"].items():
                if Path(name).name != name:
                    raise ValueError("invalid consumer member")
                data = bundle.read(prefix + name)
                if hashlib.sha256(data).hexdigest() != expected:
                    raise ValueError("consumer member differs: " + name)
                (destination / name).write_bytes(data)
            dependencies = tomllib.loads(
                (destination / "Cargo.lock").read_text(encoding="utf-8")
            )["package"]
            sdk = [p for p in dependencies if p["name"] == "yuki-plugin-sdk"]
            if (
                len(sdk) != 1
                or sdk[0]["version"] != lock[variant + "_sdk"]
                or sdk[0]["checksum"] != lock[variant + "_sdk_sha256"]
            ):
                raise ValueError("consumer SDK differs from the original lock")
            binary = destination / (
                "yuki-telegram.exe"
                if platform.startswith("windows")
                else "yuki-telegram"
            )
            binary.chmod(0o755)
        packages[variant] = {"binary": binary, "manifest": package, "asset": asset}
    return lock, selected, host, packages


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["prepare"])
    parser.parse_args()
    print(json.dumps(prepare(), indent=2))
