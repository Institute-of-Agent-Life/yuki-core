"""Run with the pinned isolated v4 environment. No provider connection is made."""

import asyncio
import json
import subprocess
import sys
from pathlib import Path

import httpx
from yuki.memory.embedding.openai_compatible import OpenAICompatibleEmbeddingProvider
from yuki.memory.embedding.qwen import QwenDashScopeEmbeddingProvider
from yuki.memory.embedding.text import EmbeddingDocumentBuilder

BASE = "329ca7c487414d2f54ddd98ff4727446ed2446cd"


async def capture():
    assert (
        subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip() == BASE
    )
    doc = EmbeddingDocumentBuilder(template_version=1, max_characters=2048)
    sample = dict(
        kind="fact",
        category="preference",
        memory_key="tea",
        content="選 １２３４５ 的茶\t  。",
    )
    wires = []

    def peer(r):
        data = json.loads(r.content)
        wires.append({"path": r.url.path, "body": data})
        if "parameters" in data:
            return httpx.Response(
                200,
                json={
                    "output": {
                        "embeddings": [{"text_index": 0, "embedding": [1.0] * 1024}]
                    },
                    "usage": {"total_tokens": 5},
                },
            )
        return httpx.Response(
            200,
            json={
                "data": [{"index": 0, "embedding": [1.0, 0.0]}],
                "usage": {"prompt_tokens": 5},
            },
        )

    async with httpx.AsyncClient(transport=httpx.MockTransport(peer)) as client:
        for adapter in (
            OpenAICompatibleEmbeddingProvider(
                base_url="http://127.0.0.1/v1",
                api_key="",
                model="fixture",
                dimensions=2,
                client=client,
            ),
            QwenDashScopeEmbeddingProvider(
                base_url="http://127.0.0.1/v1",
                model="fixture-q",
                api_key="fixture",
                client=client,
            ),
        ):
            await adapter.embed_documents((doc.build_fields(**sample),))
            await adapter.embed_query("茶")
    return {
        "source_commit": BASE,
        "fields": sample,
        "projection": doc.build_fields(**sample),
        "sha256": doc.content_hash_fields(**sample),
        "wires": wires,
    }


if __name__ == "__main__":
    Path(sys.argv[1]).write_text(
        json.dumps(asyncio.run(capture()), ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
