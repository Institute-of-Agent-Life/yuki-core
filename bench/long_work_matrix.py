"""Replay native Rust Works and compare all recorded HTTP calls with both fixed v4 owners."""

import argparse
import gzip
import json
from pathlib import Path
import sys
import tempfile
import traceback
from xml.etree import ElementTree

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from bench.file_receipt import delivered, projection as file_projection
from bench.work_trace import initial, projection as roles, sha

BASE = "329ca7c487414d2f54ddd98ff4727446ed2446cd"
COUNT = 20


def read(path):
    return json.loads(path.read_bytes())


def semantics(requests):
    if len(requests) != 6:
        raise ValueError("expected six physical model calls")
    return {
        "initial": initial(requests),
        "work_material": roles(requests),
        "file_and_caption": file_projection(delivered(requests[-1])),
    }


def rust(out):
    from bench.core_long import run
    from ci.source import source_identity
    from tests.harness import Context
    from tests.harness.network import install

    source = source_identity(ROOT)
    lock = read(ROOT / "ci/checks.json")
    install()
    results = []
    for i in range(COUNT):
        folder = out / f"work-{i:02}"
        folder.mkdir()
        ctx = Context(ROOT, folder, lock)
        with tempfile.TemporaryDirectory(prefix="yw-") as tmp:
            ctx.working = Path(tmp)
            try:
                run(ctx, i)
                results.append(True)
            except Exception:
                results.append(False)
                (folder / "failure.txt").write_text(
                    traceback.format_exc(), encoding="utf-8"
                )
            finally:
                ctx.write("assertions.json", ctx.assertions)
    binary = (
        ROOT
        / "target/debug"
        / ("yuki-core.exe" if sys.platform == "win32" else "yuki-core")
    )
    meta = {
        "arm": "C",
        "source_commit": source["commit"],
        "source_dirty": source["dirty"],
        "source_tree_sha256": source["tree_sha256"],
        "binary_sha256": sha(binary.read_bytes()),
        "long_works": COUNT,
        "works": results,
        "exit_code": int(not all(results)),
    }
    (out / "run.json").write_text(json.dumps(meta, indent=2) + "\n")
    return meta["exit_code"]


def compare(a, b, c):
    paths = {"A": a, "B": b, "C": c}
    runs = {arm: read(path / "run.json") for arm, path in paths.items()}
    patch = gzip.decompress((ROOT / "bench/controls/b.patch.gz").read_bytes())
    for arm, expected in [("A", b""), ("B", patch)]:
        r = runs[arm]
        if (
            r["arm"] != arm
            or r["source_commit"] != BASE
            or r["patch_sha256"] != sha(expected)
            or r["long_works"] != COUNT
            or r["paid_calls"] != 0
        ):
            raise ValueError("incorrect v4 benchmark source or load")
    if (
        runs["A"]["sqlite"] != runs["B"]["sqlite"]
        or runs["A"]["sqlite_library_sha256"] != runs["B"]["sqlite_library_sha256"]
        or runs["C"]["arm"] != "C"
        or len(runs["C"]["works"]) != COUNT
    ):
        raise ValueError("incompatible controls")
    junit = {}
    for arm in ("A", "B"):
        cases = list(
            ElementTree.parse(paths[arm] / "junit.xml").getroot().iter("testcase")
        )
        junit[arm] = {case.get("name"): len(case) == 0 for case in cases}
        if len(cases) != len(junit[arm]):
            raise ValueError("duplicate original Work case")
    result = []
    for i in range(COUNT):
        case = {"index": i, "arms": {}, "errors": {}}
        projections = {}
        for arm, path in paths.items():
            try:
                if arm == "C":
                    folder = path / f"work-{i:02}"
                    checks = read(folder / "assertions.json")
                    if (
                        not runs["C"]["works"][i]
                        or not checks
                        or not all(r["passed"] is True for r in checks)
                    ):
                        raise ValueError("Rust Work failed")
                    wire = folder / f"C-long-{i:02}-wire.json"
                else:
                    if not junit[arm].get(f"test_original_work[{i}]"):
                        raise ValueError("original Work failed or did not finish")
                    wire = path / f"work-{i:02}-requests.json"
                requests = read(wire)
                if arm != "C":
                    rows = read(path / f"work-{i:02}-wire.json")
                    for request, receipt in zip(requests, rows, strict=True):
                        raw = json.dumps(
                            request, ensure_ascii=False, separators=(",", ":")
                        ).encode()
                        if (
                            receipt["input"] != f"long-{i}"
                            or receipt["sha256"] != sha(raw)
                            or receipt["bytes"] != len(raw)
                        ):
                            raise ValueError("physical body digest mismatch")
                projections[arm] = semantics(requests)
                case["arms"][arm] = {
                    "physical_calls": len(requests),
                    "requests_sha256": sha(wire.read_bytes()),
                }
            except (FileNotFoundError, ValueError, TypeError, KeyError) as error:
                case["errors"][arm] = str(error)
        case["equal"] = (
            not case["errors"]
            and projections["A"] == projections["B"] == projections["C"]
        )
        if not case["equal"]:
            case["observations"] = projections
        result.append(case)
    return {
        "works_per_arm": COUNT,
        "runs": {k: sha((p / "run.json").read_bytes()) for k, p in paths.items()},
        "v4_source": BASE,
        "rust_source": runs["C"]["source_commit"],
        "cases": result,
        "verified": all(x["equal"] for x in result)
        and all(r["exit_code"] == 0 for r in runs.values())
        and not runs["C"]["source_dirty"],
        "timing_comparison_allowed": False,
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    sub.add_parser("rust").add_argument("out", type=Path)
    p = sub.add_parser("compare")
    for name in ("a", "b", "c", "out"):
        p.add_argument(name, type=Path)
    args = parser.parse_args()
    if args.mode == "rust":
        args.out.mkdir(parents=True, exist_ok=False)
        sys.exit(rust(args.out))
    result = compare(args.a, args.b, args.c)
    with args.out.open("x", encoding="utf-8") as f:
        f.write(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
    sys.exit(int(not result["verified"]))
