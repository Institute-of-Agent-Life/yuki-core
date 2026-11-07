"""Exercise EXT-029 over installed SDK stdio, with independent model and provider wires."""

import hashlib
import json
import sys
import uuid
from tests.harness.compact import Owner, call, reply
from tests.harness.json_file import read_json
from tests.harness.terminal_work import Peer
from tests.peers.chat.recorder import Recorder


def install(ctx, name, tool, peer, sdk, endpoint):
    args = [
        str(ctx.root / "tests/peers/terminal/consumer.py"),
        str(peer.folder),
        str(sdk),
    ]
    return f'''
[[plugins]]
id="{name}"
namespace="{name}"
program={json.dumps(sys.executable)}
args={json.dumps(args)}
grants=["tool.call","forget.owner","search.external"]
[plugins.external_search]
tool="{tool}"
provider="tavily"
purpose="web_search"
endpoint={json.dumps(endpoint)}
[[plugins.tools]]
name="{tool}"
description="Fixture search"
cacheable=false
parallel_read=false
code_direct=false
schema={{type="object",required=["query"],properties={{query={{type="string"}}}},additionalProperties=false}}
'''


def search_readiness(ctx, owner, model, provider, folder):
    permissions = {"search.external", "tool.call", "forget.owner"}

    def offered(prompt):
        event = owner.human(prompt)
        wire, response = owner.wire(model)
        return event, {t["function"]["name"] for t in wire["tools"]}, response

    def refused(event, response, label):
        reply(response, calls=[call("search", "tavily_search", {"query": label})])
        wire, response = owner.wire(model)
        result = json.loads(
            next(
                m["content"]
                for m in wire["messages"]
                if m.get("tool_call_id") == "search"
            )
        )
        ctx.equal(label, result["state"], "failed")
        reply(response, "")
        owner.finished(event)
        return result["result"]

    def approve(selected):
        state = owner.op("control", "read", "plugin.get", '{"plugin_id":"search"}')
        body = {
            "plugin_id": "search",
            "expected_revision": state["revision"],
            "request_id": str(uuid.uuid4()),
            "permissions": sorted(selected),
        }
        owner.op("control", "write", "plugin.approve", json.dumps(body))
        owner.op("host", "enable", "search", "true")

    owner.until(lambda: (folder / "pre-handshake.json").is_file())
    ev, tools, q = offered("before search handshake")
    ctx.check(
        "new view excludes offline search and retains the other peer",
        "tavily_search" not in tools and "other_search" in tools,
    )
    (folder / "withhold-handshake").unlink()
    owner.op("host", "restart", "search")
    refused(ev, q, "handshake cannot widen the original model view")

    ev, tools, q = offered("before disconnect")
    ctx.check("ready search is offered", "tavily_search" in tools)
    owner.op("host", "enable", "search", "false")
    error = refused(ev, q, "disconnect after pinning prevents dispatch")
    ctx.equal(
        "unavailable original peer has an explicit reason",
        error.get("error"),
        "original plugin instance unavailable",
    )
    owner.op("host", "enable", "search", "true")

    ev, tools, q = offered("before revocation")
    ctx.check("restored search is offered", "tavily_search" in tools)
    approve(permissions - {"forget.owner"})
    refused(ev, q, "frozen view never grants a newly revoked dispatch")
    for missing in ("forget.owner", "tool.call", "search.external"):
        if missing != "forget.owner":
            approve(permissions - {missing})
        ev, tools, q = offered("missing " + missing)
        ctx.check(
            "partial approval omits search without " + missing,
            "tavily_search" not in tools and "other_search" in tools,
        )
        reply(q, "")
        owner.finished(ev)
    ctx.equal(
        "unavailable search makes no provider requests", len(provider.header_shapes), 0
    )
    ctx.equal(
        "unavailable search makes no paid attempts",
        owner.rows(
            "SELECT count(*) FROM model_attempts WHERE logical LIKE 'external-search:%'"
        ),
        [(0,)],
    )
    approve(permissions)
    return {
        "new_views_without_all_grants": len(permissions),
        "physical_http": 0,
        "frozen_view_extended_after_handshake": False,
        "original_disconnect": error["error"],
    }


def exercise(ctx, sdk, profile="debug"):
    owner = Owner(ctx, "search-owner", 54, profile)
    plugin = Peer(owner, ctx.working / "external-search-peer")
    foreign = Peer(owner, ctx.working / "other-search-peer")
    receipts = {}
    seen = set()
    paid = {"provider": "tavily", "purpose": "web_search"}
    with (
        Recorder() as model,
        Recorder("/v1/responses") as native,
        Recorder("/search") as provider,
    ):
        cfg = f"""
[search]
mode="both"
strategy="bridge"
connection_id="native-fixture"
credential_namespace="native-fixture"
[search.connection]
protocol="openai_responses"
endpoint={json.dumps(native.url)}
model="fixture"
name="Native"
merge_ms=0
max_output=128
root_slots=8
max_input_units=4000000
native_web_search=true
"""
        cfg += install(ctx, "search", "tavily_search", plugin, sdk, provider.url)
        cfg += install(ctx, "other", "other_search", foreign, sdk, provider.url)
        owner.config(model, cfg)
        conf = owner.svc.data / "yuki.toml"

        def ready(p, old=None):
            owner.until(
                lambda: (p.folder / "ready.json").is_file()
                and read_json(p.folder / "ready.json")["pid"] != old
            )

        def tool(query):
            ev = owner.human(query)
            wire, q = owner.wire(model)
            ctx.check(
                "deployment-selected tool in original model wire",
                any(t["function"]["name"] == "tavily_search" for t in wire["tools"]),
            )
            reply(q, calls=[call("external", "tavily_search", {"query": query})])
            path = owner.until(
                lambda: next(
                    (
                        p
                        for p in plugin.folder.glob("call-*.json")
                        if p.name not in seen
                    ),
                    None,
                )
            )
            seen.add(path.name)
            return ev, read_json(path)

        def finish_turn(ev, frame, ok):
            plugin.post(
                {"id": frame["id"], "reply": {"ok": ok, "data": {"fixture": True}}}
            )
            _, q = owner.wire(model)
            reply(q, "")
            owner.finished(ev)

        # An installed peer can be unavailable while the rest of Main is ready.
        hold = plugin.folder / "withhold-handshake"
        hold.write_text("offline", encoding="utf-8")
        with owner.svc.running():
            ready(foreign)
            receipts["readiness"] = search_readiness(
                ctx, owner, model, provider, plugin.folder
            )
            ev = owner.human("native only")
            _, q = owner.wire(model)
            reply(q, calls=[call("native", "web_search", {"query": "no fallback"})])
            _, nq = native.receive()
            nq.put(({"error": "unavailable"}, 0, 503))
            wire, q = owner.wire(model)
            ctx.check(
                "native refusal reaches the original model",
                any(m.get("tool_call_id") == "native" for m in wire["messages"]),
            )
            reply(q, "")
            owner.finished(ev)
            ctx.equal(
                "native failure sends zero external HTTP requests",
                len(provider.header_shapes),
                0,
            )
            receipts["native_failure"] = {
                "native_http": len(native.header_shapes),
                "external_http": 0,
            }

            for scenario, code, missing, data in [
                ("success", 201, 0, {"usage": {"credits": 1, "billed_microusd": 1300}}),
                ("rate_limit", 429, 0, {"error": "limited"}),
                ("lost_response", 200, 80, {}),
                ("disconnect", 200, 0, {"usage": {"credits": 1}}),
            ]:
                ev, frame = tool(scenario)
                h = {"handle": frame["params"]["handle"]}
                args = {**h, **paid}
                foreign.rpc("search.begin", args, False)
                plugin.rpc("search.begin", {**args, "purpose": "other"}, False)
                plugin.rpc("search.begin", {**args, "handle": "guessed"}, False)
                before = owner.rows(
                    "SELECT b.root,b.reserved_requests FROM host_calls c JOIN budget_members m ON c.execution=m.execution JOIN root_budgets b ON b.root=m.root WHERE handle=?",
                    h["handle"],
                )[0]
                a = plugin.rpc("search.begin", args)
                first = plugin.seq
                ctx.equal(
                    "first permit bound to original root and deployment",
                    (a["root"], a["endpoint"], a["dispatch_now"], a["state"]),
                    (before[0], provider.url, True, "dispatched"),
                )
                ctx.equal(
                    "duplicate logical admission only reads original attempt",
                    plugin.rpc("search.begin", args),
                    {**a, "dispatch_now": False},
                )
                ctx.equal(
                    "one slot charged before a physical write",
                    owner.rows(
                        "SELECT reserved_requests FROM root_budgets WHERE root=?",
                        a["root"],
                    ),
                    [(before[1] + 1,)],
                )
                foreign.rpc("search.status", h, False)
                plugin.post({"dispatch_search": first})
                raw, answer = provider.receive()
                ctx.equal(
                    "peer sends original query once",
                    json.loads(raw),
                    {"query": scenario},
                )
                answer.put((data, missing, code))
                observed = plugin.folder / ("http-" + a["attempt"] + ".json")
                owner.until(observed.is_file)
                facts = read_json(observed)
                if scenario == "disconnect":
                    # Leave the Host with a paid attempt and no reported outcome.
                    old_pid = read_json(plugin.folder / "ready.json")["pid"]
                    break
                record = {**h, "attempt": a["attempt"], **facts}
                plugin.rpc("search.finish", {**record, "attempt": "other"}, False)
                receipt = plugin.rpc("search.finish", record)
                ctx.equal(
                    "physical receipt is idempotent",
                    plugin.rpc("search.finish", record),
                    receipt,
                )
                other = (
                    {"outcome": "unknown"}
                    if not missing
                    else {
                        "outcome": "response",
                        "status": 200,
                        "response_sha256": "0" * 64,
                    }
                )
                plugin.rpc(
                    "search.finish", {**h, "attempt": a["attempt"], **other}, False
                )
                ctx.equal(
                    "ledger exposes only observed cost and response",
                    (
                        receipt["state"],
                        receipt["status"],
                        receipt["usage"],
                        receipt["response_sha256"],
                    ),
                    (
                        "unknown"
                        if missing
                        else "rejected"
                        if code == 429
                        else "response",
                        None if missing else code,
                        data.get("usage"),
                        None
                        if missing
                        else hashlib.sha256(json.dumps(data).encode()).hexdigest(),
                    ),
                )
                finish_turn(ev, frame, True)
                ctx.equal(
                    "outer reply cannot confirm a missing or rejected provider response",
                    owner.rows(
                        "SELECT state FROM host_calls WHERE handle=?", h["handle"]
                    ),
                    [
                        (
                            "unknown"
                            if missing
                            else "failed"
                            if code == 429
                            else "confirmed",
                        )
                    ],
                )
                ctx.equal(
                    "settled attempt cannot be dispatched again",
                    plugin.rpc("search.begin", args),
                    receipt,
                )
                ctx.equal(
                    "root billing includes the same physical attempt",
                    [
                        item["usage"]
                        for item in owner.op("model", "usage", a["root"])[
                            "physical_receipts"
                        ]
                        if item["attempt"] == a["attempt"]
                    ],
                    [data.get("usage")],
                )
                ctx.equal(
                    "charged child reaches its durable final state",
                    owner.rows(
                        "SELECT state FROM executions WHERE origin=?",
                        receipt["logical"],
                    ),
                    [("held" if missing else "completed",)],
                )
                receipts[scenario] = receipt

        with owner.svc.running():
            ready(plugin, old_pid)
            recovered = plugin.rpc("search.status", h)
            ctx.equal(
                "cold restart reads unknown without dispatch",
                (
                    recovered["state"],
                    recovered["attempt"],
                    recovered["dispatch_now"],
                    recovered["usage"],
                ),
                ("unknown", a["attempt"], False, None),
            )
            ctx.equal(
                "duplicate begin after restart cannot send again",
                plugin.rpc("search.begin", args),
                recovered,
            )
            ctx.equal(
                "observed provider requests after cold restart",
                len(provider.header_shapes),
                4,
            )
            ctx.equal(
                "disconnected paid child is held",
                owner.rows(
                    "SELECT state FROM executions WHERE origin=?", recovered["logical"]
                ),
                [("held",)],
            )
            ctx.equal(
                "generic tool reader returns no provider payload",
                plugin.rpc("tool.status", h),
                {"handle": h["handle"], "state": "unknown", "result": None},
            )
            # A replacement deployment with the same manifest must not inherit a paid receipt.
            import sqlite3
            from contextlib import closing

            with closing(sqlite3.connect(owner.svc.data / "core.sqlite")) as db:
                (original,) = db.execute(
                    "SELECT value FROM core_meta WHERE key='plugin-approval:search'"
                ).fetchone()
                db.execute(
                    "UPDATE core_meta SET value=json_set(value,'$.deployment','other') WHERE key='plugin-approval:search'"
                )
                db.commit()
                for method in ("search.status", "tool.status"):
                    plugin.rpc(method, h, False)
                plugin.rpc("search.begin", args, False)
                db.execute(
                    "UPDATE core_meta SET value=? WHERE key='plugin-approval:search'",
                    (original,),
                )
                db.commit()
            receipts["disconnect"] = recovered
        # A fresh execution after a mode change cannot select the installed paid tool.
        conf.write_text(conf.read_text().replace('mode="both"', 'mode="native"'))
        with owner.svc.running():
            ev = owner.human("deployment removed external search")
            wire, q = owner.wire(model)
            ctx.check(
                "native-only deployment excludes paid external tools",
                not any(
                    t["function"]["name"] in ("tavily_search", "other_search")
                    for t in wire["tools"]
                ),
            )
            reply(q, "")
            owner.finished(ev)
        conf.write_text(
            conf.read_text()
            .replace('mode="native"', 'mode="tavily"')
            .replace("root_slots=8", "root_slots=1")
        )
        with owner.svc.running():
            owner.until(
                lambda: owner.rows(
                    "SELECT online FROM host_installations WHERE plugin='search'"
                )
                == [(1,)]
            )
            ev, frame = tool("exhausted original root")
            h = {"handle": frame["params"]["handle"]}
            plugin.rpc("search.begin", {**h, **paid}, False)
            plugin.rpc("search.status", h, False)
            ctx.equal(
                "budget denial rolls back the entire provider admission",
                owner.rows(
                    "SELECT count(*) FROM main_origins WHERE ref=?",
                    "external-search:" + h["handle"],
                ),
                [(0,)],
            )
            ctx.equal(
                "exhaustion creates no physical request", len(provider.header_shapes), 4
            )
            plugin.post(
                {
                    "id": frame["id"],
                    "reply": {"ok": False, "data": {"reason": "denied"}},
                }
            )
            owner.until(
                lambda: owner.rows(
                    "SELECT state FROM host_calls WHERE handle=?", h["handle"]
                )
                == [("failed",)]
            )
            receipts["exhausted"] = {"reserved": 0, "new_http": 0}
        report = {
            "methods": ["search.begin", "search.finish", "search.status"],
            "physical_http": len(provider.header_shapes),
            "paid_calls": 0,
            "cases": receipts,
        }
        ctx.write("external-search-ledger.json", report)
        return report
