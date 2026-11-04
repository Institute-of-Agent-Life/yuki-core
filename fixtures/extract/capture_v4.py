"""Capture structured extraction from the fixed, isolated v4 environment."""

from pathlib import Path
import argparse
import json
import subprocess
import sys

SHA = "329ca7c487414d2f54ddd98ff4727446ed2446cd"


def capture(repo):
    if (
        subprocess.check_output(
            ["git", "-C", repo, "rev-parse", "HEAD"], text=True
        ).strip()
        != SHA
    ):
        raise ValueError("wrong v4 checkout")
    sys.path.insert(0, str(repo / "src"))
    from yuki.config import Settings
    from yuki.memory.extraction import (
        BatchMemoryExtractionOutput,
        BATCH_EXTRACTION_PROMPT_VERSION,
        BATCH_EXTRACTION_SCHEMA_VERSION,
    )
    from yuki.memory.event_extractor import BATCH_EXTRACTION_INSTRUCTION

    defaults = {
        k: Settings.model_fields[k].default
        for k in [
            "memory_batch_seconds",
            "memory_batch_trigger_count",
            "memory_batch_max_events",
            "memory_batch_max_characters",
            "memory_batch_max_wait_seconds",
            "memory_batch_max_output_tokens",
        ]
    }
    return {
        "source_commit": SHA,
        "prompt_version": BATCH_EXTRACTION_PROMPT_VERSION,
        "schema_version": BATCH_EXTRACTION_SCHEMA_VERSION,
        "defaults": defaults,
        "instruction": BATCH_EXTRACTION_INSTRUCTION,
        "schema": BatchMemoryExtractionOutput.model_json_schema(),
    }


if __name__ == "__main__":
    a = argparse.ArgumentParser()
    a.add_argument("source", type=Path)
    a.add_argument("out", type=Path)
    args = a.parse_args()
    args.out.write_text(
        json.dumps(capture(args.source), ensure_ascii=False, indent=2) + "\n"
    )
