"""Run A or a byte-pinned B patch in an isolated v4 worktree with the same SQLite as C."""

import argparse
import os
import sys
import hashlib
import gzip
import json
from pathlib import Path
import subprocess
import time
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BASE = "329ca7c487414d2f54ddd98ff4727446ed2446cd"


def execute(a):
    source = a.checkout.resolve()
    out = a.out.resolve()

    def git(*args):
        return subprocess.check_output(["git", "-C", str(source), *args])

    if git("rev-parse", "HEAD").decode().strip() != BASE:
        raise ValueError("v4 source SHA mismatch")
    patch = git("diff", "HEAD", "--")
    expected = (
        gzip.decompress((ROOT / "bench/controls/b.patch.gz").read_bytes())
        if a.arm == "B"
        else b""
    )
    if patch != expected:
        raise ValueError("working tree must match the declared arm byte for byte")
    out.mkdir(parents=True, exist_ok=False)
    os.chdir(source)
    if os.name == "nt":
        import ctypes

        sqlite_dll = ctypes.WinDLL(str(a.library.resolve()))
        if not sqlite_dll._handle:
            raise RuntimeError("private SQLite DLL failed to load")
    import sqlite3

    if sqlite3.sqlite_version != "3.46.0":
        raise RuntimeError(f"SQLite mismatch: {sqlite3.sqlite_version}")
    sys.path[:0] = [
        str(source),
        str(source / "src"),
        str(source / "packages/yuki-agent-connector/src"),
        str(ROOT),
    ]
    from yuki.persistence.database import Database

    orig = Database._configure_sqlite_connection
    settings = []

    def controlled(db, record):
        orig(db, record)
        cur = db.cursor()
        cur.execute("PRAGMA synchronous=FULL")
        cur.execute("PRAGMA busy_timeout=5000")
        observed = {}
        for key, sql in [
            ("version", "SELECT sqlite_version()"),
            ("journal", "PRAGMA journal_mode"),
            ("synchronous", "PRAGMA synchronous"),
            ("foreign_keys", "PRAGMA foreign_keys"),
            ("busy_timeout_ms", "PRAGMA busy_timeout"),
        ]:
            cur.execute(sql)
            observed[key] = cur.fetchone()[0]
        cur.execute("PRAGMA compile_options")
        compile_options = sorted(v[0] for v in cur.fetchall())
        cur.close()
        if observed != {
            "version": "3.46.0",
            "journal": "wal",
            "synchronous": 2,
            "foreign_keys": 1,
            "busy_timeout_ms": 5000,
        }:
            raise RuntimeError("mismatched persistence: " + str(observed))
        observed["compile_options_sha256"] = hashlib.sha256(
            json.dumps(compile_options).encode()
        ).hexdigest()
        settings.append(observed)

    Database._configure_sqlite_connection = staticmethod(controlled)
    os.environ.update(
        YUKI_SLICE_OUT=str(out),
        YUKI_SLICE_COUNT=str(a.count),
        YUKI_SLICE_WARMUP=str(a.warmup),
        YUKI_LONG_WORKS=str(a.works),
        PYTHONDONTWRITEBYTECODE="1",
    )
    if not a.works and not a.compact and not a.automatic_compact:
        os.environ.update(
            YUKI_SLICE_FRAMES=str(a.frames.resolve(strict=True)),
            YUKI_SLICE_FRAMES_SHA256=a.frames_sha256,
        )
    from baseline.replay.record import offline

    offline()
    import pytest

    case = "legacy_case.py"
    if a.automatic_compact:
        case = "automatic_compact_case.py"
    elif a.compact:
        case = "compact_case.py"
    elif a.works:
        case = "long_work_case.py"
    begin = time.perf_counter()
    cpu = time.process_time()
    with tempfile.TemporaryDirectory(
        prefix="yuki-v4-slice-", dir=a.temp_root
    ) as working:
        data_device = os.stat(working).st_dev
        code = pytest.main(
            [
                "-q",
                "-p",
                "no:cacheprovider",
                *([] if a.works else ["--maxfail=1"]),
                "-c",
                str(source / "pyproject.toml"),
                str(ROOT / "bench/controls" / case),
                "--basetemp",
                str(Path(working) / "pytest"),
                "--junitxml",
                str(out / "junit.xml"),
            ]
        )
    elapsed = time.perf_counter() - begin
    meta = {
        "arm": a.arm,
        "source_commit": BASE,
        "patch_sha256": hashlib.sha256(expected).hexdigest(),
        "exit_code": int(code),
        "elapsed_seconds": elapsed,
        "driver_cpu_seconds": time.process_time() - cpu,
        "sqlite": settings[-1] if settings else None,
        "sqlite_library_sha256": hashlib.sha256(a.library.read_bytes()).hexdigest(),
        "count": 2 if a.automatic_compact else 15 if a.compact else a.count,
        "warmup": a.warmup,
        "long_works": a.works,
        "public_compact": a.compact,
        "automatic_compact": a.automatic_compact,
        "data_device": data_device,
        "paid_calls": 0,
    }
    (out / "run.json").write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")
    return int(code)


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("arm", choices=["A", "B"])
    p.add_argument("--checkout", required=True, type=Path)
    p.add_argument("--out", required=True, type=Path)
    p.add_argument("--library", required=True, type=Path)
    p.add_argument(
        "--temp-root", type=Path, help="native data volume shared by the arms"
    )
    p.add_argument("--count", type=int, default=1200)
    p.add_argument("--warmup", type=int, default=0)
    scenarios = p.add_mutually_exclusive_group()
    scenarios.add_argument("--automatic-compact", action="store_true", help="replay v4 automatic compaction after send and public-planner refusal")
    scenarios.add_argument("--compact",action="store_true",help="compare two checkpoints in one conversation")
    scenarios.add_argument("--works", type=int, choices=range(0, 21), default=0)
    p.add_argument("--frames", type=Path)
    p.add_argument("--frames-sha256")
    args = p.parse_args()
    if not args.automatic_compact and not args.compact and not args.works and not (args.frames and args.frames_sha256):
        p.error("QQ arrivals require --frames and --frames-sha256")
    sys.exit(execute(args))
