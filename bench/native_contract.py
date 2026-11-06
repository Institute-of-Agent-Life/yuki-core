"""Retain all text and policy fields; only compare locally allocated identifiers by their role."""

import re
from uuid import UUID

from bench.native_arrivals import pack, sha


def material(request):
    message = request["messages"][-1]
    if message["role"] != "user":
        raise ValueError("missing current user material")
    pattern = (
        r"(?P<context>[^\n]+\n资料：[^\n]+\n\n)\[(?P<clock>[^｜]+)｜(?P<name>[^|]+)\|person:(?P<person>[0-9a-f-]{36})\]"
        r"\n#(?P<event>[1-9][0-9]*)\|提及:(?P<mention>[^/]+)/@\{account:(?P<account>[0-9a-f-]{36})\}>(?P<text>.*)"
    )
    hit = re.fullmatch(pattern, message["content"], flags=re.DOTALL)
    if not hit:
        raise ValueError("missing or unexpected original group material")
    out = hit.groupdict()
    for field in ("person", "account"):
        UUID(out.pop(field))
    if int(out.pop("event")) < 1:
        raise ValueError("missing original event identity")
    return out


def request_fields(request):
    tools = {x["function"]["name"]: sha(pack(x)) for x in request["tools"]}
    if len(tools) != len(request["tools"]):
        raise ValueError("duplicate tool declaration")
    return {
        "material": material(request),
        "prefix": sha(pack(request["messages"][:-1])),
        "knobs": {k: v for k, v in request.items() if k not in ("messages", "tools")},
        "tools": tools,
    }


def differences(controls, observed):
    a, c = controls["tools"], observed["tools"]
    return {
        "missing": sorted(a.keys() - c.keys()),
        "extra": sorted(c.keys() - a.keys()),
        "changed": sorted(k for k in a.keys() & c.keys() if a[k] != c[k]),
        "request_knobs_equal": controls["knobs"] == observed["knobs"],
    }


def send_fields(sample):
    import json

    receipt = json.loads(sample["messages"][-1]["content"])
    if not isinstance(receipt, dict):
        raise ValueError("expected model-visible send receipt")
    return {
        k: sorted(v) if isinstance(v, dict) else type(v).__name__
        for k, v in receipt.items()
    }
