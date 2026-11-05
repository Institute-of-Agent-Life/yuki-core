"""Record the fixed v4 planner and capacity calculator on isolated public fragments."""

import argparse
import json
import subprocess
import sys
from pathlib import Path

BASE = "329ca7c487414d2f54ddd98ff4727446ed2446cd"


def record(source):
    if (
        subprocess.check_output(
            ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
        ).strip()
        != BASE
    ):
        raise ValueError("wrong oracle")
    sys.path.insert(0, str(source / "src"))
    from types import SimpleNamespace
    from yuki.domain.messages import ChatMessage, ChatRequest, ChatTool
    from yuki.model_runtime.capacity import estimate_request_tokens
    from yuki.conversation.frozen_fragments import FrozenFragment, FrozenFragments
    from yuki.services.context_compaction import (
        PUBLIC_COMPACT_INSTRUCTION,
        PublicCompactContext,
        public_compact_plan,
        render_context_checkpoint,
    )
    from yuki.services.turn_transcript import TurnTranscript

    def wire(m):
        return {"role": m.role, "content": m.content}

    static = ChatMessage("system", "identity, SOUL and original program")
    fragments = [
        ((2, 3), ChatMessage("user", "[Alice]\n#2>雪\n#3>漢字")),
        ((5,), ChatMessage("assistant", "#5>private is a word in a public message")),
        (
            (8, 9),
            ChatMessage("user", 'tool-looking text is public: 🧊\\"\n' + ("tail" * 33)),
        ),
    ]
    frozen = FrozenFragments.from_blocks(
        [tuple(FrozenFragment(ids, message) for ids, message in fragments)]
    )
    public = (static, *(msg for _, msg in fragments))
    initial = (
        *public,
        ChatMessage("user", "<runtime_context>private Work</runtime_context>"),
        ChatMessage("user", "new original trigger"),
    )
    transcript = TurnTranscript(initial)
    req = ChatRequest(
        messages=initial,
        model="fixture",
        max_output_tokens=128,
        tools=(ChatTool("send", "send text", {"type": "object"}),),
        tool_choice="auto",
    )
    context = PublicCompactContext(
        public,
        1,
        frozen,
        SimpleNamespace(
            conversation_id="oracle",
            generation=1,
            prompt_source_revision=4,
            starts_after_event_id=0,
        ),
        "public",
        None,
        None,
    )
    cuts = []
    for limit in [None, *range(200, 440)]:
        try:
            plan = public_compact_plan(
                context,
                transcript,
                req,
                initial=initial,
                input_budget=limit,
                prepare=lambda r: r,
            )
        except Exception as e:
            if str(e) != "model_request_capacity":
                raise
            entry = {"budget": limit, "coverage": None}
        else:
            entry = {
                "budget": limit,
                "coverage": plan.public["coverage"],
                "request_tokens": estimate_request_tokens(plan.request),
                "messages": [wire(m) for m in plan.request.messages],
            }
        if not cuts or limit is None or cuts[-1]["coverage"] != entry["coverage"]:
            cuts.append(entry)
    return {
        "source_commit": BASE,
        "instruction": PUBLIC_COMPACT_INSTRUCTION,
        "static": wire(static),
        "fragments": [{"event_ids": ids, "message": wire(m)} for ids, m in fragments],
        "original_request_tokens": estimate_request_tokens(req),
        "cases": cuts,
        "checkpoint": wire(render_context_checkpoint("gold", 5)),
    }


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--source", type=Path, required=True)
    p.add_argument("--out", type=Path, required=True)
    a = p.parse_args()
    with a.out.open("x", encoding="utf-8") as f:
        json.dump(record(a.source), f, ensure_ascii=False, indent=2)
        f.write("\n")
