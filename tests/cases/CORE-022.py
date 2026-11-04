"""Exercise the real native Main, a 96k crossing and original paid-response recovery."""

import json
from concurrent.futures import ThreadPoolExecutor
from tests.harness.compact import Owner, approx, call, reply, sha
from tests.harness.crash import kill_writer
from tests.peers.chat.recorder import Recorder


def fault_cases(ctx):
    observations = {}
    for fault in ("tool", "reset", "erase", "revoke_admin"):
        o = Owner(ctx, fault)
        group = "700" if fault == "revoke_admin" else None
        with Recorder() as http:
            o.config(http)
            with o.svc.running():
                if group:
                    o.op("person", "set-admin", o.actor, "true")
                prior = o.human("original public source " + fault, group)
                _, q = o.wire(http)
                reply(q, "ok")
                o.finished(prior)
                cmd = o.human("/ai compact original focus", group)
                public, q = o.wire(http)
                ctx.check(
                    f"{fault} manual purchased original public source",
                    "original public source " + fault in str(public),
                )
                if fault == "tool":
                    reply(
                        q,
                        calls=[call("forbidden-p-send", "send", {"text": "forbidden"})],
                    )
                    o.finished(cmd)
                    duplicate = o.human("/ai compact original focus")
                    o.finished(duplicate)
                    ctx.equal(
                        "unchanged rejected candidate is not repurchased",
                        o.rows(
                            "SELECT t.diagnostic FROM main_turns t JOIN executions x ON x.id=t.execution WHERE x.source=?",
                            duplicate,
                        ),
                        [("original_public_candidate_already_recorded",)],
                    )
                else:
                    if fault == "reset":
                        reset = o.human("/ai new")
                        o.until(
                            lambda: o.rows(
                                "SELECT 1 FROM main_controls WHERE event=?", reset
                            )
                        )
                    elif fault == "erase":
                        o.op(
                            "privacy",
                            "erase",
                            "erase-mid-P",
                            o.actor,
                            json.dumps([prior]),
                        )
                    else:
                        o.op("person", "set-admin", o.actor, "false")
                    reply(q, "stale summary must never be published")
                    o.until(
                        lambda: o.rows(
                            "SELECT 1 FROM compact_jobs WHERE state IN ('invalid','rejected')"
                        )
                    )
                states = o.rows(
                    "SELECT j.state,j.summary,p.state FROM compact_jobs j JOIN model_attempts p ON p.logical=j.logical"
                )
                ctx.check(
                    f"{fault} prevents adoption and another physical purchase",
                    len(states) == 1
                    and states[0][0] in ("invalid", "rejected")
                    and states[0][1] is None,
                )
                ctx.equal(
                    f"{fault} produces no send or business tool intent",
                    (
                        http.deliveries,
                        o.rows(
                            "SELECT count(*) FROM model_tool_intents i JOIN compact_jobs j ON j.logical=i.logical"
                        ),
                    ),
                    ([], [(0,)]),
                )
                if fault == "tool":
                    ctx.equal(
                        "original prohibited tool turn held with its paid response",
                        states[0][2],
                        "response_held",
                    )
                observations[fault] = states
    hard = Owner(ctx, "hard-limit")
    with Recorder() as http:
        hard.config(http, "max_input_tokens=64\n")
        with hard.svc.running():
            first = hard.human("too large for this fixed profile")
            hard.until(
                lambda: hard.rows(
                    "SELECT 1 FROM executions WHERE source=? AND state='held'", first
                )
            )
            diagnostic = hard.rows(
                "SELECT t.diagnostic FROM main_turns t JOIN executions x ON x.id=t.execution WHERE x.source=?",
                first,
            )
            ctx.equal(
                "known hard capacity with no admissible cut holds the normal request",
                diagnostic,
                [("model_request_capacity",)],
            )
            manual = hard.human("/ai compact only public")
            hard.finished(manual)
            ctx.equal(
                "manual with no fitting cut performs neither normal nor E purchase",
                (
                    hard.rows("SELECT count(*) FROM model_requests"),
                    hard.rows("SELECT count(*) FROM compact_jobs"),
                    http.wires.empty(),
                ),
                ([(0,)], [(0,)], True),
            )
            observations["capacity"] = diagnostic
    ctx.write("compact-public-faults.json", observations)
    return observations


def native_cases(ctx):
    evidence = {}
    for profile, path in (
        ("openai_responses", "/v1/responses"),
        ("anthropic_messages", "/v1/messages"),
        ("gemini", "/v1beta/models/fixture:generateContent"),
    ):

        def respond(q, text):
            if profile == "openai_responses":
                body = {
                    "status": "completed",
                    "output": [
                        {
                            "id": "answer",
                            "type": "message",
                            "role": "assistant",
                            "content": [{"type": "output_text", "text": text}],
                        }
                    ],
                }
            elif profile == "anthropic_messages":
                body = {
                    "type": "message",
                    "role": "assistant",
                    "stop_reason": "end_turn",
                    "content": [{"type": "text", "text": text}],
                }
            else:
                body = {
                    "candidates": [
                        {
                            "finishReason": "STOP",
                            "content": {"role": "model", "parts": [{"text": text}]},
                        }
                    ]
                }
            q.put((body, 0))

        o = Owner(ctx, profile)
        with Recorder(path) as http:
            o.config(http, f'protocol="{profile}"\ncompact_soft_tokens=29000\n')
            with o.svc.running():
                original = o.human("original shared phrase")
                before, q = o.wire(http)
                respond(q, "normal")
                o.finished(original)
                cmd = o.human("/ai compact")
                public, q = o.wire(http)
                ctx.check(
                    profile + " uses same profile and declaration on physical P",
                    public.get("tools") == before.get("tools")
                    and "original shared phrase" in str(public),
                )
                respond(q, "new native checkpoint")
                o.finished(cmd)
                next_event = o.human("native fresh tail")
                after, q = o.wire(http)
                text = json.dumps(after, ensure_ascii=False)
                ctx.check(
                    profile + " next native wire uses published public prefix",
                    "new native checkpoint" in text
                    and "original shared phrase" not in text
                    and "native fresh tail" in text,
                )
                respond(q, "done")
                o.finished(next_event)
                follow = o.human("keep the paid native continuation")
                fresh, q = o.wire(http)
                ctx.check(
                    profile
                    + " has an uncovered legal public source before the native tool",
                    "native fresh tail" in str(fresh),
                )
                # Synthetic opaque bytes force a soft crossing only after the provider turn.
                signed = "opaque-signature-" + "Q" * 92_000
                args = {"target": "a"}
                if profile == "openai_responses":
                    block = {
                        "id": "provided-id",
                        "type": "reasoning",
                        "encrypted_content": signed,
                    }
                    output = [
                        block,
                        {
                            "type": "function_call",
                            "call_id": "original-call",
                            "id": "provider-item",
                            "name": "get",
                            "arguments": json.dumps(args),
                        },
                    ]
                    q.put(({"status": "completed", "output": output}, 0))
                elif profile == "anthropic_messages":
                    block = {
                        "type": "thinking",
                        "thinking": "private reasoning",
                        "signature": signed,
                    }
                    output = [
                        block,
                        {
                            "type": "tool_use",
                            "id": "original-call",
                            "name": "get",
                            "input": args,
                        },
                    ]
                    q.put(
                        (
                            {
                                "type": "message",
                                "role": "assistant",
                                "stop_reason": "tool_use",
                                "content": output,
                            },
                            0,
                        )
                    )
                else:
                    block = {
                        "thoughtSignature": signed,
                        "functionCall": {
                            "id": "original-call",
                            "name": "get",
                            "args": args,
                        },
                    }
                    output = [block]
                    q.put(
                        (
                            {
                                "candidates": [
                                    {
                                        "finishReason": "STOP",
                                        "content": {"role": "model", "parts": output},
                                    }
                                ]
                            },
                            0,
                        )
                    )
                replay, q = o.wire(http)
                if profile == "openai_responses":
                    ctx.check(
                        profile + " retains opaque item and real call-result id",
                        block in replay["input"]
                        and any(
                            v.get("type") == "function_call_output"
                            and v.get("call_id") == "original-call"
                            for v in replay["input"]
                        ),
                    )
                elif profile == "anthropic_messages":
                    blocks = [b for m in replay["messages"] for b in m["content"]]
                    ctx.check(
                        profile + " retains thinking block and original result id",
                        block in blocks
                        and any(
                            b.get("type") == "tool_result"
                            and b.get("tool_use_id") == "original-call"
                            for b in blocks
                        ),
                    )
                else:
                    parts = [part for m in replay["contents"] for part in m["parts"]]
                    ctx.check(
                        profile
                        + " retains signature on original Part and paired functionResponse",
                        block in parts
                        and any(
                            part.get("functionResponse", {}).get("id")
                            == "original-call"
                            for part in parts
                        ),
                    )
                ctx.equal(
                    profile
                    + " never repurchases or sends opaque native state to public P",
                    o.rows("SELECT count(*) FROM compact_jobs"),
                    [(1,)],
                )
                if "[Host 上下文压缩要求]" in json.dumps(replay, ensure_ascii=False):
                    respond(q, "private native tail")
                    rebased, q = o.wire(http)
                    ctx.check(
                        profile
                        + " private rebase removes opaque bytes and carries original calls",
                        signed not in str(rebased) and "original-call" in str(rebased),
                    )
                respond(q, "finished native tail")
                o.finished(follow)
                evidence[profile] = {
                    "before": sha(before),
                    "public": sha(public),
                    "after": sha(after),
                    "opaque_replay": sha(replay),
                    "states": o.rows("SELECT state,summary FROM compact_jobs"),
                }
    ctx.write("native-public-wires.json", evidence)
    return evidence


def run(ctx):
    o = Owner(ctx, "crossing")
    gold = json.loads((ctx.root / "fixtures/compact/v4-public-plan.json").read_bytes())
    ctx.equal(
        "original gold spans no-cut, partial and full cut",
        [c["coverage"] for c in gold["cases"]],
        [9, None, 3, 5, 9],
    )
    with Recorder() as http:
        o.config(http)
        with o.svc.running():
            probe = o.human("probe")
            first, q = o.wire(http)
            base = approx(first)
            reply(q)
            o.finished(probe)
            seed = "original-public-marker " + "公" * max(
                1, (96_000 - base - 11_000) // 2
            )
            # Each record is one real Windows CLI invocation, below its UTF-16
            # argument limit. Together the two original events form the public prefix.
            midpoint = len(seed) // 2
            for portion in (seed[:midpoint], seed[midpoint:]):
                event = o.human(portion)
                before, q = o.wire(http)
                ctx.check(
                    "original public prefix fits below 96k", approx(before) < 96_000
                )
                reply(q)
                o.finished(event)
            current = o.human("deliver then continue")
            original, q = o.wire(http)
            ctx.check(
                "before original send no public purchase",
                approx(original) < 96_000 and not o.rows("SELECT id FROM compact_jobs"),
            )
            reply(
                q,
                calls=[
                    call(
                        "private-work",
                        "task_control",
                        {"action": "accept", "goal": "PRIVATE-WORK-SENTINEL"},
                    ),
                    call("original-send", "send", {"text": "送" * 9_000}),
                ],
            )
            public, pq = o.wire(http)
            ctx.equal(
                "original recipient accepted exactly one physical send",
                len(http.deliveries),
                1,
            )
            ctx.equal(
                "same declarations/profile/output reservation in P",
                (public["tools"], public["model"], public["max_tokens"]),
                (original["tools"], original["model"], original["max_tokens"]),
            )
            ctx.equal(
                "original P instruction at tail",
                public["messages"][-1],
                {"role": "user", "content": gold["instruction"]},
            )
            public_raw = json.dumps(
                [m for m in public["messages"] if m["role"] != "system"],
                ensure_ascii=False,
            )
            ctx.check(
                "only frozen public history reaches summarizer",
                "original-public-marker" in public_raw
                and all(
                    k not in public_raw
                    for k in [
                        "PRIVATE-WORK-SENTINEL",
                        "deliver then continue",
                        "original-send",
                        "<runtime_context>",
                        "送" * 20,
                    ]
                ),
            )
            appended = o.human("unrelated new tail")
            o.until(
                lambda: o.rows(
                    "SELECT execution FROM main_inputs WHERE event=?", appended
                )
            )
            reply(pq, "public P summary")
            normal, q = o.wire(http)
            visible = "\n".join(str(m.get("content", "")) for m in normal["messages"])
            ctx.write("normal-after-public.json", normal)
            ctx.write(
                "publication-state.json",
                {
                    "jobs": o.rows(
                        "SELECT state,diagnostic,coverage,last_item,summary FROM compact_jobs"
                    ),
                    "fragments": o.rows(
                        "SELECT item,source_key FROM compact_fragments"
                    ),
                    "checkpoint": "public P summary" in visible,
                    "original": "original-public-marker" in visible,
                    "tail": "unrelated new tail" in visible,
                    "private": "PRIVATE-WORK-SENTINEL" in visible,
                },
            )
            ctx.check(
                "next normal wire adopts P and preserves unrelated new input",
                "public P summary" in visible
                and "original-public-marker" not in visible
                and "unrelated new tail" in visible
                and "PRIVATE-WORK-SENTINEL" in visible,
            )
            ctx.equal(
                "original result pairing survives prefix replacement",
                [m["tool_call_id"] for m in normal["messages"] if m["role"] == "tool"],
                ["private-work", "original-send"],
            )
            ctx.equal(
                "paid compact receipt precedes public adoption",
                o.rows(
                    "SELECT j.state,p.state FROM compact_jobs j JOIN model_attempts p ON p.logical=j.logical"
                ),
                [("published", "response")],
            )
            reply(q, text="final")
            o.finished(current)
            root = o.rows("SELECT x.id FROM executions x WHERE x.source=?", current)[0][
                0
            ]
            ctx.equal(
                "compact and normal requests share the original root",
                o.rows(
                    "SELECT count(*),count(distinct r.root) FROM model_requests r JOIN activations a ON a.id=r.activation WHERE a.execution=?",
                    root,
                ),
                [(3, 1)],
            )
            reset = o.human("/ai new")
            o.until(
                lambda: o.rows("SELECT event FROM main_controls WHERE event=?", reset)
            )
            manual = o.human("/ai compact only public")
            o.finished(manual)
            ctx.equal(
                "after new no legal manual P or private fallback",
                o.rows(
                    "SELECT t.diagnostic FROM executions x JOIN main_turns t ON t.execution=x.id WHERE x.source=?",
                    manual,
                ),
                [("no_legal_public_prefix",)],
            )
            ctx.equal(
                "previous checkpoint is invalid after new",
                o.rows("SELECT state,summary FROM compact_jobs"),
                [("invalid", None)],
            )
            denied = o.human("/ai compact", group="700")
            o.until(
                lambda: o.rows(
                    "SELECT x.id FROM executions x WHERE x.source=? AND x.state='held'",
                    denied,
                )
            )
            ctx.equal(
                "unprivileged group compact made no purchase",
                o.rows(
                    "SELECT count(*) FROM activations a JOIN executions x ON x.id=a.execution WHERE x.source=?",
                    denied,
                ),
                [(0,)],
            )
            result = {
                "seed_event": event,
                "appended_event": appended,
                "normal_before": sha(original),
                "compact_request": sha(public),
                "normal_after": sha(normal),
                "original_pairs": ["private-work", "original-send"],
                "root": root,
                "before_estimate": approx(original),
            }

    crash = Owner(ctx, "crash")
    with crash.svc.running():
        prior = crash.human("public evidence before manual focus")
        manual = crash.human("/ai compact approved focus")
    with Recorder() as http:
        crash.config(http)
        with ThreadPoolExecutor(max_workers=1) as pool:
            death = pool.submit(
                kill_writer,
                ctx,
                crash.svc.data,
                "compact_crash_child",
                "compact-response-before-publication",
                {},
            )
            normal, q = crash.wire(http)
            reply(q)
            crash.finished(prior)
            public, q = crash.wire(http)
            ctx.check(
                "manual focus only in public instruction",
                "approved focus" in public["messages"][-1]["content"]
                and not any(
                    m["role"] != "system" and "<runtime_context>" in m["content"]
                    for m in public["messages"]
                ),
            )
            reply(q, "stored before publish")
            death.result()
        ctx.equal(
            "paid response survives hard kill while checkpoint pending",
            crash.rows(
                "SELECT j.state,p.state FROM compact_jobs j JOIN model_attempts p ON p.logical=j.logical"
            ),
            [("pending", "response")],
        )
        count = crash.rows("SELECT count(*) FROM model_attempts")
        with crash.svc.running():
            crash.finished(manual)
            ctx.equal(
                "cold publication reuses original paid response",
                crash.rows("SELECT state,summary FROM compact_jobs"),
                [("published", "stored before publish")],
            )
            ctx.equal(
                "no second request or delivery",
                (
                    crash.rows("SELECT count(*) FROM model_attempts"),
                    http.wires.empty(),
                    http.deliveries,
                ),
                (count, True, []),
            )
            crash.op("privacy", "erase", "erase-P", crash.actor, json.dumps([prior]))
            ctx.equal(
                "source erasure closes checkpoint and scrubs selected public bytes",
                crash.rows(
                    "SELECT (SELECT count(*) FROM compact_jobs WHERE summary IS NOT NULL),(SELECT count(*) FROM compact_fragments WHERE message IS NOT NULL)"
                ),
                [(0, 0)],
            )
    return {
        "crossing": result,
        "faults": fault_cases(ctx),
        "native": native_cases(ctx),
        "cold_recovery": {"manual_event": manual, "physical_attempts": count[0][0]},
        "fixed_v4": gold["source_commit"],
    }
