"""Capture the fixed v4 public history contract in a fresh, isolated database.

Run with the isolated baseline's Python; --baseline must point at the fixed SHA.
The original checkout is read only. All database and emitted evidence live in --out.
"""
import argparse
import asyncio
import hashlib
import json
import pathlib
import subprocess
import sys
from dataclasses import replace
from datetime import UTC, datetime, timedelta

SHA = "329ca7c487414d2f54ddd98ff4727446ed2446cd"


async def capture(baseline, out):
    sys.path[:0] = [str(baseline), str(baseline / "src")]
    from tests.support.event_seed import seed_scoped_event
    from tests.support.social_history_cases import stored_private_entry
    from tests.support.social_identity_cases import social_env
    from yuki.domain.audio import AudioTranscript, serialize_transcripts
    from yuki.domain.conversations import ConversationScope
    from yuki.identity.write_settings import (
        IdentityWriteSettings, configure_identity_write_settings,
    )
    from yuki.persistence.database import Database
    from yuki.services.history_query import HISTORY_TOOL_DESCRIPTION, history_schema

    configure_identity_write_settings(IdentityWriteSettings())
    db = Database(f"sqlite+aiosqlite:///{(out / 'oracle.sqlite').as_posix()}")
    await db.create_schema()
    env = await social_env(db, out)
    group = ConversationScope.group("80001", "20001")
    origin = datetime(2026, 1, 2, tzinfo=UTC)
    rows = []
    try:
        for index, text in enumerate(["first context", "雷達站 electric", "last context"]):
            row = await seed_scoped_event(
                env.service.writer, scope=group, origin_key=f"history-{index}",
                sender_user_id="10001", direction="inbound", content=text,
                occurred_at=origin + timedelta(minutes=index),
            )
            rows.append(row.event)
        voice = await seed_scoped_event(
            env.service.writer, scope=group, origin_key="history-voice",
            sender_user_id="10001", direction="inbound", content="",
            occurred_at=origin + timedelta(minutes=3),
            segments=({"type": "record", "data": {"name": "spoken.ogg"}},),
            audio_transcript=serialize_transcripts((
                AudioTranscript("current", 0, "乾坤 電動車"),
                AudioTranscript("reply", 1, "private quoted decoy"),
            )),
        )
        private = await stored_private_entry(env, "telegram", content="雷達站 private")
        sent = await env.service.execute("send", {"text": "original receipt"}, env.context)
        cases = [
            ("recent_default", {}),
            ("recent_empty_source", {"source": {}}),
            ("recent_source_only", {"source": {"platform": "telegram"}}),
            ("search_literal", {"view": "search", "keyword": "雷达站"}),
            ("search_current_asr", {"view": "search", "keyword": "乾 电动车", "exclude": ["private quoted"]}),
            ("source_target_intersection", {"target": {"conversation_id": env.context.conversation_id}, "source": {"platform": "telegram"}}),
            ("original_author", {"view": "search", "person_id": env.person, "limit": 201}),
            ("self_author", {"view": "search", "author_kind": "yuki"}),
            ("date_utc8", {"view": "search", "start_at": "2026-01-02T08:01:00", "end_at": "2026-01-02T08:02:00"}),
            ("context", {"view": "search", "keyword": "electric", "context_before": 1, "context_after": 1}),
            ("around_ignores_filters", {"view": "around", "event_id": rows[1].id, "before": 1, "after": 1, "keyword": "absent", "author_kind": "system", "has": ["video"], "limit": 1, "cursor": 1, "order": "recent"}),
            ("around_missing", {"view": "around", "event_id": 999999, "before": 1, "after": 1}),
            ("receipt_anchor", {"target": {"operation_id": sent["operation_id"]}, "view": "around", "before": 1}),
            ("positive_page", {"limit": 1, "order": "oldest"}),
        ]
        results = []
        env.bot.calls.clear()
        for name, request in cases:
            result = await env.service.execute(
                "query_chat_history", request, replace(env.context, call_id="read-" + name)
            )
            results.append({"name": name, "request": request, "result": result,
                            "source_event_ids": result.source_event_ids})
        assert not env.bot.calls, "history must not backfill through platform HTTP"
        declaration = {"name": "query_chat_history", "description": HISTORY_TOOL_DESCRIPTION,
                       "parameters": history_schema()}
        document = {"source_commit": SHA, "declaration": declaration,
                    "identities": {"person": env.person, "group": env.space,
                        "group_conversation": env.context.conversation_id,
                        "private_conversation": private.event.canonical_conversation_id,
                        "center": rows[1].id, "voice": voice.event.id, "receipt": sent},
                    "cases": results, "platform_calls": []}
        payload = (json.dumps(document, ensure_ascii=False, indent=2) + "\n").encode()
        (out / "original-history.json").write_bytes(payload)
        print(json.dumps({"source_commit": SHA, "cases": len(results),
                          "sha256": hashlib.sha256(payload).hexdigest(), "out": str(out)}))
    finally:
        await db.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", type=pathlib.Path, required=True)
    parser.add_argument("--out", type=pathlib.Path, required=True)
    args = parser.parse_args()
    baseline = args.baseline.resolve()
    assert subprocess.check_output(["git", "-C", str(baseline), "rev-parse", "HEAD"], text=True).strip() == SHA
    args.out.mkdir(parents=True, exist_ok=False)
    asyncio.run(capture(baseline, args.out.resolve()))
