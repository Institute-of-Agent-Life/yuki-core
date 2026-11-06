"""Interleave real fresh processes with and without identical untimed starter arrivals."""

import argparse
import json
import os
import platform
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from bench.compare import distribution, equivalent, schedule, trial_schedule  # noqa: E402
from bench.trial_evidence import arm, native_source, sha, sqlite_contract  # noqa: E402
from bench.native_arrivals import freeze, load, pack  # noqa: E402
from ci.source import digest  # noqa: E402


def usage():
    if os.name == "nt":
        return None
    import resource

    u = resource.getrusage(resource.RUSAGE_CHILDREN)
    return u.ru_utime + u.ru_stime


def measure(argv, env, log):
    import psutil

    before = usage()
    start = time.perf_counter()
    peak = 0
    cpu = {}
    sample_count = 0
    with log.open("xb") as output:
        proc = subprocess.Popen(argv, stdout=output, stderr=subprocess.STDOUT, env=env)
        root = psutil.Process(proc.pid)
        while proc.poll() is None:
            rss = 0
            try:
                members = [root, *root.children(recursive=True)]
            except psutil.Error:
                members = []
            for member in members:
                try:
                    with member.oneshot():
                        key = (member.pid, member.create_time())
                        t = member.cpu_times()
                        cpu[key] = max(cpu.get(key, 0), t.user + t.system)
                        rss += member.memory_info().rss
                except psutil.Error:
                    continue
            peak = max(peak, rss)
            sample_count += 1
            time.sleep(0.05)
    after = usage()
    return {
        "exit_code": proc.returncode,
        "wall_seconds": time.perf_counter() - start,
        "waited_cpu_seconds": after - before if after is not None else None,
        "sampled_cpu_seconds": sum(cpu.values()),
        "sampled_tree_rss_peak_bytes": peak,
        "sample_count": sample_count,
        "sample_period_seconds": 0.05,
    }


def trials(args):
    if args.repetitions < 1 or args.warmup < 1:
        raise ValueError(
            "a comparison needs repeated fresh processes and nonempty warmup"
        )
    trial_schedule(args.count, args.warmup)
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    binary, source, optimized, py, library = [
        p.resolve(strict=True)
        for p in (args.binary, args.source, args.optimized, args.python, args.library)
    ]
    # Preserve the venv interpreter path; resolving its symlink changes sys.prefix.
    py = args.python.absolute()
    temp_root = args.temp_root or (Path("/tmp") if os.name != "nt" else None)
    receipt_dir = out / "native-source"
    receipt_dir.mkdir()
    for name in ("manifest.json", "candidate-manifest.json"):
        receipt_path = (
            args.core_manifest
            if name == "manifest.json"
            else args.core_manifest.parent / name
        )
        (receipt_dir / name).write_bytes(receipt_path.read_bytes())
    native = native_source(receipt_dir)
    if native["binary_sha256"] != sha(binary):
        raise ValueError(
            "native executable differs from the verified CORE-039 candidate"
        )
    stimuli = {}
    for warm in (0, args.warmup):
        frames = out / f"qq-{warm}-warmup.ndjson"
        stimulus_hash = freeze(frames, trial_schedule(args.count, warm))
        packet_hashes = [
            digest(pack(p))
            for p in load(frames, stimulus_hash, trial_schedule(args.count, warm))
        ]
        stimuli[warm] = frames, stimulus_hash, packet_hashes
    journal = []
    with tempfile.TemporaryDirectory(dir=temp_root, prefix="yuki-tr-") as temp:
        for rep in range(args.repetitions):
            for block in range(2):
                for i in range(3):
                    name = "ABC"[(i + rep + block) % 3]
                    # Rotate arm order without assigning both of an arm's trials
                    # the same warmup: complementary conditions share one repetition.
                    warmed = (rep + "ABC".index(name) + block) % 2 == 1
                    warm = args.warmup if warmed else 0
                    label = f"{rep + 1}-{block + 1}-{name}-" + (
                        "warm" if warmed else "cold"
                    )
                    directory = out / label
                    directory.mkdir()
                    receipts = directory / "evidence"
                    data = Path(temp) / label
                    data.mkdir()
                    env = {**os.environ, "PYTHONDONTWRITEBYTECODE": "1"}
                    if name in ("A", "B"):
                        if sys.platform == "darwin":
                            env["DYLD_LIBRARY_PATH"] = str(library.parent)
                        elif os.name != "nt":
                            env["LD_LIBRARY_PATH"] = str(library.parent)
                        argv = [
                            py,
                            ROOT / "bench/legacy.py",
                            name,
                            "--checkout",
                            source if name == "A" else optimized,
                            "--library",
                            library,
                            "--out",
                            receipts,
                            "--temp-root",
                            data,
                            "--count",
                            str(args.count),
                        ]
                    else:
                        argv = [
                            sys.executable,
                            ROOT / "bench/rust_slice.py",
                            "--binary",
                            binary,
                            "--data",
                            data / "owner",
                            "--out",
                            receipts,
                            "--limit",
                            str(args.count),
                        ]
                    frames, stimulus_hash, packet_hashes = stimuli[warm]
                    row = {
                        "stimulus_sha256": stimulus_hash,
                        "label": label,
                        "arm": name,
                        "warmup_inputs": warm,
                        "measured_inputs": args.count,
                        "resource_scope": "driver and its descendants; sampled sum of RSS includes shared pages",
                    }
                    try:
                        row.update(
                            measure(
                                [
                                    *argv,
                                    "--warmup",
                                    str(warm),
                                    "--frames",
                                    str(frames),
                                    "--frames-sha256",
                                    stimulus_hash,
                                ],
                                env,
                                directory / "process.log",
                            )
                        )
                        if row["exit_code"]:
                            raise ValueError("workload exited with nonzero status")
                        events, observation = arm(receipts, name)
                        if [
                            event["packet_sha256"] for event in events
                        ] != packet_hashes:
                            raise ValueError(
                                "physical QQ packets differ from the frozen schedule"
                            )
                        row["artifacts"] = observation["files"]
                        row["physical_requests"] = observation["physical_requests"]
                        if name == "C" and sha(binary) != native["binary_sha256"]:
                            raise ValueError(
                                "native executable changed during the trial"
                            )
                        row["all_effects"] = equivalent(
                            trial_schedule(args.count, warm), events
                        )
                        measured = [r for r in events if not r.get("warmup")]
                        row["measured_effects"] = equivalent(
                            schedule(args.count), measured
                        )
                        row["observation"] = distribution(measured)
                        row["writer"] = sqlite_contract(name, observation["sqlite"])
                        row["wire_controls"] = observation["wire_controls"]
                        row["storage"] = json.loads(
                            (receipts / "storage.json").read_bytes()
                        )
                        row["status"] = (
                            "verified"
                            if row["all_effects"]["equal"]
                            and row["measured_effects"]["equal"]
                            else "effect_mismatch"
                        )
                    except Exception as exc:
                        row["status"] = "failed"
                        row["failure"] = str(exc)
                    journal.append(row)
                    (directory / "trial.json").write_text(
                        json.dumps(row, indent=2) + "\n"
                    )
                    with (out / "trials.jsonl").open("a") as f:
                        f.write(json.dumps(row) + "\n")
    complete = all(r["status"] == "verified" for r in journal)
    record = {
        "platform": platform.platform(),
        "status": "verified_effects" if complete else "failed",
        "fresh_processes": len(journal),
        "warmup_policy": "new native process and data for each trial; warm trials process the disjoint startup arrivals first",
        "measured_inputs_per_trial": args.count,
        "cross_arm_speed_comparison_allowed": False,
        "source": "329ca7c487414d2f54ddd98ff4727446ed2446cd",
        "native": native,
        "stimuli": {str(warm): h for warm, (_, h, _) in stimuli.items()},
        "excluded": [
            "shared image accounting",
            "cgroup PSS",
            "invoiced model costs",
            "Work and Compact acceptance",
        ],
    }
    (out / "index.json").write_text(json.dumps(record, indent=2) + "\n")
    if not complete:
        raise RuntimeError("trial failure preserved; inspect trials.jsonl")
    return record


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    for name in ["source", "optimized", "python", "library", "binary", "out"]:
        p.add_argument("--" + name, type=Path, required=True)
    p.add_argument("--core-manifest", required=True, type=Path)
    p.add_argument("--temp-root", type=Path)
    p.add_argument("--count", type=int, default=1200)
    p.add_argument("--repetitions", type=int, default=2)
    p.add_argument("--warmup", type=int, default=8)
    a = p.parse_args()
    if (
        not 1 <= a.count <= 1200
        or not 1 <= a.warmup <= 1200
        or not 1 <= a.repetitions <= 30
    ):
        p.error("positive bounded trial size required")
    print(json.dumps(trials(a)))
