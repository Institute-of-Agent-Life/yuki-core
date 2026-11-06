"""Reconcile original trial files and keep measurements scoped to reproduced effects."""

import argparse
import gzip
import json
import re
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from bench.compare import distribution, equivalent, schedule, trial_schedule  # noqa: E402
from bench.native_arrivals import load, pack  # noqa: E402
from bench.trial_evidence import arm, native_source, sha, sqlite_contract  # noqa: E402
from ci.source import digest  # noqa: E402


def require(check, message):
    if not check:
        raise ValueError(message)


def report(directory):
    index = json.loads((directory / "index.json").read_bytes())
    journal = [
        json.loads(line)
        for line in (directory / "trials.jsonl").read_bytes().splitlines()
    ]
    require(
        len(journal) == index["fresh_processes"]
        and len(journal) > 0
        and len(journal) % 6 == 0,
        "incomplete repetitions",
    )
    require(
        index["source"] == "329ca7c487414d2f54ddd98ff4727446ed2446cd",
        "wrong original owner",
    )
    measurements = []
    seen = set()
    writers = set()
    contracts = {"A": set(), "B": set(), "C": set()}
    native = index.get("native")
    if "native" in index or (directory / "native-source").exists():
        require(isinstance(native, dict), "missing native source receipt")
        require(
            native == native_source(directory / "native-source"),
            "changed native source receipt",
        )
    patches = {
        "A": digest(b""),
        "B": digest(gzip.decompress((ROOT / "bench/controls/b.patch.gz").read_bytes())),
    }
    total = correct = 0
    sources = {}
    failed = []
    for row in journal:
        label, name = row["label"], row["arm"]
        match = re.fullmatch(r"([1-9][0-9]*)-([12])-([ABC])-(cold|warm)", label)
        require(
            match and match[3] == name and label not in seen, "invalid trial identity"
        )
        seen.add(label)
        folder = directory / label
        require(
            json.loads((folder / "trial.json").read_bytes()) == row,
            "journal differs from the trial",
        )
        count, warm = row["measured_inputs"], row["warmup_inputs"]
        require(
            count == index["measured_inputs_per_trial"]
            and bool(warm) == (match[4] == "warm"),
            "inconsistent load",
        )
        total += count
        status = row["status"]
        require(
            status in {"verified", "failed", "effect_mismatch"}, "unknown trial status"
        )
        sources[label] = {"trial_sha256": sha(folder / "trial.json")}
        metrics = (
            "wall_seconds",
            "sampled_cpu_seconds",
            "waited_cpu_seconds",
            "sampled_tree_rss_peak_bytes",
            "resource_scope",
            "storage",
            "observation",
        )
        measurement = {
            key: row[key] if status == "verified" else row.get(key) for key in metrics
        }
        measurement.update(
            label=label,
            status=status,
            measured_inputs=count,
            warmup_inputs=warm,
            exit_code=row.get("exit_code"),
            sample_count=row.get("sample_count"),
        )
        measurements.append(measurement)
        if status != "verified":
            log = folder / "process.log"
            observed = sha(log) if log.is_file() else None
            if "failure_log_sha256" in row:
                require(row["failure_log_sha256"] == observed, "changed failure log")
            sources[label].update(
                process_log_sha256=observed,
                log_digest_recorded_at_capture="failure_log_sha256" in row,
            )
            failed.append(
                {
                    key: row.get(key)
                    for key in (
                        "label",
                        "status",
                        "failure",
                        "exit_code",
                        "all_effects",
                        "measured_effects",
                    )
                }
            )
            continue
        raw = folder / "evidence"
        run = json.loads((raw / "run.json").read_bytes())
        if name in patches:
            require(
                run["patch_sha256"] == patches[name]
                and run["source_commit"] == index["source"],
                "wrong original owner",
            )
        events, obs = arm(raw, name)
        if native is not None:
            h = index["stimuli"][str(warm)]
            packets = load(
                directory / f"qq-{warm}-warmup.ndjson", h, trial_schedule(count, warm)
            )
            require(
                h == row["stimulus_sha256"]
                and [digest(pack(p)) for p in packets]
                == [v["packet_sha256"] for v in events],
                "changed QQ inputs",
            )
            require(obs["files"] == row["artifacts"], "changed evidence bytes")
        measured = [v for v in events if not v.get("warmup")]
        effects = equivalent(schedule(count), measured)
        require(
            equivalent(trial_schedule(count, warm), events) == row["all_effects"]
            and effects == row["measured_effects"]
            and effects["equal"]
            and len(measured) == count,
            "delivery mismatch",
        )
        require(
            not equivalent(
                schedule(count), [{**measured[0], "gateway": []}, *measured[1:]]
            )["equal"],
            "missing confirmed send was accepted",
        )
        require(
            distribution(measured) == row["observation"] and row["exit_code"] == 0,
            "timing or completion mismatch",
        )
        writer = sqlite_contract(name, obs["sqlite"])
        require(
            writer
            and writer == row["writer"]
            and obs["wire_controls"] == row["wire_controls"],
            "changed writer or model controls",
        )
        require(
            json.loads((raw / "storage.json").read_bytes()) == row["storage"],
            "changed storage observations",
        )
        writers.add(tuple(writer))
        contracts[name].add(json.dumps(obs["wire_controls"], sort_keys=True))
        sources[label]["files"] = obs["files"]
        correct += count
    repeats = len(journal) // 6
    for rep in range(1, repeats + 1):
        for name in "ABC":
            cases = [
                label
                for label in seen
                if label.startswith(f"{rep}-") and f"-{name}-" in label
            ]
            require(
                len(cases) == 2
                and {label.split("-")[1] for label in cases} == {"1", "2"}
                and {label.split("-")[-1] for label in cases} == {"cold", "warm"},
                "unbalanced interleaving",
            )
    return {
        "source": index["source"],
        "native": native,
        "source_bound_native_binary": native is not None,
        "platform": index["platform"],
        "journal_sha256": sha(directory / "trials.jsonl"),
        "repetitions": repeats,
        "fresh_processes": len(journal),
        "scheduled_measured_inputs": total,
        "verified_correct_inputs": correct,
        "failed_trials": failed,
        "sources": sources,
        "writer_settings_equal": len(writers) == 1 and not failed,
        "model_controls_equal": len(contracts["A"]) == 1
        and contracts["A"] == contracts["B"] == contracts["C"]
        and not failed,
        "measurements": measurements,
        "vendor_invoice": None,
        "cost_per_correct_delivery": None,
        "cross_arm_speed_comparison_allowed": False,
        "scope": "offline QQ arrivals only; full Work, Compact and installed tool parity are separate gates",
    }


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("trials", type=Path)
    cli.add_argument("out", type=Path)
    a = cli.parse_args()
    result = report(a.trials)
    with a.out.open("x", encoding="utf-8") as out:
        json.dump(result, out, indent=2, ensure_ascii=False)
        out.write("\n")
