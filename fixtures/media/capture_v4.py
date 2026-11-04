"""Capture original attachment numbering and the declarations that consume it."""

import argparse
import json
from pathlib import Path
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
        raise ValueError("wrong oracle")
    sys.path.insert(0, str(repo / "src"))
    from yuki.conversation.media_index import (
        indexed_media_segments,
        observed_reply_media_segments,
    )
    from yuki.workspace.tools import workspace_tools

    current = [
        {"type": k, "data": {"file": "source-" + str(i)}}
        for i, k in enumerate(
            ["text", "at", "forward", "image", "face", "record", "custom", "file"]
        )
    ]
    reply = [
        {"type": k, "data": {"file": "reply-" + str(i)}}
        for i, k in enumerate(["text", "video", "record"])
    ]
    return {
        "source_commit": SHA,
        "current": current,
        "reply": reply,
        "direct_positions": list(indexed_media_segments(current)),
        "observed_reply_positions": list(observed_reply_media_segments(current, reply)),
        "tools": {
            t.name: {"description": t.description, "parameters": t.parameters}
            for t in workspace_tools()
        },
    }


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("source", type=Path)
    p.add_argument("out", type=Path)
    a = p.parse_args()
    a.out.write_text(json.dumps(capture(a.source), ensure_ascii=False, indent=2) + "\n")
