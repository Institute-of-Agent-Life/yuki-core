"""Capture actual HTTP bodies from the pinned v4 adapter using four offline choices."""

import argparse
import asyncio
import hashlib
import json
import subprocess
import sys
import threading
from dataclasses import replace
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

BASE = "329ca7c487414d2f54ddd98ff4727446ed2446cd"


async def capture(root, output):
    if (
        subprocess.check_output(
            ["git", "-C", root, "rev-parse", "HEAD"], text=True
        ).strip()
        != BASE
    ):
        raise ValueError("pinned isolated checkout required")
    sys.dont_write_bytecode = True
    sys.path[:0] = [str(root / "src"), str(root)]
    import httpx
    from tests.unit.test_provider_protocols import request
    from yuki.llm.anthropic_messages import AnthropicMessagesProvider

    wires = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            data = self.rfile.read(int(self.headers["Content-Length"]))
            wires.append(
                {
                    "path": self.path,
                    "version": self.headers["anthropic-version"],
                    "body": json.loads(data),
                    "raw_sha256": hashlib.sha256(data).hexdigest(),
                }
            )
            self.send_response(200)
            self.send_header("Content-Length", "2")
            self.end_headers()
            self.wfile.write(b"{}")

    with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            base = f"http://127.0.0.1:{server.server_port}/v1"
            async with httpx.AsyncClient(
                base_url=base + "/", trust_env=False
            ) as client:
                adapter = AnthropicMessagesProvider(
                    base_url=base,
                    api_key="synthetic",
                    timeout_seconds=3,
                    max_retries=0,
                    client=client,
                )
                for thinking in (False, True):
                    for choice in ("required", "inspect"):
                        req = replace(
                            request(), thinking_enabled=thinking, tool_choice=choice
                        )
                        await adapter._post(adapter._prepare(req))
                        wires[-1].update(
                            thinking_enabled=thinking, requested_choice=choice
                        )
        finally:
            server.shutdown()
            thread.join()
    with output.open("x", encoding="utf-8") as f:
        json.dump(
            {"source_commit": BASE, "wires": wires}, f, indent=2, ensure_ascii=False
        )
        f.write("\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    asyncio.run(capture(args.root.resolve(), args.output))
