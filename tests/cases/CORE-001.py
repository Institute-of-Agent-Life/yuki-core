"""CORE-001: real loopback exchange, order mutation, and empty-case rejection."""

import json
import os
import socket

from tests.harness import LoopbackRecorder


def exchange(ctx, events):
    with LoopbackRecorder(ctx.clock_origin) as recorder:
        received = [recorder.post(event) for event in events]
        ctx.equal("HTTP responses are complete acknowledgements", received,
                  [{"accepted": event["event_id"]} for event in events])
        ctx.equal("record-before-ack barriers", recorder.barriers,
                  ["recorded:0", "ack:0", "recorded:1", "ack:1"])
        ctx.write("client-observations.json", [
            {"sent": event, "received": response}
            for event, response in zip(events, received, strict=True)
        ])
        return recorder.observations


def run(ctx):
    if ctx.fault == "empty":
        return {}
    fixture = json.loads((ctx.root / "tests/fixtures/CORE-001.json").read_text())
    events = fixture["events"]
    if ctx.fault == "swap":
        events = list(reversed(events))
    wire = exchange(ctx, events)
    ctx.write("platform-observations.json", wire)
    ctx.equal("event order", [row["request"]["event_id"] for row in wire],
              fixture["expected_order"])
    ctx.equal("deterministic replay", exchange(ctx, events), wire)
    for fault, failure in [("swap", "event order"), ("empty", "empty case")]:
        manifest = ctx.rejected_run("CORE-001", fault)
        ctx.equal(f"{fault} exits nonzero", manifest["exit_code"], 1)
        ctx.equal(f"{fault} records a failed manifest", manifest["status"], "failed")
        ctx.check(f"{fault} has the intended failure", failure in manifest["failure"])
    ctx.rejects("external IP is denied", PermissionError,
                lambda: socket.create_connection(("203.0.113.1", 443), timeout=1))
    ctx.rejects("external DNS is denied", PermissionError,
                lambda: socket.getaddrinfo("example.invalid", 443))
    ctx.check("no inherited API credentials", not any(
        "TOKEN" in key.upper() or "SECRET" in key.upper() or "API_KEY" in key.upper()
        for key in os.environ))
    return {
        "kind": "harness-selftest",
        "fixture": "tests/fixtures/CORE-001.json",
        "normal": {"requests": len(wire), "responses": len(wire)},
        "faults": {"swap": "rejected", "empty": "rejected"},
        "replay": "identical ordered request/response bodies and virtual timestamps",
        "network": "Python socket boundary allows loopback only",
        "real_platform": {"status": "not_run", "reason": "synthetic harness bootstrap"},
    }
