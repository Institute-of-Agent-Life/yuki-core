"""One root reservation per logical request. HTTP observations, late receipts and bounded retries."""

import http.client
import json
import threading
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from tests.harness.service import Service, previous_schema


def run(ctx):
    owner = Service(ctx, ctx.working / "data")
    previous_schema(ctx, owner.data, 5)
    ctx.check("schema 5 upgrades", owner.command("migrate")["schema_version"] >= 6)

    def op(*args, ok=True):
        return owner.command("operator", *map(str, args), accepted=ok)

    def request_body(logical):
        return json.dumps(
            {"messages": [{"role": "user", "content": logical}]}, separators=(",", ":")
        )

    def model(*args, ok=True):
        if args[0] == "reserve" and "--request" not in args:
            args = (*args, "--request", request_body(args[2]))
        return op("model", *args, ok=ok)

    actor = "00000000-0000-4000-8000-000000000011"
    with owner.running():
        ctx.write("sqlite-build.json", op("database-info"))
        op("person", "create", actor)
        h = op(
            "account",
            "open",
            "qq",
            "fixture",
            "human",
            1,
            "--person",
            actor,
            "--start",
            1,
        )
        bot = op("account", "open", "qq", "fixture", "bot", 1, "--start", 1)

        def execution(n):
            frame = {
                "source": "source-" + str(n),
                "native_id": str(n),
                "account": h["account"],
                "bot_stage": bot["assignment"],
                "kind": "message",
                "segments": [{"type": "text", "text": "root budget"}],
                "occurred": 2,
                "observed": 3,
            }
            e = op("ingress", "record", json.dumps(frame), "--accept")["event"]
            return op("ingress", "claim", e)["execution"]

        root, left, right, other, late = map(execution, range(5))
        model("root", root, 1, 200)
        model("child", left, root)
        model("child", right, root)
        for run_id in (left, right):
            model("activate", run_id, "act-" + run_id, "fixture-profile")
        model("root", other, 3, 600)
        model("activate", other, "restartable", "synthetic-safe-profile")
        model("root", late, 3, 200)
        model("activate", late, "late-rejection", "fixture-retry-profile")
        model("reserve", "late-rejection", "late-req", 10, 20, "--safe-rate-limit-once")
        barrier = threading.Barrier(2)

        def compete(run_id):
            barrier.wait(timeout=5)
            # The two CLI processes race the real writer and must independently observe their outcomes.
            import subprocess

            args = [
                *owner.argv,
                "operator",
                "model",
                "reserve",
                "act-" + run_id,
                "req-" + run_id,
                "10",
                "90",
            ]
            args.extend(["--request", request_body("req-" + run_id)])
            p = subprocess.run(args, capture_output=True, text=True, timeout=10)
            return run_id, p.returncode

        with ThreadPoolExecutor(max_workers=2) as pool:
            contenders = list(pool.map(compete, (left, right)))
        ctx.equal(
            "one of two child requests wins the last root slot",
            sorted(code for _, code in contenders),
            [0, 1],
        )
        won = next(run_id for run_id, code in contenders if code == 0)
        logical = "req-" + won
        model("attach-work", won, "original-work")
        model(
            "reserve",
            "act-" + won,
            logical,
            10,
            90,
            "--request",
            request_body("modified"),
            ok=False,
        )
        model("reserve", "act-" + won, logical, 10, 90)
        adopted = model("inspect", logical)
        ctx.equal(
            "original request predates Work adoption",
            (adopted["original_work"], adopted["work"], adopted["execution"]),
            (None, "original-work", won),
        )
        ctx.equal(
            "late work adoption does not reset the root",
            model("usage", root)["reserved_requests"],
            1,
        )
    posts = []
    wire = {}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            attempt = self.headers["X-Test-Attempt"]
            wire[attempt] = self.rfile.read(
                int(self.headers["Content-Length"])
            ).decode()
            posts.append(attempt)
            body = {"attempt": attempt, "answer": "synthetic", "tools": []}
            self.send_response(429 if attempt in ("safe-1", "late-429") else 200)
            self.end_headers()
            self.wfile.write(json.dumps(body).encode())

    class Local(ThreadingHTTPServer):
        def server_bind(self):
            from socketserver import TCPServer

            TCPServer.server_bind(self)
            self.server_name, self.server_port = self.server_address[:2]

    with Local(("127.0.0.1", 0), Handler) as peer:
        worker = threading.Thread(target=peer.serve_forever, daemon=True)
        worker.start()

        def send(logical, attempt):
            permit = model("dispatch", logical, attempt)
            ctx.check("physical permit issued " + attempt, permit["dispatch_now"])
            with closing(
                http.client.HTTPConnection("127.0.0.1", peer.server_port, timeout=4)
            ) as client:
                client.request(
                    "POST",
                    "/model",
                    permit["request_raw"],
                    headers={
                        "X-Test-Attempt": attempt,
                        "Content-Type": "application/json",
                    },
                )
                response = client.getresponse()
                return response.status, json.loads(response.read())

        try:
            with owner.running():
                _, lost = send(logical, "lost")
                _, late_body = send("late-req", "late-429")
            with owner.running():
                ctx.check(
                    "same attempt cannot dispatch twice",
                    not model("dispatch", logical, "lost")["dispatch_now"],
                )
                model("dispatch", logical, "blind-retry", ok=False)
                ctx.equal(
                    "cold owner classifies a dispatched but missing response as unknown",
                    model("inspect", logical)["state"],
                    "unknown",
                )
                model(
                    "rejected",
                    "late-429",
                    429,
                    "--unbilled-before-model",
                    json.dumps(late_body),
                    "null",
                )
                model("dispatch", "late-req", "late-retry", ok=False)
                model("activate", late, "later-segment", "fixture-retry-profile")
                model("reserve", "later-segment", "renamed-unknown", 10, 20, ok=False)
                ctx.equal(
                    "late rejection retained under original physical ID",
                    model("inspect", "late-req")["response"],
                    late_body,
                )
                model("unknown", "lost")
                unknown = model("usage", root)
                ctx.equal(
                    "unreturned attempt has no billed usage",
                    (
                        unknown["attempts"],
                        unknown["usage_attempts"],
                        unknown["cost_usd"],
                    ),
                    (1, 0, None),
                )
                model("cancel", won)
                model("response", "lost", json.dumps(lost), "null", "[]")
                ctx.equal(
                    "late response after cancel is reconciled",
                    model("inspect", logical)["state"],
                    "response_held",
                )
                ctx.equal("cancel never grants a new attempt", len(posts), 2)
                model(
                    "reserve", "restartable", "safe", 12, 80, "--safe-rate-limit-once"
                )
                status, rejection = send("safe", "safe-1")
                ctx.equal(
                    "actual bounded retry starts with complete mock 429", status, 429
                )
                model(
                    "rejected",
                    "safe-1",
                    "429",
                    "--unbilled-before-model",
                    json.dumps(rejection),
                    json.dumps({"output": 0, "total": 0}),
                )
                _, paid = send("safe", "safe-2")
                tools = [
                    {
                        "call_id": "provider-call-1",
                        "action": "put",
                        "target": "answer",
                        "payload": {"answer": "result"},
                    }
                ]
                model(
                    "response",
                    "safe-2",
                    json.dumps(paid),
                    json.dumps(
                        {
                            "input": 10,
                            "output": 3,
                            "total": 13,
                            "reasoning": 1,
                            "cache": 4,
                        }
                    ),
                    json.dumps(tools),
                )
            for restart in range(3):
                with owner.running():
                    snapshot = model("inspect", "safe")
                    ctx.equal(
                        "original provider body survived cold restart",
                        snapshot["response"],
                        paid,
                    )
                    model("dispatch", "safe", "again", ok=False)
                    ctx.equal(
                        "complete raw body is available without re-encoding",
                        snapshot["response_raw"],
                        json.dumps(paid),
                    )
                    ctx.equal(
                        "stored batch exposes original provider call",
                        snapshot["tools"][0]["call_id"],
                        "provider-call-1",
                    )
                    if restart == 0:
                        ctx.equal(
                            "first cold owner leaves original paid batch unpaired",
                            snapshot["state"],
                            "response",
                        )
                        continue
                    pair = model("pair", "safe", 0)
                    ctx.equal(
                        "paired original effect", pair["call_id"], "provider-call-1"
                    )
                    report = model("usage", other)
                    ctx.equal(
                        "one reservation, two physical attempts",
                        (report["reserved_requests"], report["attempts"]),
                        (1, 2),
                    )
                    ctx.equal(
                        "input and cache coverage incomplete",
                        (
                            report["fields"]["input"]["total"],
                            report["fields"]["cache"]["total"],
                        ),
                        (None, None),
                    )
                    ctx.equal(
                        "known output denominator includes both attempts",
                        report["fields"]["output"],
                        {"known_subtotal": 3, "reported": 2, "attempts": 2, "total": 3},
                    )
                    ctx.equal(
                        "partial reasoning retains its denominator",
                        report["fields"]["reasoning"]["reported"],
                        1,
                    )
                    ctx.equal(
                        "unpriced attempts remain unknown", report["cost_usd"], None
                    )
        finally:
            peer.shutdown()
            worker.join(timeout=5)
    ctx.equal("real receiver calls", posts, ["lost", "late-429", "safe-1", "safe-2"])
    ctx.equal(
        "bounded retries use the original exact request bytes",
        (wire["safe-1"], wire["safe-2"]),
        (request_body("safe"), request_body("safe")),
    )
    h06 = {
        "contenders": contenders,
        "unknown": unknown,
        "original_response": paid,
        "posts": posts,
        "paired": pair,
    }
    ctx.write("paid-boundaries.json", {"cases": {"H06": h06}})
    ctx.write("usage-denominator.json", {"cases": {"H22": report}})
    return {"H06": h06, "H22": report}
