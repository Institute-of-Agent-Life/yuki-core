"""Exercise installed auxiliary routes through the Rust host and physical HTTP."""

import base64
import hashlib
import json
import subprocess
import sys
from tests.harness.json_file import read_json
import time
import io
import wave
import importlib.util
import os
from pathlib import Path
from tests.harness.compact import Owner, reply, call
from tests.harness.crash import kill_writer
from concurrent.futures import ThreadPoolExecutor
from tests.peers.chat.recorder import Recorder


def install(
    ctx,
    owner,
    folder,
    sdk,
    upstream,
    entry,
    plugin="aux",
    grants=True,
    tool=False,
    dialect="chat",
):
    folder.mkdir()
    text = (
        "\n[[plugins]]\nid=" + json.dumps(plugin) + "\nnamespace=" + json.dumps(plugin)
    )
    text += "\nprogram=" + json.dumps(sys.executable)
    text += "\nargs=" + json.dumps(
        [
            str(ctx.root / "tests/peers/aux-model/peer.py"),
            str(folder),
            str(sdk / "python-install"),
        ]
    )
    permissions = ["observe.metadata", "forget.owner", "llm.complete", "llm.result"] + (
        ["tool.call"] if tool else []
    )
    text += "\ngrants=" + json.dumps(permissions) + "\n"
    if grants:
        text += (
            '\n[plugins.aux_routes.jev]\npurposes=["review"]\norigins=["event","call"]\nentries='
            + json.dumps([entry])
        )
        text += "\n[plugins.aux_routes.jev.profile]\nendpoint=" + json.dumps(
            upstream.url
        )
        text += "\nprotocol=" + json.dumps(dialect)
        text += '\nmodel="aux-fixture"\nname="Jev"\nmerge_ms=0\nroot_slots=2\nmax_input_units=100000\nmax_output=128\n'
    if tool:
        text += (
            "\n[[plugins.tools]]\nname="
            + json.dumps(plugin)
            + '\ndescription="Auxiliary scoped review"\ncacheable=false\nparallel_read=false\ncode_direct=false\nschema={type="object",properties={},additionalProperties=false}\n'
        )
    with (owner.svc.data / "yuki.toml").open("a", encoding="utf-8") as f:
        f.write(text)


class Peer:
    def __init__(self, owner, folder):
        self.owner, self.folder, self.sequence = owner, folder, 0

    def submit(self, method, params):
        self.sequence += 1
        key = str(self.sequence)
        p = self.folder / ("job-" + key + ".tmp")
        p.write_text(json.dumps({"method": method, "params": params}), encoding="utf-8")
        p.replace(self.folder / ("job-" + key + ".json"))
        return key

    def receive(self, key, ok=True):
        p = self.folder / ("reply-job-" + key + ".json")
        self.owner.until(p.exists)
        result = read_json(p)
        if ok is not None:
            if ("result" in result) != ok:
                self.owner.ctx.write("aux-rpc-failure-" + key + ".json", result)
            self.owner.ctx.equal(
                "original auxiliary RPC " + key + " accepted", "result" in result, ok
            )
        return result.get("result", result)

    def call(self, method, params, ok=True):
        return self.receive(self.submit(method, params), ok)


def event(owner, source, text="original review event"):
    return owner.op(
        "ingress",
        "record",
        json.dumps(
            {
                "source": source,
                "account": owner.user["account"],
                "bot_stage": owner.bot["assignment"],
                "kind": "message",
                "observed": 20,
                "occurred": 20,
                "segments": [{"type": "text", "text": text}],
            }
        ),
    )["event"]


def run(ctx):
    sdk = ctx.working / "sdk"
    built = subprocess.run(
        [
            sys.executable,
            str(ctx.root / "ci/publish-contract.py"),
            "build",
            "--out",
            str(ctx.output / "sdk-distributions"),
            "--verify-work",
            str(sdk),
        ],
        capture_output=True,
        text=True,
    )
    if built.returncode:
        ctx.write("sdk-failure.json", {"stderr": built.stderr})
    ctx.equal("both packaged SDKs negotiate auxiliary methods", built.returncode, 0)
    owner = Owner(ctx, "aux-normal", 29)
    with owner.svc.running():
        entry = owner.op(
            "entry", "private", owner.user["assignment"], owner.bot["assignment"]
        )["entry"]
    folder = ctx.working / "peer"
    with Recorder() as upstream:
        install(ctx, owner, folder, sdk, upstream, entry)
        with owner.svc.running():
            owner.until((folder / "ready.json").exists)
            src = event(owner, "jev-source")
            owner.until((folder / ("event-" + str(src) + ".json")).exists)
            peer = Peer(owner, folder)
            params = {
                "origin": {"event": src},
                "call_id": "review-1",
                "route": "jev",
                "purpose": "review",
                "text": "review the original event",
            }
            first = peer.submit("llm.complete", params)
            raw, q = upstream.receive()
            ctx.equal(
                "actual configured model", json.loads(raw)["model"], "aux-fixture"
            )
            repeated = peer.call("llm.complete", params)
            ctx.equal(
                "concurrent original request remains dispatched",
                repeated["state"],
                "dispatched",
            )
            q.put(
                (
                    {
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "content": "accepted review",
                                },
                                "finish_reason": "stop",
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 3,
                            "completion_tokens": 2,
                            "total_tokens": 5,
                        },
                    },
                    0,
                )
            )
            done = peer.receive(first)
            ctx.equal(
                "same logical request across concurrent callbacks",
                repeated["logical"],
                done["logical"],
            )
            ctx.equal("original physical response saved", done["state"], "response")
            ctx.equal(
                "available usage stays with original receipt", done["usage"]["input"], 3
            )
            ctx.equal(
                "one actual model attempt",
                owner.rows("SELECT count(*) FROM model_attempts"),
                [(1,)],
            )
            ctx.equal(
                "review does not promote the human event",
                owner.rows("SELECT count(*) FROM accepted_sources"),
                [(0,)],
            )
            page = peer.call("llm.result", {"request": done["request"], "limit": 7})
            content = bytearray(base64.b64decode(page["bytes"]))
            while page["next_offset"] is not None:
                page = peer.call(
                    "llm.result",
                    {
                        "request": done["request"],
                        "offset": page["next_offset"],
                        "limit": 7,
                    },
                )
                content.extend(base64.b64decode(page["bytes"]))
            ctx.equal(
                "original response pages retain content",
                json.loads(content)["content"],
                "accepted review",
            )
            ctx.equal(
                "original response pages retain hash",
                hashlib.sha256(content).hexdigest(),
                page["sha256"],
            )
            for field, value in [
                ("route", "forbidden"),
                ("purpose", "unapproved"),
                ("text", "changed body"),
            ]:
                peer.call("llm.complete", {**params, field: value}, False)
            peer.call("llm.complete", {**params, "profile": "forged"}, False)
            ctx.equal(
                "denied requests reserve no new slot",
                owner.rows("SELECT reserved_requests FROM root_budgets"),
                [(1,)],
            )
            ctx.check("denied requests issue no HTTP", upstream.wires.empty())
    proofs = {
        "original": done,
        "page_sha256": page["sha256"],
        "consumer": "installed SDK fixture",
        "physical_attempts": 1,
        "cuts": cuts(ctx, sdk),
        "failures": failures(ctx, sdk),
        "parent": parent_call(ctx, sdk),
        "segment": parent_call(ctx, sdk, True),
        "wires": protocols(ctx, sdk),
        "media": media_input(ctx, sdk),
        "changed_profile": changed_profile(ctx, sdk),
    }
    proofs["binary_sha256"] = hashlib.sha256(
        (
            ctx.root
            / "target/debug"
            / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
        ).read_bytes()
    ).hexdigest()
    proofs["host_idl_sha256"] = hashlib.sha256(
        (ctx.root / "sdk/rust/src/host-preview.json").read_bytes()
    ).hexdigest()
    proofs["aux_schema_sha256"] = hashlib.sha256(
        (ctx.root / "protocol/plugin/aux-model/methods.json").read_bytes()
    ).hexdigest()
    proofs["negotiated_aux_methods"] = sorted(
        m
        for m in json.loads((folder / "ready.json").read_bytes())["methods"]
        if m.startswith("llm.")
    )
    ctx.equal(
        "actual SDK negotiated both auxiliary methods",
        proofs["negotiated_aux_methods"],
        ["llm.complete", "llm.result"],
    )
    return proofs


def cuts(ctx, sdk):
    proofs = []
    for phase in (
        "aux-reserved-before-dispatch",
        "aux-dispatched-before-http",
        "aux-response-before-reply",
    ):
        owner = Owner(ctx, phase, 29)
        with owner.svc.running():
            entry = owner.op(
                "entry", "private", owner.user["assignment"], owner.bot["assignment"]
            )["entry"]
        folder = ctx.working / (phase + "-peer")
        with Recorder() as upstream:
            install(ctx, owner, folder, sdk, upstream, entry)
            with ThreadPoolExecutor(max_workers=1) as pool:
                killed = pool.submit(
                    kill_writer,
                    ctx,
                    owner.svc.data,
                    "work_delivery_crash_child",
                    phase,
                    {},
                )
                owner.until((folder / "ready.json").exists)
                src = event(owner, phase)
                owner.until((folder / ("event-" + str(src) + ".json")).exists)
                peer = Peer(owner, folder)
                params = {
                    "origin": {"event": src},
                    "call_id": "original-cut",
                    "route": "jev",
                    "purpose": "review",
                    "text": "retain paid original response",
                }
                key = peer.submit("llm.complete", params)
                received = 0
                if phase == "aux-response-before-reply":
                    _, q = upstream.receive()
                    received += 1
                    reply(q, "saved before kill")
                killed.result()
            original = owner.rows(
                "SELECT id,logical,request_hash FROM host_aux_requests"
            )
            ctx.equal("one original request retained at " + phase, len(original), 1)
            with owner.svc.running():
                if phase == "aux-reserved-before-dispatch":
                    _, q = upstream.receive()
                    received += 1
                    reply(q, "first physical dispatch after restart")
                answer = peer.receive(key)
                ctx.equal(
                    "cold state at " + phase,
                    answer["state"],
                    "unknown" if phase == "aux-dispatched-before-http" else "response",
                )
                ctx.equal(
                    "cold request IDs retained at " + phase,
                    owner.rows("SELECT id,logical,request_hash FROM host_aux_requests"),
                    original,
                )
                ctx.equal(
                    "cold original reservation charged once at " + phase,
                    owner.rows("SELECT reserved_requests FROM root_budgets"),
                    [(1,)],
                )
                ctx.equal(
                    "one physical attempt record at " + phase,
                    owner.rows("SELECT count(*) FROM model_attempts"),
                    [(1,)],
                )
                ctx.check(
                    "no replacement HTTP after recovery at " + phase,
                    upstream.wires.empty(),
                )
                proofs.append(
                    {"cut": phase, "request": answer, "received_http": received}
                )
    return proofs


def failures(ctx, sdk):
    proofs = []
    for kind in (
        "unknown",
        "quota",
        "revoked",
        "erased",
        "no-route",
        "withdraw",
        "context",
    ):
        owner = Owner(ctx, "aux-" + kind, 29)
        with owner.svc.running():
            entry = owner.op(
                "entry", "private", owner.user["assignment"], owner.bot["assignment"]
            )["entry"]
        folder = ctx.working / ("aux-" + kind + "-peer")
        with Recorder() as upstream:
            install(ctx, owner, folder, sdk, upstream, entry, grants=kind != "no-route")
            with owner.svc.running():
                owner.until((folder / "ready.json").exists)
                src = event(owner, kind)
                owner.until((folder / ("event-" + str(src) + ".json")).exists)
                peer = Peer(owner, folder)
                p = {
                    "origin": {"event": src},
                    "call_id": "original",
                    "route": "jev",
                    "purpose": "review",
                    "text": "private auxiliary canary",
                }
                if kind == "no-route":
                    peer.call("llm.complete", p, False)
                    ctx.equal(
                        "metadata subscription never grants a model route",
                        owner.rows("SELECT count(*) FROM model_requests"),
                        [(0,)],
                    )
                    ctx.check(
                        "metadata-only subscriber never crosses HTTP",
                        upstream.wires.empty(),
                    )
                    proofs.append({"case": kind, "requests": 0})
                    continue
                erased_source = src
                if kind == "context":
                    context_text = "retained extra source canary"
                    erased_source = event(owner, "context-source", context_text)
                    p["sources"] = [erased_source]
                    p["context"] = [context_text]
                    peer.call("llm.complete", p, False)
                    owner.op(
                        "privacy", "acquire", "aux", "prior-context-read", erased_source
                    )
                    foreign = owner.op(
                        "ingress",
                        "record",
                        json.dumps(
                            {
                                "source": "foreign-context",
                                "account": owner.user["account"],
                                "bot_stage": owner.bot["assignment"],
                                "space": "outside-aux-entry",
                                "kind": "message",
                                "observed": 21,
                                "occurred": 21,
                                "segments": [
                                    {"type": "text", "text": "outside approved entry"}
                                ],
                            }
                        ),
                    )["event"]
                    owner.op("privacy", "acquire", "aux", "other-entry-read", foreign)
                    peer.call(
                        "llm.complete",
                        {
                            **p,
                            "call_id": "foreign-context-denied",
                            "sources": [foreign],
                        },
                        False,
                    )
                    ctx.equal(
                        "unretained and foreign context reserve no request",
                        owner.rows("SELECT count(*) FROM model_requests"),
                        [(0,)],
                    )
                key = peer.submit("llm.complete", p)
                _, q = upstream.receive()
                if kind == "context":
                    ctx.equal(
                        "declared context joins actual model dependencies",
                        set(owner.rows("SELECT event FROM main_dependencies")),
                        {(src,), (erased_source,)},
                    )
                if kind == "withdraw":
                    owner.op("host", "enable", "aux", "false")
                    owner.until(
                        lambda: owner.rows(
                            "SELECT id FROM model_attempts WHERE state='unknown'"
                        )
                    )
                    reply(q, "provider response after transport owner stopped")
                    owner.op("host", "enable", "aux", "true")
                elif kind == "unknown":
                    q.put((b'{"choices":[', 100))
                elif kind == "revoked":
                    owner.op(
                        "model",
                        "cancel",
                        owner.rows("SELECT execution FROM host_aux_origins")[0][0],
                    )
                    reply(q, "known paid response after cancellation")
                elif kind in ("erased", "context"):
                    owner.op(
                        "privacy",
                        "erase",
                        "erase-aux-source",
                        owner.actor,
                        json.dumps([erased_source]),
                    )
                    reply(q, "late private auxiliary canary")
                else:
                    reply(q, "first paid response")
                done = peer.receive(
                    key, ok=None if kind in ("erased", "context") else True
                )
                if kind == "withdraw":
                    ctx.equal(
                        "plugin closure marks the dispatched attempt unknown",
                        done["state"],
                        "unknown",
                    )
                    ctx.equal(
                        "plugin restart retains one logical reservation",
                        owner.rows("SELECT reserved_requests FROM root_budgets"),
                        [(1,)],
                    )
                elif kind == "unknown":
                    ctx.equal(
                        "partial physical response stays unknown",
                        done["state"],
                        "unknown",
                    )
                    replay = peer.call("llm.complete", p)
                    ctx.equal("unknown original key reads same attempt", replay, done)
                    peer.call(
                        "llm.complete", {**p, "call_id": "fresh-after-unknown"}, False
                    )
                elif kind == "quota":
                    second = peer.submit("llm.complete", {**p, "call_id": "second"})
                    _, q = upstream.receive()
                    reply(q, "last paid slot")
                    second = peer.receive(second)
                    peer.call("llm.complete", {**p, "call_id": "third"}, False)
                    ctx.equal(
                        "original root has two consumed slots",
                        owner.rows(
                            "SELECT reserved_requests,max_requests FROM root_budgets"
                        ),
                        [(2, 2)],
                    )
                    ctx.equal(
                        "paid result remains readable after quota",
                        peer.call("llm.result", {"request": second["request"]})[
                            "receipt"
                        ]["state"],
                        "response",
                    )
                elif kind == "revoked":
                    peer.call("llm.complete", {**p, "call_id": "after-cancel"}, False)
                    ctx.equal(
                        "cancelled late response is held under original cost",
                        owner.rows("SELECT state FROM model_attempts"),
                        [("response_held",)],
                    )
                    ctx.equal(
                        "cancelled call keeps its reservation",
                        owner.rows("SELECT reserved_requests FROM root_budgets"),
                        [(1,)],
                    )
                elif kind in ("erased", "context"):
                    ctx.check(
                        "erasure returns only denial or erased receipt",
                        "error" in done or done.get("erased") is True,
                    )
                    owner.until(
                        lambda: owner.rows(
                            "SELECT id FROM model_attempts WHERE state!='dispatched'"
                        )
                    )
                    ctx.equal(
                        "late erased request preserves reservation",
                        owner.rows("SELECT reserved_requests FROM root_budgets"),
                        [(1,)],
                    )
                    ctx.check(
                        "late erased request removes request and response content",
                        all(
                            "canary" not in str(r)
                            for r in owner.rows(
                                "SELECT request_json,response FROM model_requests JOIN model_attempts ON logical=model_requests.id"
                            )
                        ),
                    )
                if kind == "context":
                    ctx.equal(
                        "primary source survives extra-context erasure",
                        owner.rows(
                            "SELECT count(*) FROM invalidated_sources WHERE source=?",
                            src,
                        ),
                        [(0,)],
                    )
                ctx.check(
                    "failure path issues no replacement HTTP", upstream.wires.empty()
                )
                proofs.append(
                    {
                        "case": kind,
                        "primary_source": src,
                        "erased_source": erased_source
                        if kind in ("erased", "context")
                        else None,
                        "dependencies": owner.rows(
                            "SELECT execution,event FROM main_dependencies ORDER BY execution,event"
                        ),
                        "attempts": owner.rows(
                            "SELECT id,logical,state,ever_unknown FROM model_attempts"
                        ),
                        "requests": owner.rows("SELECT id,root FROM model_requests"),
                    }
                )
    return proofs


def parent_call(ctx, sdk, segment=False):
    tag = "aux-segment" if segment else "aux-parent"
    owner = Owner(ctx, tag, 29)
    with owner.svc.running():
        entry = owner.op(
            "entry", "private", owner.user["assignment"], owner.bot["assignment"]
        )["entry"]
    a, b = ctx.working / (tag + "-a"), ctx.working / (tag + "-b")
    with Recorder() as model, Recorder() as upstream:
        owner.config(model)
        if segment:
            with (owner.svc.data / "yuki.toml").open("a") as f:
                f.write("\n[work_segments]\nmodel_requests=3\nresume_delay_ms=2000\n")
        install(ctx, owner, a, sdk, upstream, entry, plugin="aux_a", tool=True)
        install(ctx, owner, b, sdk, upstream, entry, plugin="aux_b", tool=True)
        with owner.svc.running():
            owner.until((a / "ready.json").exists)
            owner.until((b / "ready.json").exists)
            source = owner.human("call the original plugin")
            _, q = owner.wire(model)
            if segment:
                reply(
                    q,
                    calls=[
                        call(
                            "accept",
                            "task_control",
                            {"action": "accept", "goal": "original aux segment"},
                        )
                    ],
                )
                _, q = owner.wire(model)
            reply(q, calls=[call("original", "aux_a", {})])
            owner.until(lambda: list(a.glob("call-*.json")))
            frame = json.loads(next(a.glob("call-*.json")).read_bytes())
            handle = frame["params"]["handle"]
            pa, pb = Peer(owner, a), Peer(owner, b)
            params = {
                "origin": {"handle": handle},
                "call_id": "original-parent-review",
                "route": "jev",
                "purpose": "review",
                "text": "review inside original caller",
            }
            pb.call("llm.complete", params, False)
            key = pa.submit("llm.complete", params)
            _, q = upstream.receive()
            reply(q, "original root child result")
            answer = pa.receive(key)
            pb.call("llm.result", {"request": answer["request"]}, False)
            ctx.equal(
                "auxiliary call consumes the original parent root",
                owner.rows("SELECT count(*),sum(reserved_requests) FROM root_budgets"),
                [(1, 3 if segment else 2)],
            )
            ctx.equal(
                "auxiliary origin retains original parent",
                owner.rows("SELECT parent_execution FROM host_aux_origins"),
                owner.rows("SELECT id FROM executions WHERE source=?", source),
            )
            (a / ("finish-" + handle + ".json")).write_text("{}")
            if segment:
                owner.until(
                    lambda: owner.rows(
                        "SELECT request_count FROM main_segments WHERE state='yielded'"
                    )
                )
                ctx.equal(
                    "auxiliary request counts in original Work segment",
                    owner.rows(
                        "SELECT request_count FROM main_segments WHERE state='yielded'"
                    ),
                    [(3,)],
                )
            _, q = owner.wire(model)
            reply(q, "done")
            owner.finished(source)
            pa.call("llm.complete", {**params, "call_id": "late-new-call"}, False)
            ctx.equal(
                "paid original result readable after parent completion",
                pa.call("llm.result", {"request": answer["request"]})["receipt"][
                    "state"
                ],
                "response",
            )
            ctx.equal(
                "Main and auxiliary share three original purchases",
                owner.rows("SELECT reserved_requests FROM root_budgets"),
                [(4 if segment else 3,)],
            )
            ctx.check(
                "other plugin and closed parent send no new request",
                upstream.wires.empty(),
            )
            return {
                "parent": frame["params"]["operation"],
                "receipt": answer,
                "root_requests": 4 if segment else 3,
            }


def protocols(ctx, sdk):
    proofs = []
    for dialect in (
        "openai_responses",
        "deepseek_responses_legacy",
        "deepseek_responses",
        "anthropic_messages",
        "gemini",
    ):
        owner = Owner(ctx, "aux-wire-" + dialect, 29)
        with owner.svc.running():
            entry = owner.op(
                "entry", "private", owner.user["assignment"], owner.bot["assignment"]
            )["entry"]
        folder = ctx.working / ("aux-wire-peer-" + dialect)
        path = (
            "/v1/messages"
            if dialect == "anthropic_messages"
            else "/v1beta/models/aux-fixture:generateContent"
            if dialect == "gemini"
            else "/v1/responses"
        )
        with Recorder(path) as upstream:
            install(ctx, owner, folder, sdk, upstream, entry, dialect=dialect)
            with owner.svc.running():
                owner.until((folder / "ready.json").exists)
                src = event(owner, dialect)
                owner.until((folder / ("event-" + str(src) + ".json")).exists)
                peer = Peer(owner, folder)
                params = {
                    "origin": {"event": src},
                    "call_id": "wire",
                    "route": "jev",
                    "purpose": "review",
                    "text": "read the original request only",
                }
                key = peer.submit("llm.complete", params)
                raw, q = upstream.receive()
                wire = json.loads(raw)
                if dialect == "gemini":
                    ctx.equal(
                        "auxiliary Gemini sends actual native parts",
                        wire["contents"][-1]["parts"][0]["text"],
                        params["text"],
                    )
                    response = {
                        "candidates": [
                            {
                                "content": {
                                    "role": "model",
                                    "parts": [{"text": "auxiliary native result"}],
                                },
                                "finishReason": "STOP",
                            }
                        ],
                        "usageMetadata": {
                            "promptTokenCount": 3,
                            "candidatesTokenCount": 2,
                            "totalTokenCount": 5,
                        },
                    }
                elif dialect == "anthropic_messages":
                    ctx.equal(
                        "auxiliary Messages sends actual native blocks",
                        wire["messages"][-1]["content"][0]["text"],
                        params["text"],
                    )
                    response = {
                        "type": "message",
                        "role": "assistant",
                        "stop_reason": "end_turn",
                        "content": [
                            {"type": "text", "text": "auxiliary native result"}
                        ],
                        "usage": {"input_tokens": 3, "output_tokens": 2},
                    }
                else:
                    ctx.check(
                        "auxiliary Responses sends original input " + dialect,
                        params["text"] in json.dumps(wire["input"]),
                    )
                    response = {
                        "id": "response-original",
                        "status": "completed",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "auxiliary native result",
                                    }
                                ],
                            }
                        ],
                        "usage": {
                            "input_tokens": 3,
                            "output_tokens": 2,
                            "total_tokens": 5,
                        },
                    }
                q.put((response, 0))
                answer = peer.receive(key)
                ctx.equal(
                    "native auxiliary response complete " + dialect,
                    answer["state"],
                    "response",
                )
                page = peer.call("llm.result", {"request": answer["request"]})
                body = base64.b64decode(page["bytes"]).decode()
                ctx.check(
                    "native auxiliary original result readable " + dialect,
                    "auxiliary native result" in body,
                )
                ctx.equal(
                    "native auxiliary original usage " + dialect,
                    answer["usage"][
                        "uncached_input" if dialect == "anthropic_messages" else "input"
                    ],
                    3,
                )
                if dialect == "anthropic_messages":
                    ctx.check(
                        "missing cache usage remains unknown",
                        "input" not in answer["usage"],
                    )
                proofs.append(
                    {
                        "dialect": dialect,
                        "request": wire,
                        "response": response,
                        "receipt": answer,
                    }
                )
    return proofs


def media_input(ctx, sdk):
    spec = importlib.util.spec_from_file_location(
        "original_media_case", ctx.root / "tests/cases/CORE-038.py"
    )
    media = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(media)
    audio = io.BytesIO()
    with wave.open(audio, "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(8000)
        out.writeframes(bytes(160))
    proofs = []
    for kind, raw, mime in [
        ("image", media.png(bytes([1, 2, 3])), "image/png"),
        ("audio", audio.getvalue(), "audio/wav"),
        ("converted-audio", audio.getvalue(), "audio/wav"),
    ]:
        owner = Owner(ctx, "aux-media-" + kind, 29)
        with owner.svc.running():
            entry = owner.op(
                "entry", "private", owner.user["assignment"], owner.bot["assignment"]
            )["entry"]
        folder = ctx.working / ("aux-media-peer-" + kind)
        purpose = "inspect" if kind == "image" else "transcript"
        with Recorder() as upstream:
            install(ctx, owner, folder, sdk, upstream, entry)
            config = owner.svc.data / "yuki.toml"
            text = config.read_text().replace(
                '"forget.owner"',
                '"forget.owner", "media.open", "media.process", "media.publish"',
            )
            text = text.replace(
                'purposes=["review"]',
                "purposes="
                + json.dumps([purpose])
                + "\nmedia="
                + json.dumps(["image" if kind == "image" else "audio"]),
            )
            if kind == "image":
                text = text.replace(
                    "max_output=128", "max_output=128\nvision_input=true"
                )
            config.write_text(text)
            inputs = owner.svc.data / "media-inputs"
            inputs.mkdir(exist_ok=True)
            source_bytes = (
                b"#!AMR\noriginal opaque audio input"
                if kind == "converted-audio"
                else raw
            )
            (inputs / "selected.bin").write_bytes(source_bytes)
            with owner.svc.running():
                owner.until((folder / "ready.json").exists)
                frame = {
                    "source": "aux-" + kind,
                    "account": owner.user["account"],
                    "bot_stage": owner.bot["assignment"],
                    "kind": "message",
                    "observed": 20,
                    "occurred": 20,
                    "segments": [
                        {
                            "type": "media",
                            "kind": "image" if kind == "image" else "record",
                            "reference": "original-media",
                        }
                    ],
                }
                src = owner.op("ingress", "record", json.dumps(frame), "--accept")[
                    "event"
                ]
                execution = owner.op("ingress", "claim", src)["execution"]
                cached = owner.op(
                    "media",
                    "cache-file",
                    execution,
                    owner.actor,
                    src,
                    0,
                    owner.bot["assignment"],
                    "original-media",
                    "selected.bin",
                )
                ctx.equal(
                    "original selected media cached " + kind,
                    cached["digest"],
                    hashlib.sha256(source_bytes).hexdigest(),
                )
                with ThreadPoolExecutor(max_workers=1) as pool:
                    operation = pool.submit(
                        owner.op,
                        "host",
                        "process",
                        "aux",
                        execution,
                        owner.actor,
                        "process-" + kind,
                        json.dumps({"event": src, "slot": 0}),
                        purpose,
                    )
                    owner.until(lambda: list(folder.glob("call-*.json")))
                    call_frame = json.loads(
                        next(folder.glob("call-*.json")).read_bytes()
                    )
                    handle = call_frame["params"]["handle"]
                    peer = Peer(owner, folder)
                    params = {
                        "origin": {"handle": handle},
                        "call_id": "media-model",
                        "route": "jev",
                        "purpose": purpose,
                        "text": "read only the selected original",
                        "selected_media": True,
                    }
                    if kind == "converted-audio":
                        published = peer.call(
                            "media.publish",
                            {
                                "handle": handle,
                                "key": "converted",
                                "filename": "converted.wav",
                                "kind": "file",
                                "mime": "audio/wav",
                                "digest": hashlib.sha256(raw).hexdigest(),
                                "size": len(raw),
                                "offset": 0,
                                "bytes": base64.b64encode(raw).decode(),
                            },
                        )
                        reference = {
                            "artifact_id": published["artifact_id"],
                            "version": published["version"],
                        }
                        params.pop("selected_media")
                        params["media"] = reference
                        peer.call(
                            "llm.complete",
                            {**params, "media": {**reference, "version": "0" * 64}},
                            False,
                        )
                    request = peer.submit("llm.complete", params)
                    wire, q = upstream.receive()
                    body = json.loads(wire)
                    part = body["messages"][-1]["content"][0]
                    encoded = (
                        part["image_url"]["url"].split(",", 1)[1]
                        if kind == "image"
                        else part["input_audio"]["data"]
                    )
                    ctx.equal(
                        "actual model receives exact original media " + kind,
                        base64.b64decode(encoded),
                        raw,
                    )
                    reply(
                        q,
                        "authorized original transcript"
                        if kind == "audio"
                        else "authorized image reading",
                    )
                    answer = peer.receive(request)
                    ctx.equal(
                        "selected media shares original root " + kind,
                        owner.rows("SELECT root FROM model_requests"),
                        [(execution,)],
                    )
                    (folder / ("finish-" + handle + ".json")).write_text(
                        json.dumps(
                            {
                                "ok": True,
                                "data": {"text": "authorized original transcript"},
                            }
                        )
                    )
                    try:
                        result = operation.result()
                    except Exception:
                        ctx.write("aux-media-failure-" + kind + ".json", {
                            "calls":owner.rows("SELECT handle,method,state,result FROM host_calls"),
                            "operations":owner.rows("SELECT operation_id,state,result FROM operations"),
                            "finish_present":(folder / ("finish-" + handle + ".json")).exists(),
                        })
                        raise
                    ctx.equal(
                        "original selected processing succeeds " + kind,
                        result["state"],
                        "confirmed",
                    )
                    if kind != "image":
                        ctx.equal(
                            "authorized transcript binds original event",
                            owner.rows("SELECT source,text FROM history_transcripts"),
                            [(src, "authorized original transcript")],
                        )
                    proofs.append(
                        {
                            "kind": kind,
                            "source": src,
                            "digest": cached["digest"],
                            "receipt": answer,
                        }
                    )
    return proofs


def changed_profile(ctx, sdk):
    owner = Owner(ctx, "aux-changed-profile", 29)
    with owner.svc.running():
        entry = owner.op(
            "entry", "private", owner.user["assignment"], owner.bot["assignment"]
        )["entry"]
    folder = ctx.working / "aux-changed-profile-peer"
    with Recorder() as upstream:
        install(ctx, owner, folder, sdk, upstream, entry)
        with ThreadPoolExecutor(max_workers=1) as pool:
            death = pool.submit(
                kill_writer,
                ctx,
                owner.svc.data,
                "work_delivery_crash_child",
                "aux-reserved-before-dispatch",
                {},
                sample="changed-profile",
            )
            owner.until((folder / "ready.json").exists)
            src = event(owner, "changed-profile")
            owner.until((folder / ("event-" + str(src) + ".json")).exists)
            peer = Peer(owner, folder)
            params = {
                "origin": {"event": src},
                "call_id": "old-profile",
                "route": "jev",
                "purpose": "review",
                "text": "keep original paid contract",
            }
            key = peer.submit("llm.complete", params)
            death.result()
        original = owner.rows("SELECT id,logical FROM host_aux_requests")
        config = owner.svc.data / "yuki.toml"
        config.write_text(
            config.read_text().replace(
                'model="aux-fixture"', 'model="different-profile"'
            )
        )
        with owner.svc.running():
            peer.receive(key, False)
            ctx.equal(
                "changed Profile retains original request",
                owner.rows("SELECT id,logical FROM host_aux_requests"),
                original,
            )
            ctx.equal(
                "changed Profile retains original root reservation",
                owner.rows("SELECT reserved_requests FROM root_budgets"),
                [(1,)],
            )
            ctx.equal(
                "changed Profile has no physical attempt",
                owner.rows("SELECT count(*) FROM model_attempts"),
                [(0,)],
            )
            ctx.check(
                "changed Profile cannot silently dispatch old logical identity",
                upstream.wires.empty(),
            )
        return {"request": original, "dispatches": 0}
