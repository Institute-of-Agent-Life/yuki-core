"""Compare original effects by input and part before treating timings as comparable."""

import argparse
import gzip
import json
import math
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def golden():
    with gzip.open(
        ROOT / "fixtures/oracle/v4/arrivals.jsonl.gz", "rt", encoding="utf-8"
    ) as f:
        return [json.loads(line) for line in f]


def schedule(limit=None):
    sequence = golden()
    return sequence if limit is None else sequence[:limit]


def trial_schedule(count, warmup):
    if not 0 <= warmup <= 1200 or count is not None and not 1 <= count <= 1200:
        raise ValueError("arrival range outside original workload")
    warm = []
    for row in schedule(warmup):
        key = row["input"]
        prefix = "warm-" + key
        received = json.loads(
            json.dumps(row["gateway"]).replace("reply-" + key, "reply-" + prefix)
        )
        warm.append({**row, "input": prefix, "gateway": received})
    return warm + schedule(count)


def storage(data):
    result = {"database": 0, "wal": 0, "other": 0}
    for f in data.rglob("*"):
        if not f.is_file() or f.name.startswith("._"):
            continue
        kind = (
            "wal"
            if f.name.endswith("-wal")
            else "database"
            if f.suffix in (".sqlite", ".sqlite3", ".db")
            else "other"
        )
        result[kind] += f.stat().st_size
    return result


def effects(rows):
    return [
        {
            "input": r["input"],
            "target": r["target"],
            "reason": r["reason"],
            "gateway": r["gateway"],
            "committed_events": len(r["event_ids"]),
        }
        for r in rows
    ]


def equivalent(left, right):
    a, b = effects(left), effects(right)
    if len(a) != len(b):
        return {"equal": False, "expected": len(a), "observed": len(b)}
    for i, (x, y) in enumerate(zip(a, b)):
        if x != y:
            return {"equal": False, "at": i, "expected": x, "observed": y}
    return {"equal": True, "inputs": len(a), "parts": sum(len(r["gateway"]) for r in a)}


def quantile(data, fraction):
    values = sorted(data)
    rank = (len(values) - 1) * fraction
    i = math.floor(rank)
    return values[i] + (values[math.ceil(rank)] - values[i]) * (rank - i)


def distribution(rows):
    times = [r["seconds"] for r in rows if r.get("seconds") is not None]
    if not times:
        return {"n": 0}
    return {
        "n": len(times),
        "p50_seconds": quantile(times, 0.5),
        "p95_seconds": quantile(times, 0.95),
        "mean_seconds": statistics.mean(times),
    }


def decide(arms, controls, *, runtime_controls_equal=False):
    failures = {name: equivalent(controls, rows) for name, rows in arms.items()}
    return {
        "equivalence": failures,
        "distributions": {name: distribution(rows) for name, rows in arms.items()},
        "external_effects_equal": all(v["equal"] for v in failures.values()),
        "timing_comparison_allowed": set(arms) == {"A", "B", "C"}
        and runtime_controls_equal
        and all(v["equal"] for v in failures.values()),
    }


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("arrivals", type=Path)
    args = p.parse_args()
    rows = [json.loads(s) for s in args.arrivals.read_text().splitlines()]
    print(json.dumps(decide({"C": rows}, schedule(len(rows))), ensure_ascii=False))
