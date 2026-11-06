"""Freeze the QQ packets once; each benchmark arm must consume the same hashed bytes."""

import argparse
import hashlib
import json
from pathlib import Path

from bench.compare import trial_schedule


# The fixed v4 database template seeds the two QQ owners on 2026-08-24 UTC.
ASSIGNMENT_EPOCH = 1787529600


def pack(value):
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def freeze(path, plan):
    with path.open("xb") as out:
        for i, item in enumerate(plan):
            key, target = item["input"], item["target"]
            group = target != "private"
            parts = [{"type": "text", "data": {"text": key}}]
            if group:
                parts.insert(0, {"type": "at", "data": {"qq": "80001"}})
            frame = {
                "post_type": "message",
                "message_type": "group" if group else "private",
                "sub_type": "normal" if group else "friend",
                "self_id": 80001,
                "user_id": 10001,
                "sender": {"user_id": 10001, "nickname": "真实参与者"},
                "message_id": 5000 + i,
                "time": ASSIGNMENT_EPOCH + 1 + i,
                "font": 0,
                "message": parts,
                "raw_message": ("[CQ:at,qq=80001]" if group else "") + key,
            }
            if group:
                frame["group_id"] = int(target)
            out.write(pack(frame) + b"\n")
    return sha(path.read_bytes())


def load(path, digest, plan):
    raw = path.read_bytes()
    if sha(raw) != digest:
        raise ValueError("native QQ stimulus digest mismatch")
    packets = [json.loads(line) for line in raw.splitlines()]
    if len(packets) != len(plan):
        raise ValueError("native QQ stimulus count mismatch")
    ids = set()
    for row, frame in zip(plan, packets):
        parts = frame["message"]
        if (
            [part["data"]["text"] for part in parts if part["type"] == "text"]
            != [row["input"]]
            or str(frame.get("group_id", "private")) != row["target"]
            or frame["message_type"]
            != ("group" if row["target"] != "private" else "private")
            or frame["self_id"] != 80001
            or frame["message_id"] in ids
        ):
            raise ValueError("packet does not match scheduled QQ arrival")
        ids.add(frame["message_id"])
    return packets


if __name__ == "__main__":
    cli = argparse.ArgumentParser()
    cli.add_argument("out", type=Path)
    cli.add_argument("--count", type=int, default=1200)
    cli.add_argument("--warmup", type=int, default=0)
    a = cli.parse_args()
    print(freeze(a.out, trial_schedule(a.count, a.warmup)))
