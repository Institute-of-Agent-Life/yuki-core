"""Index actual original CI test captures on a fixed isolated checkout."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

BASE = "329ca7c487414d2f54ddd98ff4727446ed2446cd"


def capture(source):
    if (
        subprocess.check_output(
            ["git", "-C", source, "rev-parse", "HEAD"], text=True
        ).strip()
        != BASE
    ):
        raise ValueError("unexpected oracle commit")
    sys.path.insert(0, str(source / "src"))
    from yuki.services.context_compaction import EXECUTION_COMPACT_INSTRUCTION
    from yuki.services.turn_transcript import tool_result_evidence
    from yuki.domain.messages import ChatMessage, ToolCall, ToolFunction

    out = {}
    for folder in sorted((source / "test-results/compact").glob("*")):
        for path in sorted(
            folder.rglob("evidence.json"), key=lambda p: p.stat().st_mtime, reverse=True
        ):
            raw = path.read_bytes()
            d = json.loads(raw)
            if (
                d["source_revision"] != BASE
                or d.get("source_patch_sha256")
                or not d.get("requests")
            ):
                continue
            if (
                "test_compact_delivery_e2e" not in d["nodeid"]
                and "test_native_execution_compact_can_hand_over" not in d["nodeid"]
            ):
                continue
            out[d["nodeid"]] = {
                "sha256": hashlib.sha256(raw).hexdigest(),
                "requests": len(d["requests"]),
                "wire_count": len(d.get("wire_requests", [])),
                "call_id": d.get("call_id"),
                "effect_key": d.get("original_effect_key"),
                "hard_capacity": d.get("hard_capacity"),
                "source_files": d["source_files"],
            }
            break
    if len(out) < 5:
        raise ValueError("original delivery and native compaction evidence missing")
    call = ToolCall(
        type="function",
        id="call-old",
        function=ToolFunction(name="send", arguments='{"text":"test"}'),
    )
    msg = tool_result_evidence(
        ChatMessage("assistant", "original", tool_calls=(call,)),
        {"call-old": '{"state":"unknown"}'},
        call_key=lambda _: "operation-old",
    )
    return {
        "source_commit": BASE,
        "execution_instruction": EXECUTION_COMPACT_INSTRUCTION,
        "portable": json.loads(msg.content),
        "original_cases": out,
    }


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("source", type=Path)
    p.add_argument("out", type=Path)
    a = p.parse_args()
    a.out.write_text(json.dumps(capture(a.source), ensure_ascii=False, indent=2) + "\n")
