"""python ci/check.py CORE-NNN --out a/new/attempt-directory"""

import argparse
import json
import os
import platform
import sqlite3
import subprocess
import sys
import time
from pathlib import Path

from source import digest, source_identity


ROOT = Path(__file__).resolve().parents[1]


def clean_environment():
    allowed = {"PATH", "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT", "TEMP", "TMP",
               "HOME", "USERPROFILE", "LOCALAPPDATA", "APPDATA", "LANG", "LC_ALL"}
    result = {key: value for key, value in os.environ.items() if key.upper() in allowed}
    result.update(PYTHONUTF8="1", PYTHONDONTWRITEBYTECODE="1", CARGO_NET_OFFLINE="true",
                  UV_OFFLINE="1", NO_PROXY="127.0.0.1,::1,localhost")
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("card")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--fault", choices=["swap", "empty"])
    args = parser.parse_args()
    out = args.out.resolve()
    try:
        out.mkdir(parents=True, exist_ok=False)
    except FileExistsError:
        parser.exit(2, "output exists; choose a new attempt directory\n")
    command = [sys.executable, str(ROOT / "tests/harness/worker.py"), args.card, str(out)]
    if args.fault:
        command.extend(["--fault", args.fault])
    manifest = {
        "format_version": 1, "card": args.card, "status": "failed", "exit_code": 1,
        "command": ["python", "ci/check.py", args.card, "--out", str(args.out)],
        "worker_command": command, "platform": platform.platform(),
        "tools": {"python": platform.python_version(), "sqlite": sqlite3.sqlite_version},
        "dependency_lock": "ci/checks.json", "network": "python-loopback-only",
        "cost": {"physical_provider_calls": 0, "paid_calls": 0, "source": "offline harness"},
        "failure": None, "assertions": [], "artifacts": {},
    }
    started = time.monotonic()
    try:
        lock_bytes = (ROOT / "ci/checks.json").read_bytes()
        lock = json.loads(lock_bytes)
        manifest.update(lock_sha256=digest(lock_bytes), seed=lock["seed"], clock=lock["clock"])
        manifest["source"] = source_identity(ROOT)
        if ".".join(map(str, sys.version_info[:2])) != lock["python"]:
            raise RuntimeError("locked Python " + lock["python"] + " required")
        spec = lock["cards"][args.card]
        manifest["capabilities"] = spec["capabilities"]
        try:
            process = subprocess.run(command, cwd=ROOT, env=clean_environment(),
                                     capture_output=True, timeout=spec["timeout_seconds"])
            manifest["exit_code"] = process.returncode
            (out / "stdout.txt").write_bytes(process.stdout)
            (out / "stderr.txt").write_bytes(process.stderr)
        except subprocess.TimeoutExpired as error:
            (out / "stdout.txt").write_bytes(error.stdout or b"")
            (out / "stderr.txt").write_bytes(error.stderr or b"")
            raise RuntimeError("case timeout") from error
        result_path = out / "case-result.json"
        if not result_path.exists():
            raise RuntimeError("worker did not produce a case result")
        result = json.loads(result_path.read_text(encoding="utf-8"))
        manifest["assertions"] = result["assertions"]
        if process.returncode or result["failure"]:
            raise RuntimeError(result["failure"] or "worker failed")
        if not result["assertions"]:
            raise RuntimeError("empty case")
        if not (out / spec["artifact"]).is_file():
            raise RuntimeError("missing named artifact: " + spec["artifact"])
        manifest.update(status="verified", exit_code=0)
    except Exception as error:
        manifest.update(status="failed", exit_code=1, failure=str(error))
    finally:
        manifest["duration_seconds"] = round(time.monotonic() - started, 6)
        for path in sorted(out.rglob("*")):
            if path.is_file() and path != out / "manifest.json":
                manifest["artifacts"][path.relative_to(out).as_posix()] = digest(path.read_bytes())
        (out / "manifest.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False)
                                           + "\n", encoding="utf-8")
    print(json.dumps({"card": args.card, "status": manifest["status"], "out": str(out),
                      "failure": manifest["failure"]}, ensure_ascii=False))
    return manifest["exit_code"]


if __name__ == "__main__":
    sys.exit(main())
