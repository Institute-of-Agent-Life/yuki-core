"""Native Responses search through the editable Main profile and its original physical ledger."""

import hashlib
import json
import os
import subprocess
from tests.harness.compact import Owner
from tests.peers.chat.recorder import Recorder


def exercise(ctx, profile="debug"):
    owner = Owner(ctx, "native-only-profile", 54, profile)
    receipts = {}
    with Recorder("/responses") as model:
        owner.config(model, '\n[search]\nmode="native"\nstrategy="native"\n')
        data = owner.svc.data
        settings = data / "yuki.toml"
        settings.write_text(
            'model_profiles_file="models.toml"\n' + settings.read_text()
        )
        profiles = (
            ctx.root / "config/model_profiles.deepseek-native.example.toml"
        ).read_text()
        profiles = profiles.replace(
            '"https://api.deepseek.com"',
            json.dumps(model.url.removesuffix("/responses")),
        )
        (data / "models.toml").write_text(profiles)
        env = {**os.environ, "DEEPSEEK_API_KEY": "loopback-only"}
        with owner.svc.running(environment=env):
            for label, counts, output in [
                (
                    "server-search",
                    {"tool_search": 2, "tool_fetch": 1},
                    [
                        {
                            "type": "web_search_call",
                            "id": "ws-1",
                            "status": "completed",
                            "action": {"type": "search", "query": "fixture"},
                        },
                        {
                            "type": "web_search_call",
                            "id": "ws-2",
                            "status": "completed",
                            "action": {
                                "type": "open_page",
                                "url": "https://example.org/reference",
                            },
                        },
                    ],
                ),
                ("answer-without-search", None, []),
                ("rate-limit", None, None),
                ("lost-response", None, None),
            ]:
                event = owner.human(label)
                wire, response = owner.wire(model)
                ctx.equal("selected Responses model", wire["model"], "deepseek-v4-pro")
                ctx.equal(
                    "one provider search declaration",
                    [t for t in wire["tools"] if t.get("type") == "web_search"],
                    [{"type": "web_search"}],
                )
                ctx.check(
                    "no external or bridge search in model wire",
                    not any(
                        t.get("name") in ("tavily_search", "web_search")
                        for t in wire["tools"]
                    ),
                )
                citations = (
                    [
                        {
                            "type": "url_citation",
                            "url": "https://example.org/reference",
                            "title": "Loopback reference",
                            "start_index": 0,
                            "end_index": 7,
                        }
                    ]
                    if counts
                    else []
                )
                if label == "lost-response":
                    response.put((b'{"status":', 64, 200))
                elif label == "rate-limit":
                    response.put(({"error": {"message": "fixture rate limit"}}, 0, 429))
                else:
                    usage = {"input_tokens": 12, "output_tokens": 2, "total_tokens": 14}
                    if counts is not None:
                        usage["server_tool_use"] = counts
                    message = {
                        "type": "message",
                        "role": "assistant",
                        "status": "completed",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "fixture answer https://example.org/prose",
                                "annotations": citations,
                            }
                        ],
                    }
                    response.put(
                        (
                            {
                                "status": "completed",
                                "output": output + [message],
                                "usage": usage,
                            },
                            0,
                        )
                    )
                execution = owner.rows(
                    "SELECT id FROM executions WHERE source=?", event
                )[0][0]
                row = owner.until(
                    lambda: owner.rows(
                        "SELECT t.id,t.state,t.status_code,t.usage_json FROM model_attempts t JOIN model_requests r ON r.id=t.logical JOIN activations a ON a.id=r.activation WHERE a.execution=? AND t.state IN ('response','rejected','unknown')",
                        execution,
                    )
                )
                ctx.equal("one recorded physical attempt for " + label, len(row), 1)
                attempt, state, status, raw = row[0]
                if label == "lost-response":
                    ctx.equal(
                        "incomplete body leaves original attempt unknown",
                        state,
                        "unknown",
                    )
                else:
                    ctx.equal(
                        "provider HTTP status for " + label,
                        status,
                        429 if label == "rate-limit" else 200,
                    )
                data_usage = json.loads(raw) if raw else None
                received = {
                    k: data_usage[k]
                    for k in ("tool_search", "tool_fetch")
                    if k in (data_usage or {})
                }
                ctx.equal(
                    "only reported search and fetch counts enter original ledger",
                    received,
                    counts or {},
                )
                events = []
                if output is not None:
                    owner.finished(event)
                    saved = owner.op("main", "inspect", execution)["result"]
                    events = saved["native_events"]
                    ctx.equal(
                        "original native operations survive decoding", events, output
                    )
                    ctx.equal(
                        "only server citation records count as sources",
                        saved["citations"],
                        citations,
                    )
                receipts[label] = {
                    "attempt": attempt,
                    "state": state,
                    "http_status": status,
                    "usage": data_usage,
                    "native_output_ids": [v["id"] for v in events],
                    "citations": [v["url"] for v in citations],
                    "request_sha256": hashlib.sha256(
                        json.dumps(wire, sort_keys=True, ensure_ascii=False).encode()
                    ).hexdigest(),
                }
            ctx.equal(
                "no Tavily plugin, tool handle or paid child",
                owner.rows(
                    "SELECT (SELECT count(*) FROM host_installations),(SELECT count(*) FROM host_calls),(SELECT count(*) FROM model_requests WHERE id LIKE 'external-search:%')"
                ),
                [(0, 0, 0)],
            )
            ctx.equal(
                "one physical model POST per scenario, no alternate route",
                len(model.header_shapes),
                len(receipts),
            )
        with owner.svc.running(environment=env):
            ctx.equal(
                "restart retains native physical attempts",
                owner.rows("SELECT count(*) FROM model_attempts"),
                [(len(receipts),)],
            )
            ctx.equal(
                "cold restart makes no extra model request",
                len(model.header_shapes),
                len(receipts),
            )
        (data / "models.toml").write_text(
            profiles.replace("deepseek-v4-pro", "deepseek-flash")
        )
        previous = len(model.header_shapes)

        failed = subprocess.run(
            [*owner.svc.argv, "run"],
            env=env,
            capture_output=True,
            text=True,
            timeout=10,
        )
        ctx.check(
            "unsupported DeepSeek Responses model refuses native search before network",
            failed.returncode != 0
            and "unsupported_profile_feature" in failed.stderr
            and len(model.header_shapes) == previous,
        )
    result = {
        "model": "deepseek-v4-pro",
        "protocol": "responses",
        "mode": "native",
        "strategy": "native",
        "provider": "deepseek",
        "validation": "loopback",
        "external_plugin_installed": False,
        "external_http": 0,
        "provider_http": len(receipts),
        "real_endpoint_tested": False,
        "cases": receipts,
    }
    ctx.write("native-only-search.json", result)
    return result
