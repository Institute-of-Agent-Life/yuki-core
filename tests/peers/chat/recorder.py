"""Loopback Chat Completions. Exposes the exact serialized request and explicit transport faults."""

import json
import queue
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Recorder:
    def __init__(self, path="/v1/chat/completions", binding_status=200):
        self.path = path
        self.wires = queue.Queue()
        self.deliveries = []
        self.header_shapes = []
        recorder = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_POST(self):
                raw = self.rfile.read(int(self.headers["Content-Length"]))
                recorder.header_shapes.append({
                    "version": self.headers.get("anthropic-version"),
                    "api_key": "x-api-key" in self.headers,
                    "bearer": "Authorization" in self.headers,
                })
                if "x-goog-api-key" in self.headers:
                    recorder.header_shapes[-1]["google_key"] = True
                status = 200
                if self.path == "/binding":
                    status = binding_status
                    call = json.loads(raw)
                    recorder.deliveries.append(call)
                    body, missing = (
                        {
                            "accepted": call["operation"],
                            "sequence": len(recorder.deliveries),
                        },
                        0,
                    )
                else:
                    answer = queue.Queue()
                    recorder.wires.put((self.path, raw, answer))
                    result = answer.get(timeout=25)
                    body, missing = result[:2]
                    if len(result)==3: status=result[2]
                raw = (
                    body
                    if isinstance(body, bytes)
                    else json.dumps(body, ensure_ascii=False).encode()
                )
                self.send_response(status)
                self.send_header(
                    "Content-Type",
                    "text/event-stream"
                    if isinstance(body, bytes)
                    else "application/json",
                )
                self.send_header("Content-Length", str(len(raw) + missing))
                self.end_headers()
                try:
                    self.wfile.write(raw)
                except (BrokenPipeError, ConnectionResetError):
                    pass

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    @property
    def url(self):
        return "http://127.0.0.1:" + str(self.server.server_port) + self.path

    def receive(self):
        path, raw, q = self.wires.get(timeout=12)
        assert path == self.path
        return raw.decode(), q
