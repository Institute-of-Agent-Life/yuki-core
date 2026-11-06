"""Time real Rust scoped retrieval on the same rows as vec1_matrix.py. Run in a new directory."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from contextlib import closing

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tests.harness.compact import Owner
from tests.peers.chat.recorder import Recorder
from vec1_matrix import D, K, PACK, seed


class Evidence:
    root = ROOT

    def __init__(self, folder):
        self.working = folder
        folder.mkdir()

    def equal(self, name, actual, want):
        if actual != want:
            raise ValueError((name, actual, want))

    def check(self, name, ok):
        if not ok:
            raise ValueError(name)

    def write(self, name, value):
        (self.working / name).write_text(json.dumps(value, indent=2) + "\n")


def stage(o, persons):
    sources = []
    with o.svc.running():
        for i, p in enumerate(persons):
            o.op("person", "create", p)
            account = o.op(
                "account",
                "open",
                "qq",
                "compact",
                str(300 + i),
                1,
                "--person",
                p,
                "--start",
                1,
            )
            frame = {
                "source": f"dv-reader-{i}",
                "account": account["account"],
                "bot_stage": o.bot["assignment"],
                "occurred": 100,
                "observed": 100,
                "kind": "message",
                "segments": [{"type": "text", "text": "measure"}],
            }
            ev = o.op("ingress", "record", json.dumps(frame), "--accept")["event"]
            ex = o.op("ingress", "claim", ev)["execution"]
            o.op("model", "root", ex, 64, 10_000_000)
            sources.append(ex)
    return sources


def populate(o, db, size, persons, execs, profile):
    people = [
        str(uuid.UUID(int=(0x10000000 << 96) + i + 1, version=4)) for i in range(100)
    ]
    db.executemany("INSERT INTO persons(id) VALUES(?)", [(p,) for p in people])
    for reader, percent in zip(persons, (1, 10, 100)):
        entry = f"DV-{percent}"
        db.execute(
            "INSERT INTO entries(id,scope,space) VALUES(?,'group',?)", (entry, entry)
        )
        for p in [reader, *people[:percent]]:
            db.execute(
                "INSERT INTO events(source_scope,source_key,entry_id,account_id,raw_actor,bot_stage,observed_at,envelope,mentions) VALUES('dv',?,?,?,?,?,100,?, '[]')",
                (
                    entry + p,
                    entry,
                    o.user["account"],
                    p,
                    o.bot["assignment"],
                    '{"kind":"message","segments":[]}',
                ),
            )
    t = time.perf_counter()
    for i in range(1, size + 1):
        fact = f"ordinal-{i}"
        text = "benchmark"
        cat = "fixture"
        key = "setting"
        date = f"2024-01-{(i//7)%30+1:02}T00:00:00Z"
        person = people[(i - 1) % 100]
        db.execute(
            "INSERT INTO memory_facts(id,kind,person,visibility,fact_key,created_execution,status) VALUES(?,'person',?,'subject',?,?,'active')",
            (fact, person, key, execs[0]),
        )
        db.execute(
            "INSERT INTO memory_versions(fact,revision,body,confidence,execution) VALUES(?,1,?,0.8,?)",
            (fact, text, execs[0]),
        )
        db.execute(
            "INSERT INTO extract_fact_details(fact,category,kind,importance,valid_from) VALUES(?,?,'fact',3,?)",
            (fact, cat, date),
        )
        if i % 71 == 0:
            db.execute("UPDATE memory_facts SET status='retracted' WHERE id=?", (fact,))
            continue
        if i % 113 == 0:
            text = "revision two"
            revision = 2
            db.execute("UPDATE memory_facts SET revision=2 WHERE id=?", (fact,))
            db.execute(
                "INSERT INTO memory_versions(fact,revision,body,confidence,execution) VALUES(?,2,?,0.8,?)",
                (fact, text, execs[0]),
            )
        else:
            revision = 1
        h = hashlib.sha256(
            f"Kind: fact\nCategory: {cat}\nKey: {key}\nFact: {text}".encode()
        ).hexdigest()
        db.execute(
            "INSERT INTO fact_vectors VALUES(?,?,'document',?,?,?,?)",
            (fact, profile, D, h, revision, seed(i + size if revision == 2 else i)),
        )
    db.commit()
    db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    return time.perf_counter() - t


def measure(out, original, sizes, binary, backends):
    rows = []
    selected_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    (out / "build.json").write_text(
        json.dumps(
            {
                "binary_sha256": selected_hash,
                "binary": str(binary),
                "rustc": subprocess.check_output(["rustc", "-vV"], text=True),
                "source_commit": subprocess.check_output(
                    ["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True
                ).strip(),
                "source_dirty": bool(
                    subprocess.check_output(
                        ["git", "-C", str(ROOT), "status", "--porcelain"]
                    )
                ),
            },
            indent=2,
        )
        + "\n"
    )
    source = json.loads(original.read_bytes())
    data_root = Path(
        tempfile.mkdtemp(prefix="yc-dv-", dir="/tmp" if os.name == "posix" else None)
    )
    (out / "data-root.txt").write_text(str(data_root) + "\n")
    for size in sizes:
        o = Owner(Evidence(data_root / f"n-{size}"), "core", 22)
        o.svc.binary = binary
        o.svc.argv = [str(binary), "--data", str(o.svc.data)]
        people = [str(uuid.uuid4()) for _ in range(3)]
        execs = stage(o, people)
        with Recorder("/v1/embeddings") as peer:
            (o.svc.data / "yuki.toml").write_text(
                f'format_version=1\nlisten="127.0.0.1:0"\n[memory]\nmode="hybrid"\n[memory.embedding]\nprovider="openai_compatible"\nmodel="bench"\nbase_url="{peer.url.removesuffix("/embeddings")}"\ndimensions={D}\n'
            )
            with o.svc.running():
                profile = o.op("memory", "vector-status")["profile"]
            with closing(sqlite3.connect(o.svc.data / "core.sqlite")) as db:
                db.execute("PRAGMA foreign_keys=ON")
                elapsed = populate(o, db, size, people, execs, profile)
            peak = [0]
            with o.svc.running(), ThreadPoolExecutor(max_workers=1) as pool:
                owner_pid = int(
                    subprocess.check_output(
                        ["pgrep", "-nf", str(o.svc.data) + " run"], text=True
                    ).strip()
                )
                halt = threading.Event()

                def rss():
                    while not halt.is_set():
                        try:
                            peak[0] = max(
                                peak[0],
                                int(
                                    subprocess.check_output(
                                        ["ps", "-o", "rss=", "-p", str(owner_pid)],
                                        text=True,
                                    ).strip()
                                )
                                * 1024,
                            )
                        except (ValueError, subprocess.CalledProcessError):
                            pass
                        halt.wait(0.05)

                monitor = threading.Thread(target=rss, daemon=True)
                monitor.start()
                try:
                    prepare_ms = None
                    if "vec1" in backends:
                        started = time.perf_counter_ns()
                        for _ in range(size // 512 + 3):
                            if o.op("memory", "prepare-reference", execs[0], people[0])[
                                "ready"
                            ]:
                                break
                        else:
                            raise AssertionError("reference index failed to finish")
                        prepare_ms = (time.perf_counter_ns() - started) / 1e6
                    for expected in [r for r in source["rows"] if r["N"] == size]:
                        for backend in (
                            backends
                            if len(rows) // max(1, len(backends)) % 2 == 0
                            else list(reversed(backends))
                        ):
                            percent = expected["scope_percent"]
                            i = [1, 10, 100].index(percent)
                            low, high = expected["date"]
                            timings = []
                            physical = 0
                            for repeat in range(4):
                                cmd = [
                                    *o.svc.argv,
                                    "operator",
                                    "memory",
                                    "retrieve",
                                    execs[i],
                                    people[i],
                                    f"bench-{backend}-{percent}-{low}-{repeat}",
                                    "zzbenchmark-vector",
                                    "--limit",
                                    str(K),
                                    "--candidates",
                                    str(K),
                                    "--backend",
                                    backend,
                                    "--start",
                                    f"2024-01-{low+1:02}T00:00:00Z",
                                    "--end",
                                    f"2024-01-{high+2:02}T00:00:00Z",
                                ]
                                at = time.perf_counter_ns()
                                pending = pool.submit(
                                    subprocess.run,
                                    cmd,
                                    capture_output=True,
                                    text=True,
                                    timeout=240,
                                )
                                while not pending.done():
                                    try:
                                        path, raw, reply = peer.wires.get(timeout=0.02)
                                    except __import__("queue").Empty:
                                        continue
                                    body = json.loads(raw)
                                    assert body["input"] == ["zzbenchmark-vector"]
                                    reply.put(
                                        (
                                            {
                                                "data": [
                                                    {
                                                        "index": 0,
                                                        "embedding": list(
                                                            PACK.unpack(seed(31017))
                                                        ),
                                                    }
                                                ],
                                                "usage": {"prompt_tokens": 2},
                                            },
                                            0,
                                        )
                                    )
                                    physical += 1
                                result = pending.result()
                                if result.returncode:
                                    raise RuntimeError(
                                        f"{backend} {size} {percent} {low}: {result.stderr}"
                                    )
                                view = json.loads(result.stdout)
                                timings.append((time.perf_counter_ns() - at) / 1e6)
                            actual = [x["fact_id"] for x in view["items"]]
                            assert (
                                view["authorized"] == expected["authorized"]
                                and not view["partial_failure"]
                                and (not view["approximate"] or backend == "vec1")
                            )
                            recall = len(
                                set(actual) & set(expected["actual_top"])
                            ) / len(expected["actual_top"])
                            plan = subprocess.run(
                                [*cmd, "--explain"],
                                capture_output=True,
                                text=True,
                                timeout=120,
                                check=True,
                            )
                            plan = json.loads(plan.stdout)["semantic_eqp"]
                            assert plan
                            rows.append(
                                {
                                    "N": size,
                                    "backend": backend,
                                    "scope_percent": percent,
                                    "date": [low, high],
                                    "authorized": view["authorized"],
                                    "covered": view["covered"],
                                    "approximate": view["approximate"],
                                    "actual_top": actual,
                                    "original_top": expected["actual_top"],
                                    "exact_order_matches": actual
                                    == expected["actual_top"],
                                    "recall_at_k": recall,
                                    "wall_ms": timings,
                                    "query_http_calls": physical,
                                    "query_plan": plan,
                                    "disk_bytes": sum(
                                        (o.svc.data / name).stat().st_size
                                        for name in ("core.sqlite", "core.sqlite-wal")
                                        if (o.svc.data / name).is_file()
                                    ),
                                    "build_seconds": elapsed,
                                    "reference_prepare_ms": prepare_ms,
                                    "sampled_owner_rss_bytes": peak[0],
                                }
                            )
                            (out / "core-observations.json").write_text(
                                json.dumps(rows, indent=2) + "\n"
                            )
                            print(size, backend, percent, low, recall, flush=True)
                finally:
                    halt.set()
                    monitor.join()
    assert hashlib.sha256(binary.read_bytes()).hexdigest() == selected_hash
    return rows


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--vec1-report", type=Path, required=True)
    parser.add_argument("--sizes", type=int, nargs="+", default=[1500, 10000, 100000])
    parser.add_argument(
        "--binary", type=Path, default=ROOT / "target/release/yuki-core"
    )
    parser.add_argument(
        "--backends", nargs="+", choices=["vec1", "exact"], default=["vec1", "exact"]
    )
    args = parser.parse_args()
    args.out.mkdir(exist_ok=False, parents=True)
    measure(
        args.out, args.vec1_report, args.sizes, args.binary.resolve(), args.backends
    )
