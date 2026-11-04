"""Loopback Chat Completions. Exposes the exact serialized request and explicit transport faults."""

import json
import queue
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Recorder:
    def __init__(self):
        self.wires = queue.Queue()
        self.deliveries = []
        recorder = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_POST(self):
                raw = self.rfile.read(int(self.headers["Content-Length"]))
                if self.path == "/binding":
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
                    body, missing = answer.get(timeout=25)
                raw = json.dumps(body, ensure_ascii=False).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
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
        return (
            "http://127.0.0.1:" + str(self.server.server_port) + "/v1/chat/completions"
        )

    def receive(self):
        path, raw, q = self.wires.get(timeout=12)
        assert path == "/v1/chat/completions"
        return raw.decode(), q
