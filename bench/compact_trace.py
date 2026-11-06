"""Shared public-turn schedule. Markers stand for distinct original sources, not model instructions."""


def turns():
    for generation in (1, 2):
        for index in range(6):
            yield (
                "normal",
                f"PUBLIC_{generation}_{index}: "
                + "observation from this conversation. " * 12,
            )
        yield "public_compact", f"/ai compact preserve public phase {generation}"
    yield "normal", "CURRENT_TAIL_AFTER_TWO_CHECKPOINTS"


def summary(n):
    return (
        f"BENCH_PUBLIC_CHECKPOINT_{n}: recorded public observations through phase {n}."
    )


def observed(messages):
    import hashlib, json

    raw = json.dumps(messages, sort_keys=True, ensure_ascii=False)
    return {
        "sha256": hashlib.sha256(raw.encode()).hexdigest(),
        "checkpoints": [i for i in (1, 2) if summary(i) in raw],
        "sources": [
            f"{g}-{i}" for g in (1, 2) for i in range(6) if f"PUBLIC_{g}_{i}:" in raw
        ],
        "tail": "CURRENT_TAIL_AFTER_TWO_CHECKPOINTS" in raw,
    }


def projection(rows):
    return [
        {k: r[k] for k in ("turn", "kind", "sources", "checkpoints", "tail")}
        for r in rows
    ]


def original_fixture(a, b):
    """Freeze only after both independent owners exercised the same schedule and boundary."""
    from pathlib import Path
    import gzip, hashlib, json

    sha = lambda b: hashlib.sha256(b).hexdigest()
    root = Path(__file__).resolve().parents[1]
    patches = {
        "A": sha(b""),
        "B": sha(gzip.decompress((root / "bench/controls/b.patch.gz").read_bytes())),
    }
    receipts = {}
    expected = {}
    db = None
    for arm, path in [("A", a), ("B", b)]:
        run = json.loads((path / "run.json").read_bytes())
        if not (
            run["arm"] == arm
            and run["source_commit"] == "329ca7c487414d2f54ddd98ff4727446ed2446cd"
            and run["patch_sha256"] == patches[arm]
            and run["public_compact"]
            and run["exit_code"] == run["paid_calls"] == 0
            and run["count"] == 15
        ):
            raise ValueError("invalid original run: " + arm)
        if db is not None and db != run["sqlite"]:
            raise ValueError("mismatched SQLite")
        db = run["sqlite"]
        receipts[arm] = {
            n: sha((path / n).read_bytes())
            for n in [
                "run.json",
                "junit.xml",
                "public-compact.json",
                "public-compact-refused.json",
            ]
        }
        for kind, name in [
            ("adopt", "public-compact.json"),
            ("reject", "public-compact-refused.json"),
        ]:
            sample = json.loads((path / name).read_bytes())
            rows = sample["trace"]
            if (
                len(rows) != 15
                or len(sample["turns"]) != 15
                or any(t["physical"] != 1 for t in sample["turns"])
            ):
                raise ValueError("incomplete physical schedule")
            if [r["wire"]["input"] for r in rows] != [
                f"compact-{i:02}" for i in range(15)
            ]:
                raise ValueError("unbound physical request")
            if len({r["wire"]["declarations_sha256"] for r in rows}) != 1:
                raise ValueError("original declarations changed")
            view = projection(rows)
            if kind in expected and view != expected[kind]:
                raise ValueError("A/B continuity differs")
            if view[13]["checkpoints"] != [1] or view[-1]["checkpoints"] != (
                [1] if kind == "reject" else [2]
            ):
                raise ValueError("checkpoint continuity missing")
            expected[kind] = view
    stimulus = sha(
        json.dumps(list(turns()), ensure_ascii=False, separators=(",", ":")).encode()
    )
    return {
        "source_commit": run["source_commit"],
        "patch_sha256": patches,
        "original_receipts_sha256": receipts,
        "sqlite": db,
        "stimulus_sha256": stimulus,
        "scope": "original source coverage, checkpoint chain and current tail on explicit public compaction",
        "expected": expected,
    }


if __name__ == "__main__":
    import argparse, json
    from pathlib import Path

    p = argparse.ArgumentParser()
    p.add_argument("a", type=Path)
    p.add_argument("b", type=Path)
    p.add_argument("out", type=Path)
    args = p.parse_args()
    data = original_fixture(args.a, args.b)
    with args.out.open("x", encoding="utf-8") as f:
        f.write(json.dumps(data, indent=2, ensure_ascii=False) + "\n")
