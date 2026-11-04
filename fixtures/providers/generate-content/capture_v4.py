"""Record original GenerateContent POSTs including one tool with and one without a native ID."""

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
        raise ValueError("isolated pinned source required")
    sys.dont_write_bytecode = True
    sys.path[:0] = [str(root / "src"), str(root)]
    import httpx
    from tests.unit.test_provider_protocols import (
        request,
        parse_response,
        TurnTranscript,
    )
    from yuki.llm.gemini import GeminiProvider
    from yuki.domain.messages import NativeToolDefinition, NativeToolType

    parts = [
        {"thought": True, "text": "private synthetic thought"},
        {
            "functionCall": {"name": "inspect", "id": "native-1", "args": {"index": 1}},
            "thoughtSignature": "signed+/first",
        },
        {
            "functionCall": {"name": "inspect", "args": {"index": 2}},
            "thoughtSignature": "signed+/second",
        },
    ]
    response = {
        "candidates": [
            {"finishReason": "STOP", "content": {"role": "model", "parts": parts}}
        ]
    }
    wires = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            raw = self.rfile.read(int(self.headers["Content-Length"]))
            wires.append(
                {
                    "path": self.path,
                    "body": json.loads(raw),
                    "raw_sha256": hashlib.sha256(raw).hexdigest(),
                }
            )
            b = json.dumps(response).encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(b)))
            self.end_headers()
            self.wfile.write(b)

    with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            base = f"http://127.0.0.1:{server.server_port}/v1beta"
            async with httpx.AsyncClient(
                base_url=base + "/", trust_env=False
            ) as client:
                adapter = GeminiProvider(
                    base_url=base,
                    api_key="synthetic",
                    timeout_seconds=4,
                    max_retries=0,
                    client=client,
                )
                original = replace(
                    request(),
                    native_tools=(
                        NativeToolDefinition(type=NativeToolType.WEB_SEARCH),
                    ),
                )
                await adapter._post(adapter._prepare(original))
                result = parse_response(
                    adapter, httpx.Response(200, json=response), original
                )
                transcript = TurnTranscript(original.messages)
                transcript.accept(result.continuation)
                for call in result.tool_calls:
                    transcript.append_result(call.id, call.function.arguments)
                sequence = transcript.request()
                await adapter._post(
                    adapter._prepare(
                        replace(
                            original,
                            continuation=sequence.continuation,
                            continuation_items=sequence.items,
                        )
                    )
                )
        finally:
            server.shutdown()
            thread.join()
    with out.open("x", encoding="utf-8") as f:
        json.dump(
            {"source_commit": BASE, "wires": wires, "original_response": response},
            f,
            indent=2,
            ensure_ascii=False,
        )
        f.write("\n")


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("checkout", type=Path)
    p.add_argument("out", type=Path)
    args = p.parse_args()
    asyncio.run(capture(args.checkout.resolve(), args.out))
