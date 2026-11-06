"""Drive the Rust QQ owner with the fixed original post-send automatic Compact input."""

import gzip
import json
import os
import shutil
from contextlib import closing

from bench.automatic_trace import observation
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer


def group_frame(identity, text):
    return {
        "post_type": "message",
        "message_type": "group",
        "self_id": 100,
        "group_id": 20001,
        "user_id": 200,
        "message_id": identity,
        "time": 1787529600 + identity,
        "sender": {"nickname": "Alice"},
        "message": [
            {"type": "at", "data": {"qq": "100"}},
            {"type": "text", "data": {"text": text}},
        ],
    }


def run(ctx, *, withdraw=False):
    oracle = json.loads(
        gzip.decompress(
            (ctx.root / "fixtures/compact/v4-automatic-delivery.json.gz").read_bytes()
        )
    )
    stimulus = oracle["stimulus"]
    prefix = "withdrawn" if withdraw else "normal"
    owner = Owner(ctx, "auto-" + prefix)
    shutil.copyfile(ctx.root / "character/SOUL.md", owner.svc.data / "SOUL.md")
    wire = []
    deliveries = []
    with Recorder() as model:
        cfg = owner.svc.data / "yuki.toml"
        cfg.write_text(
            'model_profiles_file="automatic-models.toml"\n'
            + cfg.read_text(encoding="utf-8"),
            encoding="utf-8",
        )
        (owner.svc.data / "automatic-models.toml").write_text(
            f'''schema_version = 3
[profiles.main]
provider = "fixture"
protocol = "chat_completions"
base_url = "{model.url.removesuffix("/chat/completions")}"
api_key_env = "YUKI_AUTOMATIC_TEST_QQ"
model = "synthetic-model"
timeout_seconds = 10.0
max_retries = 0
default_temperature = 0.5
default_max_output_tokens = 1024
thinking_mode = "enabled"
reasoning_effort = "medium"
capabilities = ["tools","reasoning"]
[routes]
chat_agent = "main"
memory_extraction = "main"
memory_self_reflection = "main"
memory_dream = "main"
plugin_agent_session = "main"
utility_structured = "main"
''',
            encoding="utf-8",
        )
        with cfg.open("a", encoding="utf-8") as f:
            f.write(
                f'\n[main]\nendpoint="{model.url}"\nmodel="synthetic-model"\nname="Yuki"\n'
                f"max_output=1024\nroot_slots=12\nmax_input_units=4000000\nmerge_ms=0\n"
                f'compact_soft_tokens={stimulus["trigger"]}\n[qq]\nnamespace="compact"\n'
                f'designated_ingress="100"\n[qq.groups]\n"20001"="open"\n[[qq.bots]]\nself_id="100"\nstage="{owner.bot["assignment"]}"\n'
                'token_env="YUKI_AUTOMATIC_TEST_QQ"\n'
            )
        env = dict(os.environ, YUKI_AUTOMATIC_TEST_QQ="offline")
        with (
            owner.svc.running(environment=env),
            closing(Peer(owner.svc.address, "100", "offline")) as peer,
        ):
            owner.until(lambda: owner.op("qq", "status", "100")["online"])
            peer.send(group_frame(10, stimulus["old_public"]))
            setup, q = owner.wire(model)
            reply(q, "NO_REPLY")
            owner.finished(
                owner.rows("SELECT event_id FROM event_locators WHERE native_id='10'")[
                    0
                ][0]
            )
            ctx.write(prefix + "-prior-history-request.json", setup)
            peer.send(group_frame(11, stimulus["question"]))
            event = owner.until(
                lambda: owner.rows(
                    "SELECT event_id FROM event_locators WHERE native_id='11'"
                )
            )[0][0]

            def acknowledge():
                while len(deliveries) < len(
                    oracle["scenarios"]["finish"][-1]["delivered_parts"]
                ):
                    action = peer.receive()
                    if action["action"] == "get_group_member_info":
                        peer.send(
                            {
                                "status": "ok",
                                "retcode": 0,
                                "data": {"role": "member"},
                                "echo": action["echo"],
                            }
                        )
                        continue
                    if action["action"] != "send_group_msg":
                        raise ValueError("unexpected gateway call " + action["action"])
                    deliveries.append(action["params"])
                    peer.send(
                        {
                            "status": "ok",
                            "retcode": 0,
                            "data": {"message_id": 1000 + len(deliveries)},
                            "echo": action["echo"],
                        }
                    )

            try:
                for turn in range(8):
                    request, q = owner.wire(model)
                    last = request["messages"][-1]["content"] or ""
                    if last.startswith("[Host 会话上下文压缩要求]"):
                        kind = "public_compact"
                        reply(q, "PUBLIC_CHECKPOINT: preserve the earlier decision.")
                    elif last.startswith("[Host 上下文压缩要求]"):
                        kind = "execution_compact"
                        reply(
                            q,
                            "PRIVATE_CHECKPOINT: fanout was answered successfully; no remaining action.",
                        )
                    elif turn == 0:
                        kind = "normal"
                        reply(
                            q,
                            calls=[
                                call(
                                    stimulus["call_id"],
                                    "send",
                                    {"text": stimulus["answer"]},
                                )
                            ],
                        )
                        acknowledge()
                    else:
                        kind = "normal"
                        if withdraw:
                            from tests.harness.send_entry import (
                                recall_after_reserve,
                            )

                            receipt = next(
                                json.loads(m["content"])
                                for m in request["messages"]
                                if m.get("tool_call_id") == stimulus["call_id"]
                            )
                            execution = owner.rows(
                                "SELECT id FROM executions WHERE source=?", event
                            )[0][0]
                            recall_after_reserve(
                                owner,
                                execution,
                                receipt["data"]["parts"][2]["event_id"],
                            )
                            reply(
                                q,
                                calls=[
                                    call("after-withdrawal", "time_get_current", {})
                                ],
                            )
                        else:
                            reply(q, "NO_REPLY")
                    wire.append((kind, request))
                    if turn and kind == "normal":
                        if withdraw:
                            owner.until(
                                lambda: owner.rows(
                                    "SELECT t.diagnostic FROM executions x JOIN main_turns t ON t.execution=x.id "
                                    "WHERE x.source=? AND x.state='held'",
                                    event,
                                )
                                == [("send_receipt_source_changed",)]
                            )
                            ctx.equal(
                                "withdrawal at reservation blocks the fourth physical dispatch",
                                owner.rows(
                                    "SELECT count(*) FROM model_attempts WHERE logical=(SELECT reserved_logical FROM send_receipt_fault)"
                                ),
                                [(0,)],
                            )
                            ctx.check(
                                "the affected continuation was actually reserved",
                                bool(
                                    owner.rows(
                                        "SELECT reserved_logical FROM send_receipt_fault"
                                    )[0][0]
                                ),
                            )
                        else:
                            owner.finished(event)
                        break
            finally:
                ctx.write(prefix + "-automatic-wire.json", wire)
                ctx.write(prefix + "-automatic-gateway.json", deliveries)
                ctx.write(
                    prefix + "-automatic-operations.json",
                    owner.rows("SELECT state,target FROM operations"),
                )
    observed = [observation(kind, body, stimulus) for kind, body in wire]
    ctx.write(prefix + "-automatic-observed.json", observed)
    ctx.equal(
        "automatic P keeps the original six-part send receipt",
        observed,
        oracle["scenarios"]["finish"],
    )
    expected_parts = observed[-1]["delivered_parts"]
    ctx.equal(
        "one native group acknowledgement for each original delivered text",
        [(d["group_id"], d["message"]) for d in deliveries],
        [
            (20001, [{"type": "text", "data": {"text": text}}])
            for text in expected_parts
        ],
    )
    return {
        "scenario": prefix,
        "physical_requests": len(wire),
        "confirmed_parts": len(deliveries),
        "original_oracle": oracle["arms"],
        "middle_part_withdrawn": withdraw,
    }
