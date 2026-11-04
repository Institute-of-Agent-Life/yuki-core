"""Real loopback recorder and assertions with explicit rejected child runs."""

import datetime
import http.client
import json
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class CaseFailure(AssertionError):
    pass


class Context:
    def __init__(self, root, output, lock, fault=None):
        self.root = root
        self.output = output
        self.seed = lock["seed"]
        self.clock_origin = lock["clock"]
        self.fault = fault
        self.assertions = []

    def check(self, name, condition):
        self.assertions.append({"name": name, "passed": bool(condition)})
        if not condition:
            raise CaseFailure(name)

    def equal(self, name, actual, expected):
        self.check(name, actual == expected)

    def rejects(self, name, exception, operation):
        try:
            operation()
        except exception:
            self.check(name, True)
        else:
            self.check(name, False)

    def write(self, name, value):
        path = self.output / name
        if not path.resolve().is_relative_to(self.output.resolve()):
            raise ValueError("artifact path escapes output")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    def rejected_run(self, card, fault):
        out = self.output / ("fault-" + fault)
        process = subprocess.run(
            [sys.executable, str(self.root / "ci/check.py"), card,
             "--out", str(out), "--fault", fault],
            capture_output=True, timeout=20,
        )
        self.check(f"{fault} rejected by runner", process.returncode != 0)
        return json.loads((out / "manifest.json").read_text(encoding="utf-8"))


class LoopbackRecorder:
    def __init__(self, clock_origin):
        self.observations = []
        self.barriers = []
        self.clock = datetime.datetime.fromisoformat(clock_origin.replace("Z", "+00:00"))

    def __enter__(self):
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                size = int(self.headers["Content-Length"])
                request = json.loads(self.rfile.read(size))
                index = len(owner.observations)
                response = {"accepted": request["event_id"]}
                owner.observations.append({
                    "ordinal": index,
                    "virtual_time": (owner.clock + datetime.timedelta(seconds=index)).isoformat(),
                    "method": "POST", "path": self.path,
                    "request": request, "response": response,
                })
                owner.barriers.append(f"recorded:{index}")
                body = json.dumps(response).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                owner.barriers.append(f"ack:{index}")
                self.wfile.write(body)

            def log_message(self, *_args):
                pass

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        return self

    def post(self, event):
        connection = http.client.HTTPConnection(*self.server.server_address, timeout=5)
        try:
            connection.request("POST", "/events", json.dumps(event),
                               {"Content-Type": "application/json"})
            response = connection.getresponse()
            if response.status != 200:
                raise CaseFailure("unexpected HTTP status")
            return json.loads(response.read())
        finally:
            connection.close()

    def __exit__(self, *_args):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
