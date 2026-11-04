"""Run using the isolated, pinned v4 checkout environment. Capture actual adapter POST bodies on loopback."""

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


async def capture(root, out):
    if (
        subprocess.check_output(
            ["git", "-C", root, "rev-parse", "HEAD"], text=True
        ).strip()
        != BASE
    ):
        raise ValueError("fixed oracle checkout required")
    sys.dont_write_bytecode = True
    sys.path[:0] = [str(root / "src"), str(root)]
    import httpx
    from tests.unit.test_provider_protocols import request
    from yuki.llm.openai_responses import OpenAIResponsesProvider
    from yuki.llm.deepseek_responses import DeepSeekResponsesProvider
    from yuki.domain.messages import NativeToolDefinition, NativeToolType

    recorded = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            raw = self.rfile.read(int(self.headers["Content-Length"]))
            recorded.append(
                {
                    "path": self.path,
                    "body": json.loads(raw),
                    "raw_sha256": hashlib.sha256(raw).hexdigest(),
                }
            )
            self.send_response(200)
            self.send_header("Content-Length", "2")
            self.end_headers()
            self.wfile.write(b"{}")

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    results = {}
    try:
        endpoint = f"http://127.0.0.1:{server.server_port}/v1"
        async with httpx.AsyncClient(
            base_url=endpoint + "/", trust_env=False
        ) as client:
            for kind in (OpenAIResponsesProvider, DeepSeekResponsesProvider):
                adapter = kind(
                    base_url=endpoint,
                    api_key="synthetic",
                    timeout_seconds=4,
                    max_retries=0,
                    client=client,
                )
                req = replace(
                    request(),
                    tool_choice="inspect",
                    native_tools=(
                        NativeToolDefinition(type=NativeToolType.WEB_SEARCH),
                    ),
                )
                await adapter._post(adapter._prepare(req))
                results[kind.__name__] = recorded[-1]
            try:
                adapter._prepare(replace(req, tool_choice="none"))
            except Exception as err:
                results["legacy_rejection"] = type(err).__name__
            else:
                raise AssertionError("legacy adapter accepted native plus none")
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
    with out.open("x", encoding="utf-8") as f:
        json.dump(
            {"source_commit": BASE, "adapter_bodies": results},
            f,
            ensure_ascii=False,
            indent=2,
        )
        f.write("\n")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("checkout", type=Path)
    ap.add_argument("out", type=Path)
    args = ap.parse_args()
    asyncio.run(capture(args.checkout.resolve(), args.out))
