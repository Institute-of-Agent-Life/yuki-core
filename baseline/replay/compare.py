"""Verify immutable v4 captures offline without an installed v4 runtime."""
import base64
import gzip
import json
from collections import Counter
import random

from baseline.replay.freeze import digest, FIXTURES, PROTOCOL


def compare(left, right, path=""):
    """Report the first structural difference, including list order and native bytes."""
    if type(left) is not type(right):
        return [path]
    if isinstance(left, dict):
        if left.keys() != right.keys():
            return [path + " (keys)"]
        items = ((key, left[key], right[key]) for key in left)
    elif isinstance(left, list):
        if len(left) != len(right):
            return [path + " (length)"]
        items = ((i, a, b) for i, (a, b) in enumerate(zip(left, right)))
    else:
        return [] if left == right else [path]
    for key, a, b in items:
        if delta := compare(a, b, f"{path}/{key}"):
            return delta
    return []


def observe(run, spec):
    assert set(run) == set(spec["cases"])
    for key, nodeid in spec["cases"].items():
        assert run[key]["nodeid"] == nodeid
    for key in ("next_main", "signature"):
        observed = [json.loads(base64.b64decode(r["body_bytes"], validate=True)) for r in run[key]["http"]]
        assert observed == run[key]["wire_requests"]
        assert all(r["status"] == 200 and r["method"] == "POST" for r in run[key]["http"])
    assert len(run["next_main"]["http"]) == 3
    assert sum(name == "send_group_msg" for name, _ in run["next_main"]["gateway_calls"]) == 6
    assert run["signature"]["maintenance"] == ["E", "P"]
    assert "original-native-signature-" in json.dumps(run["signature"]["wire_requests"])
    partial = run["partial"]
    assert [i[0] for i in partial["gateway_calls"]].count("upload_group_file") == 1
    receipts = partial["tables"]["social_operation_receipts"]
    assert any(r["status"] == "succeeded" and r["platform_reference"] for r in receipts)
    assert any(r["status"] == "uncertain" and r["error_category"] == "ReadTimeout" for r in receipts)
    app = run["app_work"]
    work, = app["tables"]["runtime_work"]
    supplied, = app["tables"]["runtime_work_inputs"]
    assert supplied["state"] == "consumed" and work["state"] == "completed"
    assert supplied["work_id"] == work["id"] and supplied["generation"] == work["generation"]
    assert len(app["tables"]["runtime_work_effects"]) == work["model_requests"] == 6
    assert any(frame["path"] == "/client/v1/pair" and frame["status"] == 200 for frame in app["loopback"])
    assert sum(name == "upload_private_file" for name, _ in app["gateway_calls"]) == 1
    frames = run["wire4"]["frames"]
    hello = [i for i, f in enumerate(frames) if f["body"]["type"] == "hello"]
    assert len(hello) == 2 and hello[0] == 0
    before, after = ([f["body"]["sequence"] for f in chunk if f["body"]["type"] == "event"]
                     for chunk in (frames[:hello[1]], frames[hello[1]:]))
    assert before == list(range(1, 41)) and after == list(range(2, 41))


def validate_bundle(root):
    lock = json.loads((root / "baseline/lock.json").read_bytes())
    spec = json.loads((root / "baseline/replay/scenarios.json").read_bytes())
    assert lock["source_commit"] == spec["source_commit"] and lock["source_tree"] == spec["source_tree"]
    for filename, expected in lock["files"].items():
        file = (root / filename).resolve()
        assert file.is_relative_to(root.resolve()) and digest(file.read_bytes()) == expected, filename
    for file, expected in lock["historical_evidence"].items():
        assert digest((root / "docs/taskbook/v3.2/audit/results" / file).read_bytes()) == expected
    fixtures = root / FIXTURES
    runs = []
    evidence = []
    for name in ("first", "second"):
        case = fixtures / name
        info = json.loads((case / "run.json").read_bytes())
        raw = gzip.decompress((case / "transcript.json.gz").read_bytes())
        assert digest(raw) == info["raw_sha256"] and info["nodeids"] == spec["cases"]
        assert info["source_commit"] == lock["source_commit"] and info["tree"] == lock["source_tree"]
        assert info["uv_lock_sha256"] == lock["source_files"]["uv.lock"]["sha256"]
        assert sorted(e["name"] for e in info["junit"]["cases"]) == sorted(s.split("::")[-1] for s in spec["cases"].values())
        run = json.loads(raw)
        observe(run, spec)
        for key in ("next_main", "signature"):
            for file in run[key]["verified_sources"]:
                assert file["sha256"] == lock["source_files"][file["path"]]["sha256"]
        runs.append(run)
        evidence.append({"run": name, "transcript_sha256": digest(raw), "junit": info["junit"]["original_sha256"],
                         "sqlite": info["sqlite"], "vec1_sha256": info["vec1_sha256"]})
    assert evidence[0]["junit"] != evidence[1]["junit"]
    assert not compare(*runs)
    assert json.loads((root / PROTOCOL / "client-openapi.json").read_bytes())["info"]["x-yuki-api-revision"] == 9
    assert json.loads((root / PROTOCOL / "wire4-frames.json").read_bytes()) == runs[0]["wire4"]["frames"]
    counts = Counter()
    seen = set()
    def lines(name):
        return [json.loads(line) for line in gzip.decompress((fixtures / name).read_bytes()).splitlines()]
    expected_schedule = ["private"] * spec["load"]["private"]
    for target, count in spec["load"]["groups"].items():
        expected_schedule.extend([target] * count)
    random.Random(spec["seed"]).shuffle(expected_schedule)
    arrivals = lines("arrivals.jsonl.gz")
    assert [row["target"] for row in arrivals] == expected_schedule
    for ordinal, arrival in enumerate(arrivals):
        key, target = arrival["input"], arrival["target"]
        assert key == f"a-{ordinal:04d}" and key not in seen and arrival["reason"] == "chat"
        seen.add(key)
        call, = arrival["gateway"]
        event, = arrival["event_ids"]
        assert event > 0 and "reply-" + key in json.dumps(call)
        assert str(call.get("user_id" if target == "private" else "group_id")) == ("10001" if target == "private" else target)
        counts[target if target == "private" else "group:" + target] += 1
    works = lines("long-works.jsonl.gz")
    assert [row["work"] for row in works] == list(range(spec["load"]["long_works"]))
    assert all(row["original"] == spec["cases"]["app_work"] for row in works)
    counts["long_works"] = len(works)
    load = json.loads((fixtures / "load.json").read_bytes())
    assert load["exit_code"] == 0 and load["source_commit"] == lock["source_commit"] and not load["smoke"]
    workload = {"counts": dict(counts), "line": "A", "model": load["model"], "process_cpu_seconds": load["process_cpu_seconds"],
                "elapsed_seconds": load["elapsed_seconds"], "B": "same as A until a verified patch exists",
                "C": "pending Rust feature-equivalent host"}
    historical_dir = root / "docs/taskbook/v3.2/audit/results"
    upstream = json.loads((historical_dir / "ci-source-proof.json").read_bytes())
    review = json.loads((historical_dir / "r2-source-proof.json").read_bytes())
    historical = {"ci_sha": upstream["ci_sha"], "archives": upstream["archive_hashes"],
                  "compact_then_normal": upstream["checked_public_then_normal"],
                  "original_receipt": review["compact_example"]}
    index = {"historical_ci": historical, "status": "unpublished", "source_commit": lock["source_commit"], "source_tree": lock["source_tree"],
             "evidence": evidence, "cases": spec["cases"], "workload": workload, "old_schema": lock["schema"],
             "client_revision": 9, "connector_wire": 4, "connector_distribution": "0.4.3",
             "goldens": {k: v for k, v in lock["files"].items() if k.startswith("protocol/")},
             "not_witnessed": ["real QQ packets from two bots", "Rust plugin SDK", "external consumer acceptance"]}
    return {"runs": runs, "index": index, "workload": workload}
