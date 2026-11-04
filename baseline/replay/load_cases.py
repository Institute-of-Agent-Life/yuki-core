"""Run the A schedule through v4. Every input has a separately observed gateway effect."""
import json
import os
import random
import time
from dataclasses import replace
from pathlib import Path
from unittest.mock import patch

import pytest
from sqlalchemy import select
pytest_plugins = ["tests.conftest"]
from tests.integration import test_current_material_main_contract as deployment
from tests.integration.test_client_continuation import (
    response, test_original_work_continues_across_devices_tools_and_two_cold_runtimes as original_work,
)
from tests.support.account_observations import apply_observations, profile_message
from tests.support.canonical_identity import ensure_space
from yuki.domain.conversations import ScopeType
from yuki.llm.fake import FakeLLMProvider
from yuki.persistence.models import ChatEventModel

SPEC = json.loads(Path(__file__).with_name("scenarios.json").read_text())
OUT = Path(os.environ["YUKI_ORACLE_OUT"])
SMOKE = os.environ.get("YUKI_ORACLE_SMOKE") == "1"


async def test_queued_inputs(database, tmp_path):
    load = SPEC["load"]
    schedule = ["private"] * (1 if SMOKE else load["private"])
    for group, count in load["groups"].items():
        schedule.extend([group] * (1 if SMOKE else count))
    random.Random(SPEC["seed"]).shuffle(schedule)
    # The schedule has one in-flight ingress. One visible send precedes the final response.
    expected = ""
    def respond(request):
        if request.messages[-1].role == "tool":
            return "NO_REPLY"
        return response("send", {"text": expected}, "send-" + expected)
    provider = FakeLLMProvider(respond)
    configure = deployment.make_settings
    def two_groups(url, **settings):
        settings.update(enabled_groups_csv="20001,20002", per_user_requests_per_minute=2400,
                        per_group_requests_per_minute=2400)
        return configure(url, **settings)
    with patch.object(deployment, "make_settings", two_groups):
        app, env, message, sender = await deployment._deployment(database, tmp_path, provider)
    try:
        async with database.immediate_session() as session:
            other = await ensure_space(session, "20002")
        await apply_observations(database, profile_message(user_id="10001", nickname="test-10001", group_id="20002"))
        assert await env.router.cas_takeover_space(other) == "taken"
        with (OUT / "arrivals.jsonl").open("x", encoding="utf-8") as log:
            for number, target in enumerate(schedule):
                key = f"a-{number:04d}"
                expected = "reply-" + key
                group = None if target == "private" else target
                source = replace(message, ingress_key=key, native_message_id=key, text=key, group_id=group,
                    scope_type=ScopeType.GROUP if group else ScopeType.PRIVATE, mentions_bot=bool(group),
                    event_type="message:group:normal" if group else "message:private:friend")
                offset = len(env.bot.calls)
                started = time.perf_counter()
                result = await app.processor.handle(source, sender)
                elapsed = time.perf_counter() - started
                action = "send_group_msg" if group else "send_private_msg"
                calls = [params for name, params in env.bot.calls[offset:] if name == action]
                async with database.read_session() as session:
                    saved = (await session.scalars(select(ChatEventModel).where(
                        ChatEventModel.content == expected, ChatEventModel.direction == "outbound"))).all()
                record = {"input": key, "target": target, "reason": result.reason, "gateway": calls,
                          "event_ids": [r.id for r in saved], "seconds": elapsed}
                log.write(json.dumps(record, ensure_ascii=False) + "\n")
                log.flush()
                assert result.reason == "chat" and len(calls) == len(saved) == 1, record
                assert expected in json.dumps(calls[0]) and str(calls[0].get("group_id" if group else "user_id")) == (group or "10001")
                provider.requests.clear()  # the fake retains whole requests for inspection
    finally:
        assert not await app.lifecycle._close_entries(app.lifecycle._entries)


@pytest.mark.parametrize("n", range(1 if SMOKE else SPEC["load"]["long_works"]))
async def test_long_work(database, tmp_path, n):
    started = time.perf_counter()
    await original_work(database, tmp_path, "qq")
    with (OUT / "long-works.jsonl").open("a", encoding="utf-8") as log:
        log.write(json.dumps({"work": n, "original": SPEC["cases"]["app_work"],
                              "seconds": time.perf_counter() - started}) + "\n")
