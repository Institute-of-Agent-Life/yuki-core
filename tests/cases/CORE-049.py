"""Four evidence layers, scoped server-tool receipts and a single physical probe budget."""

import copy
import datetime as dt
import importlib.util
import json
import shutil
import threading
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler


def run(ctx):
    spec = importlib.util.spec_from_file_location(
        "probe_profile", ctx.root / "ci/probe-profile.py"
    )
    probe = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(probe)
    data = ctx.root / "baseline/profile-evidence"
    fixture = json.loads((data / "wire.json").read_bytes())
    grid = probe.grid()
    rows = grid["cases"]["H23"]
    ctx.equal(
        "four original wire replays",
        [(r["wire"], r["offline_replay"]["state"]) for r in rows],
        [
            ("chat", "ignored"),
            ("responses", "native_sources"),
            ("messages", "native_sources"),
            ("gemini", "native_sources"),
        ],
    )
    ctx.check(
        "official docs and offline tests are distinct from real endpoints",
        all(
            r["official_document"]["reviewed"]
            and r["real_endpoint"]["state"] == "not_authorized"
            for r in rows
        ),
    )
    schema = fixture["cases"]["chat_structured"]["payloads"][0]["response_format"][
        "json_schema"
    ]
    ctx.check(
        "fixed adapter emits the requested structured schema",
        schema["strict"] and schema["name"] == "answer",
    )
    for value, state in [
        ("{}", "invalid_structured_result"),
        (
            '{"results":[{"title":"IANA","url":"https://example.org/"}]}',
            "structured_result",
        ),
    ]:
        result = probe.classify(
            "chat",
            {"choices": [{"finish_reason": "stop", "message": {"content": value}}]},
            "structured_search",
            200,
        )
        ctx.equal("JSON search output: " + value, result["state"], state)
    msg = copy.deepcopy(fixture["cases"]["messages"]["attempts"][0]["response"])
    for block in msg["content"]:
        if block["type"] == "web_search_tool_result":
            block["tool_use_id"] = "unrelated"
    ctx.equal(
        "unpaired Messages result ignored",
        probe.classify("messages", msg, "native_search", 200)["state"],
        "ignored",
    )
    empty = {
        "output": [
            {
                "id": "s1",
                "status": "completed",
                "type": "web_search_call",
                "action": {"sources": []},
            }
        ]
    }
    ctx.equal(
        "completed search with no sources",
        probe.classify("responses", empty, "native_search", 200)["state"],
        "no_source",
    )
    ctx.equal(
        "HTTP rejection separate",
        probe.classify("responses", {}, "native_search", 400)["state"],
        "rejected",
    )

    posts = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            posts.append(
                {
                    "path": self.path,
                    "body": json.loads(
                        self.rfile.read(int(self.headers["Content-Length"]))
                    ),
                }
            )
            body = json.dumps({"output": [{"type": "message", "content": []}]}).encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    class Local(ThreadingHTTPServer):
        def server_bind(self):
            from socketserver import TCPServer

            TCPServer.server_bind(self)
            self.server_name, self.server_port = self.server_address[:2]

    with Local(("127.0.0.1", 0), Handler) as server:
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        try:
            profile = {
                "endpoint": f"http://127.0.0.1:{server.server_port}/responses",
                "region": "fixture",
                "model": "synthetic",
                "wire": "responses",
                "thinking": "default",
                "task": "native_search",
            }
            proxy = {
                **profile,
                "endpoint": profile["endpoint"].replace(
                    "/responses", "/other/responses"
                ),
            }
            auth = ctx.working / "local-authorization.json"
            doc = {
                "authorized": True,
                "authorization_id": "00000000-0000-4000-8000-000000000049",
                "profile_key": probe.identity(profile),
                "task": profile["task"],
                "max_usd": "0.01",
                "endpoint_hard_cap_usd": "0.01",
                "endpoint_cap_receipt": "loopback-fixture",
                "expires_at": (
                    dt.datetime.now(dt.timezone.utc) + dt.timedelta(hours=1)
                ).isoformat(),
            }
            auth.write_text(json.dumps(doc))

            def call(name, *, target=profile, **kwargs):
                return probe.run_probe(
                    target,
                    ctx.working / name,
                    reservation_dir=ctx.working / "ledger",
                    **kwargs,
                )

            call("no-opt-in", authorization=auth, key="synthetic-key")
            call("no-key", live=True, authorization=auth)
            call("no-grant", live=True, key="synthetic-key")
            different = call(
                "new-proxy",
                target=proxy,
                live=True,
                authorization=auth,
                key="synthetic-key",
            )
            ctx.equal(
                "no opt-in, key, grant, or changed proxy sends zero HTTP", len(posts), 0
            )
            ctx.check(
                "same model proxy has distinct identity",
                different["profile_key"] != probe.identity(profile),
            )
            result = call(
                "actual-loopback", live=True, authorization=auth, key="synthetic-key"
            )
            ctx.equal("one actual localhost POST", len(posts), 1)
            ctx.equal(
                "200 with no server event never establishes search",
                result["state"],
                "ignored",
            )
            ctx.check(
                "missing usage and amount stay unknown",
                result["usage"] is None and result["cost_usd"] is None,
            )
            auth_copy = ctx.working / "renamed.json"
            shutil.copyfile(auth, auth_copy)
            again = call(
                "retry", live=True, authorization=auth_copy, key="synthetic-key"
            )
            ctx.equal(
                "reservation prevents another attempt",
                (again["state"], len(posts)),
                ("already_reserved", 1),
            )
        finally:
            server.shutdown()
            worker.join(timeout=5)
    ctx.write("endpoint-evidence-grid.json", grid)
    return {
        "H23": "verified offline",
        "paid_calls": 0,
        "wire_sha256": grid["wire_sha256"],
        "real_endpoints_verified": [],
        "loopback": {
            "attempts": len(posts),
            "status": result["state"],
            "unknown_cost_usd": result["cost_usd"],
        },
    }
