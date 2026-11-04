"""Source identity shared by checkout and verified Windows snapshots."""

import hashlib
import json
import subprocess
from pathlib import Path


def digest(data):
    return hashlib.sha256(data).hexdigest()


def git(root, *args):
    result = subprocess.run(["git", "-C", str(root), *args], capture_output=True)
    return result.stdout.decode("utf-8").strip() if result.returncode == 0 else None


def source_identity(root):
    root = Path(root)
    snapshot = root / ".yuki-source.json"
    if snapshot.exists():
        source = json.loads(snapshot.read_text(encoding="utf-8"))
        for name, expected in source["files"].items():
            path = root / name
            if not path.resolve().is_relative_to(root.resolve()):
                raise ValueError("source path escapes snapshot")
            if digest(path.read_bytes()) != expected:
                raise ValueError("source hash mismatch: " + name)
        encoded = json.dumps(source["files"], sort_keys=True).encode()
        if digest(encoded) != source["tree_sha256"]:
            raise ValueError("source tree digest mismatch")
        return source
    result = subprocess.run(
        ["git", "-C", str(root), "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        capture_output=True, check=True,
    )
    names = sorted(set(result.stdout.decode("utf-8").split("\0")) - {""})
    files = {}
    for name in names:
        path = root / name
        if not path.is_file() or path.is_symlink():
            raise ValueError("source must be a regular file: " + name)
        files[name] = digest(path.read_bytes())
    return {
        "commit": git(root, "rev-parse", "--verify", "HEAD"),
        "branch": git(root, "symbolic-ref", "--short", "HEAD"),
        "dirty": bool(git(root, "status", "--porcelain")),
        "tree_sha256": digest(json.dumps(files, sort_keys=True).encode()),
        "files": files,
    }
