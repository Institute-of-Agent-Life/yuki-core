"""Compare the observed arms to each other and the fixed schedule before interpreting any timings."""

import hashlib
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from bench.compare import distribution  # noqa: E402


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def arm(path, label):
    run = json.loads((path / "run.json").read_bytes())
    rows = [
        json.loads(v)
        for v in (path / "arrivals.jsonl").read_text(encoding="utf-8").splitlines()
    ]
    if (
        run["arm"] != label
        or run.get("exit_code", 0) != 0
        or not rows
        or run["paid_calls"] != 0
    ):
        raise ValueError("invalid original arm receipt " + label)
    wires = (
        json.loads((path / "wire.json").read_bytes())
        if label != "C"
        else run["wire_timings"]
    )
    samples = (
        json.loads((path / "wire-samples.json").read_bytes())
        if label != "C"
        else run["wire_samples"]
    )
    if [r["input"] for r in rows] != [r["input"] for r in wires] or not all(
        w["wire"] for w in wires
    ):
        raise ValueError("physical requests do not cover the original inputs")
    first = samples[0]
    air = {
        name: sorted({str(w[name]) for group in wires for w in group["wire"]})
        for name in ["declarations_sha256", "prefix_sha256", "model", "max_tokens"]
    }
    work_context = next(
        (
            m["content"]
            for m in first["messages"]
            if "<runtime_context>" in (m.get("content") or "") and m["role"] == "user"
        ),
        None,
    )
    return rows, {
        "path": str(path.resolve()),
        "files": {
            name: sha(path / name)
            for name in ["run.json", "arrivals.jsonl", "wire.json", "wire-samples.json"]
            if (path / name).is_file()
        },
        "physical_requests": sum(len(group["wire"]) for group in wires),
        "wire_bytes": sum(w["bytes"] for group in wires for w in group["wire"]),
        "declared_tools": sorted(t["function"]["name"] for t in first["tools"]),
        "wire_controls": air,
        "work_state": work_context,
        "first_event_has_scene": first["messages"][-1]["content"].startswith("当前是"),
        "sqlite": run["sqlite"],
        "driver_cpu_seconds": run.get("driver_cpu_seconds"),
        "native_process_start_seconds": run.get("process_start_seconds"),
        "latency_observation": distribution(rows),
        "vendor_invoice": None,
    }


def sqlite_contract(kind, values):
    if kind == "C":
        if values.get("observed_connection") != "writer":
            return None
        options = values.get("compile_options")
        digest = (
            hashlib.sha256(json.dumps(sorted(options)).encode()).hexdigest()
            if options
            else None
        )
        version = values.get("sqlite_version")
        journal = values.get("journal_mode")
    else:
        version = values.get("version")
        journal = values.get("journal")
        digest = values.get("compile_options_sha256")
    fields = [
        version,
        journal,
        values.get("synchronous"),
        values.get("foreign_keys"),
        values.get("busy_timeout_ms"),
        digest,
    ]
    return fields if all(v is not None for v in fields) else None


def native_source(directory):
    manifest_path = directory / "manifest.json"
    manifest = json.loads(manifest_path.read_bytes())
    candidate_path = directory / "candidate-manifest.json"
    candidate = json.loads(candidate_path.read_bytes())
    source = manifest["source"]
    if (
        manifest["card"] != "CORE-039"
        or manifest["status"] != "verified"
        or manifest["failure"]
        or manifest["exit_code"]
        or source["dirty"]
        or candidate["dirty"]
        or not manifest["assertions"]
        or not all(a["passed"] is True for a in manifest["assertions"])
        or manifest["artifacts"].get(candidate_path.name) != sha(candidate_path)
        or candidate["source_commit"] != source["commit"]
        or candidate["source_tree_sha256"] != source["tree_sha256"]
    ):
        raise ValueError("invalid release candidate provenance")
    return {
        "commit": source["commit"],
        "tree_sha256": source["tree_sha256"],
        "binary_sha256": candidate["binary_sha256"],
        "manifest_sha256": sha(manifest_path),
    }
