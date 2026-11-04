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
               "HOME", "USERPROFILE", "LOCALAPPDATA", "APPDATA", "LANG", "LC_ALL",
               "CARGO_HOME", "RUSTUP_HOME", "CC", "AR", "INCLUDE", "LIB", "LIBPATH", "YUKI_MINGW_BIN"}
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
        if spec.get("rust"):
            environment = clean_environment()
            if os.name == "nt" and environment.get("YUKI_MINGW_BIN"):
                bin_dir = Path(environment["YUKI_MINGW_BIN"])
                environment.update(CC=str(bin_dir / "gcc.exe"), CXX=str(bin_dir / "g++.exe"), AR=str(bin_dir / "ar.exe"),
                                   CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=str(bin_dir / "gcc.exe"),
                                   CARGO_ENCODED_RUSTFLAGS="\x1f".join(["-C", "link-self-contained=no", "-C", "dlltool=" + str(bin_dir / "dlltool.exe")]))
            rustc = subprocess.check_output(["rustc", "--version"], env=environment, text=True).strip()
            if not rustc.startswith("rustc 1.96.0 "):
                raise RuntimeError("locked Rust 1.96.0 required: " + rustc)
            manifest["tools"]["rustc"] = rustc
            manifest["tools"]["cargo"] = subprocess.check_output(
                ["cargo", "--version"], env=environment, text=True).strip()
            manifest["cargo_lock_sha256"] = digest((ROOT / "Cargo.lock").read_bytes())
            manifest["stages"] = []
            stages = [
                ("rust-format", ["cargo", "fmt", "--all", "--", "--check"]),
                ("rust-clippy", ["cargo", "clippy", "--locked", "--offline", "--all-targets", "--", "-D", "warnings"]),
                ("rust-writer-tests", ["cargo", "test", "--locked", "--offline", "--bin", "yuki-core", "--message-format=json", "--", "--nocapture"]),
                ("rust-build", ["cargo", "build", "--locked", "--offline", "--bin", "yuki-core"]),
            ]
            for name, stage in stages:
                process = subprocess.run(stage, cwd=ROOT, env=environment,
                                         capture_output=True, timeout=600)
                (out / (name + ".txt")).write_bytes(process.stdout + process.stderr)
                manifest["stages"].append({"name": name, "command": stage, "exit_code": process.returncode})
                if process.returncode:
                    raise RuntimeError(name + " failed; see " + name + ".txt")
                if name == "rust-writer-tests":
                    required = ["r01_cancel_rollback_failure_keeps_writer_until_physical_close",
                                "r01_unknown_close_stops_new_writers_while_reader_works",
                                "root_commit_hints_exclude_savepoint_release_and_rollback"]
                    output = process.stdout.decode("utf-8", errors="replace")
                    for line in output.splitlines():
                        if not line.startswith('{"reason":"compiler-artifact"'):
                            continue
                        item = json.loads(line)
                        if item.get("executable") and item["profile"]["test"] and item["target"]["name"] == "yuki-core":
                            (out / "rust-test-executable.txt").write_text(item["executable"], encoding="utf-8")
                    manifest["rust_cases"] = {case: "test store::tests::" + case + " ... ok" in output
                                              for case in required}
                    if not all(manifest["rust_cases"].values()):
                        raise RuntimeError("required Rust cleanup boundary test did not pass")
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
        if source_identity(ROOT)["tree_sha256"] != manifest["source"]["tree_sha256"]:
            raise RuntimeError("source changed during verification")
        if spec.get("rust"):
            native = json.loads((out / "sqlite-build.json").read_text(encoding="utf-8"))
            manifest["tools"]["rust_sqlite"] = native["sqlite_version"]
            manifest["sqlite_compile_options_sha256"] = digest(json.dumps(sorted(native["compile_options"])).encode())
            manifest["schema_version"] = native["schema_version"]
            manifest["sdk_version"] = None
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
