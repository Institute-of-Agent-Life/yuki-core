"""Run the v4 application and gateway binding with the same physical Chat script and fixed arrivals as C."""

import json
import os
import time
from pathlib import Path
from datetime import UTC, datetime
from unittest.mock import patch

import httpx
from sqlalchemy import select
from tests.integration import test_current_material_main_contract as deployment
from tests.support.account_observations import apply_observations, profile_message
from tests.support.canonical_identity import ensure_presence, ensure_space
from nonebot.adapters.onebot.v11 import GroupMessageEvent, PrivateMessageEvent
from yuki.adapters.onebot.normalizer import normalize_event
from bench.native_arrivals import ASSIGNMENT_EPOCH, load, pack, sha
from yuki.llm.openai_compatible import OpenAICompatibleProvider
from yuki.persistence.models import ChatEventModel
from bench.compare import trial_schedule, storage
from bench.controls.chat import Chat

pytest_plugins = ["tests.conftest"]
OUT = Path(os.environ["YUKI_SLICE_OUT"])


async def test_original_slice(database, tmp_path):
    length = int(os.environ.get("YUKI_SLICE_COUNT", "1200"))
    original = deployment.make_settings

    def settings(url, **kwargs):
        kwargs.update(
            enabled_groups_csv="20001,20002",
            per_user_requests_per_minute=2400,
            per_group_requests_per_minute=2400,
        )
        return original(url, **kwargs)

    timings = []
    with Chat() as peer:
        async with httpx.AsyncClient(
            base_url=peer.url + "/", trust_env=False
        ) as client:
            provider = OpenAICompatibleProvider(
                base_url=peer.url,
                api_key="fixture",
                timeout_seconds=10,
                max_retries=0,
                client=client,
            )
            seed = datetime.fromtimestamp(ASSIGNMENT_EPOCH, UTC)
            async with database.immediate_session() as session:
                await ensure_presence(session, "80001", now=seed)
            with patch.object(deployment, "make_settings", settings):
                app, env, _, sender = await deployment._deployment(
                    database, tmp_path, provider, code_enabled=False
                )
            try:
                async with database.immediate_session() as session:
                    other = await ensure_space(session, "20002", now=seed)
                await apply_observations(
                    database,
                    profile_message(
                        user_id="10001", nickname="test-10001", group_id="20002"
                    ),
                )
                assert await env.router.cas_takeover_space(other) == "taken"
                with (OUT / "arrivals.jsonl").open("x", encoding="utf-8") as log:
                    plan = trial_schedule(
                        length, int(os.environ.get("YUKI_SLICE_WARMUP", "0"))
                    )
                    packets = load(
                        Path(os.environ["YUKI_SLICE_FRAMES"]),
                        os.environ["YUKI_SLICE_FRAMES_SHA256"],
                        plan,
                    )
                    for expected, packet in zip(plan, packets, strict=True):
                        key, target = expected["input"], expected["target"]
                        kind = (
                            GroupMessageEvent
                            if packet["message_type"] == "group"
                            else PrivateMessageEvent
                        )
                        event = kind.model_validate(
                            {**packet, "original_message": packet["message"]}
                        )
                        source = normalize_event(event)
                        cursor = len(env.bot.calls)
                        wire = len(peer.wires)
                        peer.key = key
                        start = time.perf_counter()
                        result = await app.processor.handle(source, sender)
                        end = time.perf_counter()
                        calls = [
                            params
                            for action, params in env.bot.calls[cursor:]
                            if action in ("send_group_msg", "send_private_msg")
                        ]
                        async with database.read_session() as session:
                            events = (
                                await session.scalars(
                                    select(ChatEventModel).where(
                                        ChatEventModel.direction == "outbound",
                                        ChatEventModel.content == "reply-" + key,
                                    )
                                )
                            ).all()
                        row = {
                            "input": key,
                            "target": target,
                            "packet_sha256": sha(pack(packet)),
                            "native_message_id": source.native_message_id,
                            "warmup": key.startswith("warm-"),
                            "reason": result.reason,
                            "gateway": calls,
                            "event_ids": [e.id for e in events],
                            "seconds": end - start,
                        }
                        log.write(json.dumps(row, ensure_ascii=False) + "\n")
                        log.flush()
                        wires = peer.wires[wire:]
                        timings.append(
                            {
                                "input": key,
                                "first_request_seconds": wires[0]["monotonic"] - start
                                if wires
                                else None,
                                "wire": wires,
                            }
                        )
                        assert (
                            len(calls) == len(events) == 1 and result.reason == "chat"
                        ), row
                (OUT / "storage.json").write_text(
                    json.dumps(storage(Path(database.engine.url.database).parent))
                    + "\n"
                )
            finally:
                (OUT / "wire-samples.json").write_text(
                    json.dumps(peer.samples, ensure_ascii=False) + "\n",
                    encoding="utf-8",
                )
                (OUT / "wire.json").write_text(
                    json.dumps(timings, ensure_ascii=False) + "\n", encoding="utf-8"
                )
                assert not await app.lifecycle._close_entries(app.lifecycle._entries)
