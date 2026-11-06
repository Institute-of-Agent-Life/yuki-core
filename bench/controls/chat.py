"""One physical, deterministic Chat endpoint for all benchmark arms."""

import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import queue
import threading
import time


class Chat:
    def __init__(self, *, scripted=False):
        self.responses = queue.Queue() if scripted else None
        self.key = None
        self.wires = []
        self.samples = []
        self.lock = threading.Lock()
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_POST(self):
                raw = self.rfile.read(int(self.headers["Content-Length"]))
                body = json.loads(raw)
                last = body["messages"][-1]
                if owner.responses is not None:
                    m, finish = owner.responses.get(timeout=10)
                elif last["role"] == "tool":
                    m = {"role": "assistant", "content": "NO_REPLY"}
                    finish = "stop"
                else:
                    m = {
                        "role": "assistant",
                        "content": None,
                        "tool_calls": [
                            {
                                "type": "function",
                                "id": "send-reply-" + owner.key,
                                "function": {
                                    "name": "send",
                                    "arguments": json.dumps(
                                        {"text": "reply-" + owner.key},
                                        ensure_ascii=False,
                                        separators=(",", ":"),
                                    ),
                                },
                            }
                        ],
                    }
                    finish = "tool_calls"
                with owner.lock:
                    if len(owner.samples) < 2:
                        owner.samples.append(body)
                    owner.wires.append(
                        {
                            "input": owner.key,
                            "monotonic": time.perf_counter(),
                            "finish": finish,
                            "bytes": len(raw),
                            "sha256": hashlib.sha256(raw).hexdigest(),
                            "model": body.get("model"),
                            "prefix_sha256": hashlib.sha256(
                                json.dumps(
                                    body["messages"][0],
                                    ensure_ascii=False,
                                    sort_keys=True,
                                ).encode()
                            ).hexdigest(),
                            "declarations_sha256": hashlib.sha256(
                                json.dumps(
                                    body.get("tools"),
                                    ensure_ascii=False,
                                    sort_keys=True,
                                ).encode()
                            ).hexdigest(),
                            "max_tokens": body.get(
                                "max_tokens", body.get("max_completion_tokens")
                            ),
                        }
                    )
                payload = json.dumps(
                    {
                        "id": "fixture-" + owner.key,
                        "choices": [
                            {"index": 0, "message": m, "finish_reason": finish}
                        ],
                    }
                ).encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *args):
        self.server.shutdown()
        self.thread.join()
        self.server.server_close()

    @property
    def url(self):
        return f"http://127.0.0.1:{self.server.server_port}/v1"
