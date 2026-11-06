"""Run Rust's real QQ → Chat → send on four shared native packets against the independently replayed v4 slice."""

import json
import os
from copy import deepcopy
from pathlib import Path

from bench.compare import equivalent, trial_schedule
from bench.native_arrivals import freeze, load, sha
from bench.native_contract import differences, request_fields, send_fields
from bench.rust_slice import run as replay


def run(ctx):
    oracle = ctx.root / "fixtures/oracle/v4/native-group-contract.json"
    original = json.loads(oracle.read_bytes())
    plan = trial_schedule(4, 0)
    frames = ctx.working / "frames.jsonl"
    digest = freeze(frames, plan)
    ctx.equal(
        "replay the packets that reached both original applications",
        digest,
        original["frames_sha256"],
    )
    ctx.rejects(
        "wrong stimulus digest cannot start an arm",
        ValueError,
        lambda: load(frames, "0" * 64, plan),
    )
    binary = (
        ctx.root
        / "target/debug"
        / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
    )
    proof = ctx.output / "rust-replay"
    observed = replay(binary, ctx.working / "owner", proof, frames, digest, 4)
    ctx.write("sqlite-build.json", observed["sqlite"])
    rows = [
        json.loads(line)
        for line in (proof / "arrivals.jsonl").read_bytes().splitlines()
    ]
    ctx.equal(
        "all native packets reached the correct Rust execution",
        [r["packet_sha256"] for r in rows],
        original["packet_sha256"],
    )
    ctx.check(
        "all four original receiver/content/outbound event observations match",
        equivalent(plan, rows)["equal"],
    )
    fault = deepcopy(rows)
    fault[1]["gateway"].pop()
    ctx.check(
        "omitting one private send breaks equivalence",
        not equivalent(plan, fault)["equal"],
    )
    ctx.equal(
        "each real QQ turn used the two physical Chat requests required by the original script",
        [len(x["wire"]) for x in observed["wire_timings"]],
        [2] * 4,
    )
    request, result = observed["wire_samples"]
    current = request_fields(request)
    ctx.equal(
        "admitted Person, original-zone clock and bot mention render as in the original",
        current["material"],
        original["original"]["material"],
    )
    ctx.equal(
        "the physical system/runtime prefix matches the original",
        current["prefix"],
        original["original"]["prefix"],
    )
    diff = differences(original["original"], current)
    ctx.check(
        "effective Chat options match both original applications",
        diff["request_knobs_equal"],
    )
    ctx.equal("the actual confirmed text send envelope and data fields agree with the recorded original",
              send_fields(result),original["original_send_shape"])
    from bench.compact_trace import projection
    from bench.core_compact import run as compact
    compact_fixture=ctx.root/'fixtures/oracle/v4/two-public-checkpoints.json'
    expected=json.loads(compact_fixture.read_bytes())
    compact_results={}
    for name,refuse in [('adopt',False),('reject',True)]:
        rows=projection(compact(ctx,refuse))
        ctx.equal('all fifteen physical requests retain the original public sources and checkpoints: '+name,rows,expected['expected'][name])
        compact_results[name]={'physical_requests':len(rows),'final':rows[-1]}
    from bench.core_long import run as long_work
    work_fixture = ctx.root / "fixtures/oracle/v4/work-answered-question.json"
    original_work = json.loads(work_fixture.read_bytes())
    work = long_work(ctx, 0)
    ctx.equal("the original question and phone answer keep their roles across both cold owners",
              work["requests"], original_work["requests"])
    lost = long_work(ctx, 1, withdraw_question=True)
    return {
        "long_work": {"oracle_sha256": sha(work_fixture.read_bytes()), "normal": work, "withdrawn": lost},
        "explicit_public_compact":{"oracle_sha256":sha(compact_fixture.read_bytes()),"scenarios":compact_results},
        "scope": original["scope"],
        "v4_source": original["source_commit"],
        "original_receipts": original["arms"],
        "oracle_sha256": sha(oracle.read_bytes()),
        "frames_sha256": digest,
        "rust_binary_sha256": sha(binary.read_bytes()),
        "inputs": len(plan),
        "physical_requests": 8,
        "send_fault_detected": True,
        "group_material": current["material"],
        "declarations": diff,
        "model_visible_send_shape_equal": send_fields(result)
        == original["original_send_shape"],
        "status": "partial_evidence",
        "timing_comparison_allowed": False,
        "remaining": [
            "remaining tool declaration differences and untested receipt variants",
            "full long Work wire and automatic P/E Compact equivalence; controlled cross-platform timings",
        ],
    }
