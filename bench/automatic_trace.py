"""Build a portable automatic Compact oracle from both pinned v4 runs."""

import argparse
import gzip
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BASE = "329ca7c487414d2f54ddd98ff4727446ed2446cd"


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def observation(kind, wire, stimulus):
    messages = wire["messages"]
    content = "\n".join(
        m["content"] for m in messages if isinstance(m.get("content"), str)
    )
    call = stimulus["call_id"]
    receipts = [
        json.loads(m["content"]) for m in messages if m.get("tool_call_id") == call
    ]
    if len(receipts) > 1:
        raise ValueError("duplicated original send receipt")
    parts = None
    if receipts:
        receipt = receipts[0]
        result = receipt["data"]
        delivery = result["parts"]
        if (
            receipt["ok"] is not True
            or result["status"] != "succeeded"
            or len(delivery) != result["sent_messages"]
            or not delivery
            or len({p["operation_id"] for p in delivery}) != len(delivery)
            or len({p["event_id"] for p in delivery}) != len(delivery)
            or not all(
                p["operation_id"] and p["event_id"] and p["status"] == "succeeded"
                for p in delivery
            )
        ):
            raise ValueError("unconfirmed or duplicated delivery in the model receipt")
        parts = [p["delivered_text"] for p in delivery]
    return {
        "kind": kind,
        "question": stimulus["question"] in content,
        "old_public": stimulus["old_public"] in content,
        "public_checkpoint": "PUBLIC_CHECKPOINT: preserve the earlier decision."
        in content,
        "private_checkpoint": "PRIVATE_CHECKPOINT: fanout was answered successfully; no remaining action."
        in content,
        "tool_calls": [c["id"] for m in messages for c in m.get("tool_calls") or []],
        "responses": [m["tool_call_id"] for m in messages if m.get("tool_call_id")],
        "delivered_parts": parts,
    }


def freeze(a, b):
    result = {"source_commit": BASE, "arms": {}, "scenarios": {}}
    patch = gzip.decompress((ROOT / "bench/controls/b.patch.gz").read_bytes())
    for arm, directory, expected_patch in [("A", a, b""), ("B", b, patch)]:
        raw_run = (directory / "run.json").read_bytes()
        run = json.loads(raw_run)
        if (
            run["source_commit"] != BASE
            or run["arm"] != arm
            or run["patch_sha256"] != sha(expected_patch)
            or run["exit_code"] != 0
            or not run["automatic_compact"]
            or run["count"] != 2
            or run["paid_calls"] != 0
        ):
            raise ValueError("unverified original automatic Compact run")
        controls = [run["sqlite"], run["sqlite_library_sha256"]]
        if arm == "A":
            result["sqlite"] = controls
        elif result["sqlite"] != controls:
            raise ValueError("different SQLite controls")
        evidence = {}
        for scenario in ["finish", "p_refused"]:
            raw = (directory / scenario / "original.json").read_bytes()
            d = json.loads(raw)
            stimulus = {
                k: d[k]
                for k in ("old_public", "question", "answer", "call_id", "trigger")
            }
            if arm == "A" and scenario == "finish":
                result["stimulus"] = stimulus
            elif result["stimulus"] != stimulus:
                raise ValueError("changed original stimulus")
            history = [
                observation(r["kind"], wire, stimulus)
                for r, wire in zip(d["requests"], d["wire_requests"], strict=True)
            ]
            delivered = [
                x["content"] for x in d["events"] if x["direction"] == "outbound"
            ]
            if history[-1]["delivered_parts"] != delivered or len(delivered) != sum(
                name == "send_group_msg" for name, _ in d["gateway_calls"]
            ):
                raise ValueError(
                    "retained receipt does not match original group delivery"
                )
            if arm == "A":
                result["scenarios"][scenario] = history
            elif result["scenarios"][scenario] != history:
                raise ValueError("original owners disagree on " + scenario)
            evidence[scenario] = sha(raw)
        result["arms"][arm] = {
            "original": evidence,
            "run_sha256": sha(raw_run),
            "patch_sha256": sha(expected_patch),
        }
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("a", "b", "out"):
        parser.add_argument(name, type=Path)
    args = parser.parse_args()
    oracle = freeze(args.a, args.b)
    with args.out.open("xb") as f:
        f.write(
            gzip.compress(
                json.dumps(oracle, sort_keys=True, ensure_ascii=False).encode(), mtime=0
            )
        )
