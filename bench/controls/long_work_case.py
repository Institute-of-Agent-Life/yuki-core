"""Replay the original two-device QQ Work assertions with physical Chat requests."""

import json
import os
from pathlib import Path
from unittest.mock import patch

import httpx
import pytest
from tests.integration import test_client_continuation as old
from yuki.llm.openai_compatible import OpenAICompatibleProvider
from bench.controls.chat import Chat

pytest_plugins = ["tests.conftest"]
OUT = Path(os.environ["YUKI_SLICE_OUT"])


@pytest.mark.parametrize("index", range(int(os.environ["YUKI_LONG_WORKS"])))
async def test_original_work(database, tmp_path, index):
    with Chat(scripted=True) as peer:
        peer.key = f"long-{index}"
        async with httpx.AsyncClient(
            base_url=peer.url + "/", trust_env=False
        ) as client:
            adapter = OpenAICompatibleProvider(
                base_url=peer.url,
                api_key="offline",
                timeout_seconds=10,
                max_retries=0,
                client=client,
            )

            class PhysicalScript(old.FakeLLMProvider):
                async def complete(self, request):
                    # The original fixture asserts its domain request and chooses the
                    # response. Run its actual adapter, HTTP parser and physical body
                    # before delivering those results to the original Work.
                    response = await super().complete(request)
                    m = {"role": "assistant", "content": response.content}
                    if response.tool_calls:
                        m["tool_calls"] = [
                            {
                                "id": c.id,
                                "type": "function",
                                "function": {
                                    "name": c.function.name,
                                    "arguments": c.function.arguments,
                                },
                            }
                            for c in response.tool_calls
                        ]
                    peer.responses.put(
                        (m, "tool_calls" if response.tool_calls else "stop")
                    )
                    return await adapter.complete(request)

            try:
                with patch.object(old, "FakeLLMProvider", PhysicalScript):
                    await old.test_original_work_continues_across_devices_tools_and_two_cold_runtimes(
                        database, tmp_path, "qq"
                    )
            finally:
                (OUT / f"work-{index:02}-wire.json").write_text(
                    json.dumps(peer.wires, indent=2) + "\n"
                )
            assert len(peer.wires) >= 6
