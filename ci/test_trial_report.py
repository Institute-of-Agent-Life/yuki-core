"""A failed cold/warm process remains in the resource and evidence accounting."""

import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from bench.trial_report import report
from ci.source import digest


class FailedTrials(unittest.TestCase):
    def test_digest_resource_and_unknowns(self):
        with tempfile.TemporaryDirectory() as name:
            run = Path(name)
            rows = []
            for block, condition in enumerate(["cold", "warm"], start=1):
                for arm in "ABC":
                    label = f"1-{block}-{arm}-{condition}"
                    folder = run / label
                    folder.mkdir()
                    row = {
                        "label": label,
                        "arm": arm,
                        "status": "failed",
                        "failure": "owner exited",
                        "measured_inputs": 2,
                        "warmup_inputs": int(condition == "warm"),
                        "resource_scope": "process tree",
                    }
                    if arm == "A":
                        output = b"attempted work, process exited\n"
                        (folder / "process.log").write_bytes(output)
                        row.update(
                            exit_code=1,
                            wall_seconds=0.25,
                            sampled_cpu_seconds=0.1,
                            sampled_tree_rss_peak_bytes=1024,
                            failure_log_sha256=digest(output),
                        )
                    if arm == "B":
                        row.pop("failure")
                        row.update(
                            status="effect_mismatch",
                            exit_code=0,
                            measured_effects={"equal": False, "at": 1},
                        )
                    (folder / "trial.json").write_text(json.dumps(row))
                    rows.append(row)
            (run / "trials.jsonl").write_text("\n".join(map(json.dumps, rows)))
            (run / "index.json").write_text(
                json.dumps(
                    {
                        "fresh_processes": 6,
                        "measured_inputs_per_trial": 2,
                        "source": "329ca7c487414d2f54ddd98ff4727446ed2446cd",
                        "platform": "fixture",
                    }
                )
            )
            result = report(run)
            self.assertEqual(result["scheduled_measured_inputs"], 12)
            self.assertEqual(result["verified_correct_inputs"], 0)
            self.assertFalse(result["cross_arm_speed_comparison_allowed"])
            self.assertEqual(len(result["measurements"]), 6)
            self.assertEqual(len(result["sources"]), 6)
            first, mismatch, unavailable = result["measurements"][:3]
            self.assertEqual(
                (first["wall_seconds"], first["sampled_tree_rss_peak_bytes"]),
                (0.25, 1024),
            )
            self.assertEqual(result["failed_trials"][0]["failure"], "owner exited")
            self.assertEqual(result["failed_trials"][1]["measured_effects"]["at"], 1)
            self.assertEqual(mismatch["status"], "effect_mismatch")
            self.assertIsNone(unavailable["sampled_cpu_seconds"])
            first_log = run / rows[0]["label"] / "process.log"
            first_log.write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "changed failure log"):
                report(run)
            first_log.unlink()
            with self.assertRaisesRegex(ValueError, "changed failure log"):
                report(run)
