"""Observe original immutable results, dispatch permits, three child ordinals and loss of ACK."""

import http.client
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from contextlib import closing
from tests.harness.service import Service, previous_schema
from tests.harness.crash import kill_writer


def run(ctx):
    owner = Service(ctx, ctx.working / "data")
    previous_schema(ctx, owner.data, 4)
    ctx.check("schema 4 upgrades", owner.command("migrate")["schema_version"] >= 5)
    a, b = [f"00000000-0000-4000-8000-{n:012x}" for n in (1, 2)]

    def op(*args, **kw):
        return owner.command("operator", *args, **kw)

    def effect(action, key, *args, reader=None, ok=True):
        return op(
            "effect",
            action,
            run_id,
            key,
            *([reader] if reader else []),
            *args,
            accepted=ok,
        )

    def intent(key, action, target, payload, *extra, ok=True):
        return effect(
            "intent", key, action, target, json.dumps(payload), *extra, reader=a, ok=ok
        )

    with owner.running():
        ctx.write("sqlite-build.json", op("database-info"))
        for actor in (a, b):
            op("person", "create", actor)
        human = op(
            "account",
            "open",
            "qq",
            "local",
            "200",
            str(1),
            "--person",
            a,
            "--start",
            "1",
        )
        bot = op("account", "open", "qq", "local", "900", "1", "--start", "1")
        f = {
            "source": "effect-fixture",
            "native_id": "111",
            "account": human["account"],
            "bot_stage": bot["assignment"],
            "occurred": 2,
            "observed": 3,
            "kind": "message",
            "segments": [{"type": "text", "text": "effect"}],
        }
        event = op("ingress", "record", json.dumps(f), "--accept")["event"]
        run_id = op("ingress", "claim", str(event))["execution"]
        intent("R1", "put", "note", {"z": 1, "a": [1, 2]})
        first = effect("local", "R1", reader=a)
        intent("R2", "put", "note", {"z": 2})
        second = effect("local", "R2", reader=a)
        intent("R1", "put", "note", {"a": [1, 2], "z": 1})
        ctx.equal(
            "R1 replay yields exact original result",
            effect("local", "R1", reader=a),
            first,
        )
        ctx.equal(
            "later R2 is still current",
            op("effect", "value", run_id, a, "note"),
            second["result"],
        )
        intent("R1", "put", "different", {"z": 1, "a": [1, 2]}, ok=False)
        intent("R1", "put", "note", {"a": [2, 1], "z": 1}, ok=False)
        intent("R3", "put", "note", {"z": 2})
        third = effect("local", "R3", reader=a)
        ctx.equal(
            "equal parameters, new real intent gets its own effect",
            third["result"]["revision"],
            3,
        )
        intent("read", "get", "note", {})
        read = effect("local", "read", reader=a)
        ctx.equal("read-after-write sees R3", read["result"], third["result"])
        intent("batch", "external", "gateway", {})
        for i in range(3):
            intent(
                "part" + str(i),
                "external",
                "qq-peer",
                {"part": i},
                "--parent",
                "batch",
                "--ordinal",
                str(i),
            )
        intent(
            "other",
            "external",
            "qq-peer",
            {},
            "--parent",
            "batch",
            "--ordinal",
            "0",
            ok=False,
        )
        effect("read", "R1", reader=b, ok=False)
        for phase in ("before", "after"):
            intent("local-" + phase, "put", "durability", {"phase": phase})
    ambiguous = []
    for phase, expected in (("before", "intent"), ("after", "confirmed")):
        key = "local-" + phase
        kill_writer(
            ctx,
            owner.data,
            "effect_crash_child",
            phase,
            {
                "YUKI_EFFECT_TEST_ACTION": json.dumps(
                    {"method": "local", "execution": run_id, "id": key, "actor": a}
                )
            },
        )
        with owner.running():
            seen = effect("read", key, reader=a)
            ctx.equal(
                "query original effect after unknown " + phase, seen["state"], expected
            )
            result = effect("local", key, reader=a)
            ctx.equal(
                "no duplicate write after lost local reply",
                result["result"]["revision"],
                1 if phase == "before" else 2,
            )
            ambiguous.append({"phase": phase, "observed": seen, "replayed": result})
    deliveries = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            deliveries.append(
                json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            )
            self.send_response(204)
            self.end_headers()

    class Local(ThreadingHTTPServer):
        def server_bind(self):
            from socketserver import TCPServer

            TCPServer.server_bind(self)
            self.server_name, self.server_port = self.server_address[:2]

    with Local(("127.0.0.1", 0), Handler) as gateway:
        peer = threading.Thread(target=gateway.serve_forever, daemon=True)
        peer.start()
        try:
            with owner.running():
                for ordinal in range(3):
                    key = "part" + str(ordinal)
                    permit = effect("dispatch", key, reader=a)
                    ctx.check("exclusive pre-I/O permit " + key, permit["dispatch_now"])
                    with closing(
                        http.client.HTTPConnection(
                            "127.0.0.1", gateway.server_port, timeout=3
                        )
                    ) as client:
                        client.request(
                            "POST",
                            "/",
                            body=json.dumps({"execution": run_id, "key": key}),
                        )
                        ctx.equal(
                            "physical fake gateway received " + key,
                            client.getresponse().status,
                            204,
                        )
                    if ordinal != 2:
                        effect(
                            "settle", key, "confirmed", json.dumps({"reference": key})
                        )
                # The last real peer effect crossed the boundary; lose its response locally.
            with owner.running():
                before = effect("read", "part2", reader=a)
                ctx.equal(
                    "recovery sees original dispatched key",
                    before["state"],
                    "dispatched",
                )
                ctx.check(
                    "cold retry cannot obtain another dispatch",
                    not effect("dispatch", "part2", reader=a)["dispatch_now"],
                )
                effect("settle", "part2", "unknown", "null")
                ctx.equal(
                    "siblings stay independently confirmed",
                    [
                        effect("read", "part" + str(i), reader=a)["state"]
                        for i in range(3)
                    ],
                    ["confirmed", "confirmed", "unknown"],
                )
                # Read back the original external key; no second POST.
                witness = next(row for row in deliveries if row["key"] == "part2")
                effect("settle", "part2", "confirmed", json.dumps(witness))
                effect(
                    "settle",
                    "part2",
                    "confirmed",
                    json.dumps({"forged": "new receipt"}),
                    ok=False,
                )
                effect("settle", "part2", "confirmed", json.dumps(witness))
                op("person", "merge", b, a)
                ctx.equal(
                    "same authorized current family may read history",
                    effect("read", "R1", reader=b),
                    first,
                )
                effect("dispatch", "batch", reader=b, ok=False)
                op("person", "set-enabled", a, "false")
                effect("read", "R1", reader=a, ok=False)
                effect("local", "R1", reader=a, ok=False)
        finally:
            gateway.shutdown()
            peer.join(timeout=5)
    ctx.equal(
        "only one physical call per intended child",
        [r["key"] for r in deliveries],
        ["part0", "part1", "part2"],
    )
    return {
        "local_commit_barriers": ambiguous,
        "first": first,
        "second": second,
        "third": third,
        "read": read,
        "dispatched_before_recovery": before,
        "physical_deliveries": deliveries,
        "unknown_resolved_by_original_key": witness,
    }
