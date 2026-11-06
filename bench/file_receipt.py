"""Compare the complete two-part file result, normalizing only per-run identities."""

import json
from copy import deepcopy


def delivered(request):
    results = [
        json.loads(m["content"])
        for m in request["messages"]
        if m.get("tool_call_id") == "result"
    ]
    if len(results) != 1:
        raise ValueError("expected the original result tool call")
    return results[0]


def projection(receipt):
    result = deepcopy(receipt)
    parent = result["data"]
    file, caption = parent["parts"]
    if not (
        parent["target"] == file["target"] == caption["target"]
        and parent["target"]["id"]
        and file["presence_id"] == caption["presence_id"]
        and file["presence_id"]
        and file["original_conversation_id"] == caption["original_conversation_id"]
        and file["original_conversation_id"]
        and parent["artifact_id"] == file["artifact_id"]
        and file["artifact_id"]
        and len({part["operation_id"] for part in (parent, file, caption)}) == 3
        and file["original_event_id"] != caption["original_event_id"]
    ):
        raise ValueError("misbound multipart identity")
    for part, name in [(parent, "parent"), (file, "file"), (caption, "caption")]:
        if not isinstance(part["operation_id"], str) or not part["operation_id"]:
            raise ValueError("missing operation identity")
        part["operation_id"] = name
        part["target"]["id"] = "recipient"
    for part, name in [(file, "file"), (caption, "caption")]:
        if not (
            isinstance(part["event_id"], int)
            and part["event_id"] > 0
            and part["event_id"] == part["original_event_id"]
        ):
            raise ValueError("unbound delivery event")
        part.update(
            event_id=name,
            original_event_id=name,
            original_conversation_id="entry",
            presence_id="bot",
        )
    parent["artifact_id"] = file["artifact_id"] = "versioned-artifact"
    return result
