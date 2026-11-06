"""Exercise the original flat Vec1 backend in isolated databases for each DV scale."""

import argparse
import hashlib
import json
import math
from pathlib import Path
import random
import resource
import sqlite3
import struct
import subprocess
import sys
import time

COMMIT = "329ca7c487414d2f54ddd98ff4727446ed2446cd"
D = 1024
K = 20
PACK = struct.Struct("<1024f")


def seed(i):
    # Every vector is reproducible from its fact number and generation.
    g = random.Random(i)
    return PACK.pack(*(int(x) - 127 for x in g.randbytes(D)))


def run(out, sizes):
    import yuki.persistence.vec1 as v4

    root = Path(v4.__file__).resolve().parents[3]
    assert (
        subprocess.check_output(
            ["git", "-C", str(root), "rev-parse", "HEAD"], text=True
        ).strip()
        == COMMIT
    )
    ext = v4.extension_path()
    report = {
        "v4_commit": COMMIT,
        "extension_sha256": hashlib.sha256(ext.read_bytes()).hexdigest(),
        "sqlite": sqlite3.sqlite_version,
        "python": sys.version,
        "dimension": D,
        "K": K,
        "rows": [],
    }
    query = seed(31017)
    for size in sizes:
        path = out / f"{size}.sqlite"
        assert not path.exists()
        db = sqlite3.connect(path)
        db.enable_load_extension(True)
        db.load_extension(str(ext))
        db.enable_load_extension(False)
        db.executescript(
            "PRAGMA journal_mode=WAL; CREATE TABLE facts(id INTEGER PRIMARY KEY, bucket INTEGER,occurred INTEGER,active INTEGER);CREATE INDEX fact_scope_date ON facts(bucket,occurred,active,id); CREATE VIRTUAL TABLE vecs USING vec1(vector,fact_id);"
        )
        db.execute(
            "INSERT INTO vecs(cmd,arg) VALUES('rebuild',?)",
            (json.dumps({"index": "flat", "distance": "cos"}),),
        )
        begin = time.perf_counter()
        with db:
            for i in range(1, size + 1):
                # One seeded hotspot, stable sparse percentage scopes, three date decades.
                db.execute(
                    "INSERT INTO facts VALUES(?,?,?,1)",
                    (i, (i - 1) % 100, (i // 7) % 30),
                )
                db.execute(
                    "INSERT INTO vecs(rowid,fact_id,vector) VALUES(?,?,?)",
                    (i, i, seed(i)),
                )
        ingest = time.perf_counter() - begin
        with db:
            db.execute("UPDATE facts SET active=0 WHERE id%71=0")
            db.execute(
                "DELETE FROM vecs WHERE rowid IN(SELECT id FROM facts WHERE active=0)"
            )
            for i in range(113, size + 1, 113):
                if i % 71:
                    db.execute(
                        "UPDATE vecs SET vector=? WHERE rowid=?", (seed(i + size), i)
                    )
        db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        for coverage in (1, 10, 100):
            for low, high in ((0, 29), (7, 13)):
                sql = "SELECT rowid,fact_id FROM vecs(?,?) WHERE fact_id IN(SELECT id FROM facts WHERE bucket<? AND active=1 AND occurred BETWEEN ? AND ?)"
                args = (query, json.dumps({"K": K}), coverage, low, high)
                plan = [row[3] for row in db.execute("EXPLAIN QUERY PLAN " + sql, args)]
                times = []
                answers = []
                for _ in range(4):
                    at = time.perf_counter_ns()
                    ans = [r[1] for r in db.execute(sql, args)]
                    times.append((time.perf_counter_ns() - at) / 1e6)
                    answers.append(ans)
                assert answers[0] and len(set(map(tuple, answers))) == 1
                assert all(
                    db.execute(
                        "SELECT bucket<? AND occurred BETWEEN ? AND ? AND active=1 FROM facts WHERE id=?",
                        (coverage, low, high, i),
                    ).fetchone()[0]
                    for i in ans
                )
                row = {
                    "N": size,
                    "scope_percent": coverage,
                    "date": [low, high],
                    "build_seconds": ingest,
                    "disk_bytes": path.stat().st_size,
                    "authorized": db.execute(
                        "SELECT count(*) FROM facts WHERE bucket<? AND active=1 AND occurred BETWEEN ? AND ?",
                        (coverage, low, high),
                    ).fetchone()[0],
                    "actual_top": ans,
                    "timings_ms": times,
                    "query_plan": plan,
                    "child_peak_rss": resource.getrusage(
                        resource.RUSAGE_SELF
                    ).ru_maxrss,
                }
                report["rows"].append(row)
        db.close()
        (out / "vec1-observations.json").write_text(json.dumps(report, indent=2) + "\n")
        print(size, flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--sizes", nargs="+", type=int, default=[1500, 10000, 100000])
    args = parser.parse_args()
    args.out.mkdir(exist_ok=False, parents=True)
    run(args.out, args.sizes)
