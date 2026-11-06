"""Freeze the visible question and answer at each physical call in the original QQ Work."""

import gzip
import hashlib
import json
from pathlib import Path


def sha(data):
    return hashlib.sha256(data).hexdigest()


def projection(requests):
    def roles(messages, text):
        return sorted({m["role"] for m in messages if text in (m.get("content") or "")})

    return [
        {
            "question_roles": roles(request["messages"], "which city?"),
            "phone_answer_roles": roles(request["messages"], "Use Taipei"),
        }
        for request in requests
    ]


def original_fixture(a, b):
    source = "329ca7c487414d2f54ddd98ff4727446ed2446cd"
    receipts = {}
    observations = []
    sqlite = []
    for arm, path, patch in (
        ("A", a, b""),
        (
            "B",
            b,
            gzip.decompress(
                (Path(__file__).parent / "controls/b.patch.gz").read_bytes()
            ),
        ),
    ):
        run = json.loads((path / "run.json").read_bytes())
        if not (
            run["arm"] == arm
            and run["source_commit"] == source
            and run["patch_sha256"] == sha(patch)
            and run["long_works"] == 1
            and run["paid_calls"] == run["exit_code"] == 0
        ):
            raise ValueError("invalid original Work receipt in " + arm)
        sqlite.append((run["sqlite"], run["sqlite_library_sha256"]))
        requests = json.loads((path / "work-00-requests.json").read_bytes())
        wires = json.loads((path / "work-00-wire.json").read_bytes())
        if len(requests) != 6 or len(wires) != 6:
            raise ValueError("incomplete request sequence")
        for request, wire in zip(requests, wires, strict=True):
            raw = json.dumps(
                request, ensure_ascii=False, separators=(",", ":")
            ).encode()
            if (
                wire["sha256"] != sha(raw)
                or wire["bytes"] != len(raw)
                or wire["input"] != "long-0"
            ):
                raise ValueError("request differs from the physical recording")
        observations.append(projection(requests))
        receipts[arm] = {
            name: sha((path / name).read_bytes())
            for name in (
                "run.json",
                "junit.xml",
                "work-00-wire.json",
                "work-00-requests.json",
            )
        }
    if observations[0] != observations[1] or sqlite[0] != sqlite[1]:
        raise ValueError("independent baselines disagree")
    return {"source_commit": source, "receipts": receipts, "requests": observations[0]}


if __name__ == "__main__":
    import argparse

    p = argparse.ArgumentParser(description=__doc__)
    for name in ("a", "b", "out"):
        p.add_argument(name, type=Path)
    args = p.parse_args()
    result = original_fixture(args.a, args.b)
    with args.out.open("x", encoding="utf-8") as out:
        out.write(json.dumps(result, indent=2) + "\n")
