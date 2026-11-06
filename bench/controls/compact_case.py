"""Original owner with a real HTTP Chat adapter; run in the fixed A or B checkout."""

import json, os
from dataclasses import replace
from pathlib import Path
import httpx
import pytest
from tests.integration import test_compact_delivery_e2e as old
from yuki.llm.openai_compatible import OpenAICompatibleProvider
from bench.controls.chat import Chat
from bench.compact_trace import turns, summary, observed

pytest_plugins = ["tests.conftest"]


@pytest.mark.parametrize("refuse", [False, True], ids=["adopt", "reject-second"])
async def test_two_manual_public_checkpoints(database, tmp_path, refuse):
    out = Path(os.environ["YUKI_SLICE_OUT"])
    env = await old.social_env(database, tmp_path)
    events = []
    trace = []
    summaries = 0
    with Chat(scripted=True, sample_limit=15) as peer:
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

            class Physical(old.FakeLLMProvider):
                async def complete(self, request):
                    nonlocal summaries
                    kind = old._kind(request)
                    if kind == "public_compact":
                        summaries += 1
                    answer = (
                        summary(summaries) if kind == "public_compact" else "NO_REPLY"
                    )
                    self.requests.append(request)
                    if refuse and kind == "public_compact" and summaries == 2:
                        m = {
                            "role": "assistant",
                            "content": None,
                            "tool_calls": [
                                {
                                    "id": "forbidden-public-send",
                                    "type": "function",
                                    "function": {
                                        "name": "send",
                                        "arguments": '{"text":"FORBIDDEN_COMPACT_SEND"}',
                                    },
                                }
                            ],
                        }
                        finish = "tool_calls"
                    else:
                        m = {"role": "assistant", "content": answer}
                        finish = "stop"
                    peer.responses.put((m, finish))
                    physical = await adapter.complete(request)
                    trace.append(
                        {
                            "turn": len(events),
                            "kind": kind,
                            **observed(peer.samples[-1]["messages"]),
                            "wire": peer.wires[-1],
                        }
                    )
                    return physical

            provider = Physical(lambda _: None)
            h = old.build_harness(
                database,
                old.make_settings(
                    database.url, runtime_work_enabled=False, enabled_groups_csv="20001"
                ),
                provider,
            )
            old.bind_main_contract(h, tmp_path)
            chat = h.processor._chat
            chat._tools.social_service = env.service
            env.service.runtime_config = chat._runtime_config
            async with database.immediate_session() as w:
                (await w.get(old.CanonicalPersonModel, env.person)).is_superuser = True
            try:
                for i, (kind, text) in enumerate(turns()):
                    peer.key = f"compact-{i:02}"
                    msg = replace(
                        old.inbound(
                            text,
                            message_id=peer.key,
                            user_id="10001",
                            group_id="20001",
                            mentions_bot=True,
                        ),
                        bot_user_id="80001",
                        conversation_id=env.context.conversation_id,
                        person_id=env.person,
                        space_id=env.space,
                        presence_id=env.presence,
                    )
                    count = len(trace)
                    result = await h.processor.handle(msg, old.MemorySender())
                    events.append(
                        {
                            "turn": i,
                            "kind": kind,
                            "reason": result.reason,
                            "physical": len(trace) - count,
                        }
                    )
                assert summaries == 2 and len(trace) == len(events) and len(trace) == 15
                assert all(e["physical"] == 1 for e in events)
                assert trace[6]["kind"] == trace[13]["kind"] == "public_compact"
                assert 1 in trace[13]["checkpoints"] and trace[14]["checkpoints"] == (
                    [1] if refuse else [2]
                )
                assert not any(
                    "FORBIDDEN_COMPACT_SEND" in str(b) for _, b in env.bot.calls
                )
                assert trace[-1]["tail"] and "1-0" not in trace[-1]["sources"]
            finally:
                (
                    out
                    / (
                        "public-compact-refused.json"
                        if refuse
                        else "public-compact.json"
                    )
                ).write_text(
                    json.dumps(
                        {"trace": trace, "turns": events, "summaries": summaries},
                        indent=2,
                    )
                    + "\n"
                )
