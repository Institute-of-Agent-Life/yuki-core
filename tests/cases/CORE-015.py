"""Drive a paid tool chain, read the same private artifact after restart, and revoke its original actor."""

import json
import hashlib
import os
import subprocess
import sys
import sqlite3
import time
from contextlib import closing
from tests.harness.service import Service, previous_schema
from tests.peers.chat.recorder import Recorder


def run(ctx):
    original_views=json.loads((ctx.root/"fixtures/tool-results/v4-tool-views.json").read_bytes())
    views = []
    barriers = []
    evidence = []
    text = "雪🥝𠮷ß" * 80 + "漢\\\n" * 32
    sequence = ["x" * 1500, "b", "c"]

    def call(name, id, args):
        return {
            "type": "function",
            "id": id,
            "function": {
                "name": name,
                "arguments": json.dumps(args, ensure_ascii=False),
            },
        }

    def answer(calls=None):
        return {
            "choices": [
                {
                    "finish_reason": "tool_calls" if calls else "stop",
                    "message": {
                        "role": "assistant",
                        "content": None if calls else "internal",
                        **({"tool_calls": calls} if calls else {}),
                    },
                }
            ]
        }

    for mode in ("direct", "code", "quota"):
        svc = Service(ctx, ctx.working / mode)
        previous_schema(ctx, svc.data, 11)
        (svc.data / "SOUL.md").write_bytes(
            (ctx.root / "character/SOUL.md").read_bytes()
        )
        ctx.check(
            "previous database upgrades additively",
            svc.command("migrate")["schema_version"] >= 12,
        )

        def op(*args, ok=True):
            return svc.command("operator", *map(str, args), accepted=ok)

        def rows(sql, *args):
            with closing(
                sqlite3.connect(
                    (svc.data / "core.sqlite").as_uri() + "?mode=ro", uri=True
                )
            ) as db:
                return db.execute(sql, args).fetchall()

        def wait(run):
            until = time.monotonic() + 10
            while rows("SELECT state FROM executions WHERE id=?", run) != [
                ("completed",)
            ]:
                if time.monotonic() > until:
                    raise AssertionError("tool turn did not complete")
                time.sleep(0.03)

        with svc.running():
            if mode == "direct":
                ctx.write("sqlite-build.json", op("database-info"))
            actor = "00000000-0000-4000-8000-000000000015"
            op("person", "create", actor)
            a = op(
                "account",
                "open",
                "qq",
                "tools",
                "200",
                1,
                "--person",
                actor,
                "--start",
                1,
            )["assignment"]
            b = op("account", "open", "qq", "tools", "100", 1, "--start", 1)[
                "assignment"
            ]
            entry = op("entry", "private", a, b)["entry"]
        with Recorder() as peer:
            config = f'\n[main]\nendpoint="{peer.url}"\nmodel="offline"\nname="Yuki"\nmerge_ms=0\nroot_slots=7\nmax_input_units=500000\nmax_output=128\nresult_page_budget={32000 if mode == "code" else 680}\nresult_storage_bytes={0 if mode == "quota" else 65536}\ncode_projection={str(mode == "code").lower()}\n'
            with (svc.data / "yuki.toml").open("a") as f:
                f.write(config)

            def trigger(key):
                return op("main", "trigger", "origin", key, json.dumps({"text": key}))[
                    "execution"
                ]

            def receive():
                raw, q = peer.receive()
                return json.loads(raw), q

            def receipts(w):
                return [
                    json.loads(m["content"])
                    for m in w["messages"]
                    if m["role"] == "tool"
                ]

            def page(run, h, path, offset=0, limit=8000, ok=True):
                return op(
                    "tools",
                    "read-artifact",
                    run,
                    "origin",
                    json.dumps(
                        {
                            "handle": h["handle"],
                            "version": h["version"],
                            "path": path,
                            "offset": offset,
                            "limit": limit,
                        }
                    ),
                    ok=ok,
                )

            with svc.running():
                op("main", "register", "plugin", "origin", entry)
                run = trigger("open")
                wire, q = receive()
                view = op("tools", "views", run, "origin")
                views.append(view)
                ctx.equal(
                    "actual paid declarations match frozen direct view",
                    wire["tools"],
                    view["direct"],
                )
                names = [t["function"]["name"] for t in wire["tools"]]
                ctx.equal("tool order is deterministic", names, sorted(names))
                ctx.check(
                    "direct loads no VM and code projects fixed real tools",
                    "execute_code" not in names
                    and (
                        ("get" not in names and "lookup_tools" in names)
                        if mode == "code"
                        else ("get" in names and "lookup_tools" not in names)
                    ),
                )
                for original in original_views['declarations']:
                    if original['name'] in names:
                        ctx.equal('native Main preserves original '+original['name']+' declaration',next(t['function'] for t in wire['tools'] if t['function']['name']==original['name']),original)
                if mode == 'code':
                    ctx.equal('all implemented core tools follow the original fixed Code projection',names,sorted(set(view['executable']) & set(original_views['direct_names'])))
                    calls = [
                        call("lookup_tools", "a", {"name": "history_directory"}),
                        call("lookup_tools", "b", {"query": "time"}),
                        call("lookup_tools", "c", {"name": "absent"}),
                        call("time_get_current", "now", {}),
                    ]
                    q.put((answer(calls), 0))
                    w, q = receive()
                    r = receipts(w)
                    ctx.check(
                        "hidden tool appears only in discovery with its original schema",
                        r[0]["result"]["data"]["direct"] is False
                        and r[0]["result"]["data"]["script_name"] == "yuki_history_directory",
                    )
                    ctx.check(
                        "paged lookup omits schemas",
                        all("parameters" not in i for i in r[1]["result"]["data"]["tools"]),
                    )
                    ctx.equal('lookup retains the original frozen manifest identity',r[0]['result']['manifest_revision'],view['revision'])
                    ctx.equal('unknown lookup preserves original nonexecution error',r[2]['result'],{'ok':False,'executed':False,'error':'unknown_capability'})
                    directory = {item["name"]: item for item in r[1]["result"]["data"]["tools"]}
                    ctx.check(
                        "long wait tool uses a short directory summary",
                        len(directory["task_control"]["description"]) <= 240
                        and "time_get_current" in directory,
                    )
                    ctx.equal(
                        "one denied call does not lose independent result",
                        [r[2]["state"], r[3]["result"]["timezone"]],
                        ["failed", "Asia/Shanghai"],
                    )
                    q.put(
                        (
                            answer(
                                [
                                    call(
                                        "lookup_tools",
                                        "alias",
                                        {"name": "history_directory"},
                                    ),
                                    call("time_get_current", "another-time", {}),
                                ]
                            ),
                            0,
                        )
                    )
                    w, q = receive()
                    ctx.equal(
                        "only frozen lookup reuses its earlier representative",
                        rows(
                            "SELECT operation,representative FROM tool_aliases WHERE execution=?",
                            run,
                        ),
                        [(receipts(w)[-2]["id"], r[0]["id"])],
                    )
                    q.put((answer(), 0))
                    wait(run)
                    continue
                args = {"target": "k", "value": {"text": text, "array": sequence}}
                calls = [
                    call("get", "before", {"target": "k"}),
                    call("time_get_current", "first-clock", {}),
                    call("put", "write", args),
                    call("get", "after", {"target": "k"}),
                ]
                q.put((answer(calls), 0))
                w, q = receive()
                r = receipts(w)
                ctx.equal(
                    "paired IDs remain in original order",
                    [m["tool_call_id"] for m in w["messages"] if m["role"] == "tool"],
                    ["before", "first-clock", "write", "after"],
                )
                ctx.equal(
                    "first read cannot prefetch later write", r[0]["result"], None
                )
                ctx.check(
                    "last dynamic read observed actual written version",
                    r[-1]["result"]["truncated"],
                )
                ctx.equal(
                    "no cache alias for dynamic reads and time",
                    rows("SELECT count(*) FROM tool_aliases WHERE execution=?", run),
                    [(0,)],
                )
                barriers.append(
                    {
                        "mode": mode,
                        "original_ids": [x["id"] for x in r],
                        "before": r[0],
                        "after": r[-1],
                    }
                )
                ctx.check(
                    "serialized complete receipt respects page budget",
                    all(
                        len(m["content"]) <= 680
                        for m in w["messages"]
                        if m["role"] == "tool"
                    ),
                )
                if mode == "quota":
                    ctx.check(
                        "full store exposes a preview without handle",
                        "artifact" not in r[-1]["result"]
                        and bool(r[-1]["result"]["preview"])
                        and r[-1]["result"]["reason"] == "storage_full",
                    )
                    ctx.equal(
                        "no artifact was promised",
                        rows("SELECT count(*) FROM tool_artifacts"),
                        [(0,)],
                    )
                    q.put((answer(), 0))
                    wait(run)
                    evidence.append(r[-1])
                    continue
                h = r[-1]["result"]["artifact"]
                path = "/result/value/value/text"
                parts = []
                ofs = 0
                while True:
                    p = page(run, h, path, ofs, 32)
                    parts.append(p)
                    if p["next_offset"] is None:
                        break
                    ofs = p["next_offset"]
                ctx.equal(
                    "Unicode pages reconstruct the original string",
                    "".join(p["value"] for p in parts),
                    text,
                )
                ctx.equal(
                    "equal first and final page lengths do not predict continuation",
                    (parts[0]["length"], parts[-1]["length"], parts[-1]["next_offset"]),
                    (32, 32, None),
                )
                arr = page(run, h, "/result/value/value/array")
                ctx.equal(
                    "first oversized array item remains whole",
                    [arr["value"], arr["next_offset"], arr["reason"]],
                    [[], 0, "item_exceeds_budget"],
                )
                one = page(run, h, "/result/value/value/array/0", limit=10)
                ctx.equal(
                    "nested path resumes the original oversize scalar",
                    one["value"],
                    "x" * 10,
                )
                q.put(
                    (
                        answer(
                            [
                                call(
                                    "read_tool_artifact",
                                    "follow",
                                    {"handle":h["handle"], "path":["value","value","text"], "offset":32,"limit":32},
                                ),
                                call(
                                    "update_short_state",
                                    "remember",
                                    {"slot": 1, "text": " transient marker "},
                                ),
                            ]
                        ),
                        0,
                    )
                )
                w, q = receive()
                tail = receipts(w)[-2:]
                ctx.equal(
                    "original read-more parameters have no internal version dependency",
                    r[-1]["result"]["read_more"],
                    {"handle":h["handle"],"operation":"get","path":[],"offset":0},
                )
                def original_page(operation, **extra):
                    return op("tools","read-artifact",run,"origin",json.dumps({"handle":h["handle"],"operation":operation,**extra}))
                structure=original_page("inspect",path=["value","value"],limit=1)
                ctx.equal("original inspect returns complete first child shape",structure["children"],[{"key":"array","type":"array","length":3}])
                found=original_page("search",path=["value","value"],query="b")
                ctx.equal("original search returns whole matching scalar and real array path",found["matches"],[{"path":["value","value","array",1],"matched_path":["value","value","array",1],"value":"b"}])
                raw_body=rows("SELECT body FROM tool_artifacts WHERE handle=?",h["handle"])[0][0]
                raw_page=original_page("text",offset=1,limit=12)
                ctx.equal("text preserves raw archive character offsets",raw_page["content"],raw_body[1:13])
                ctx.equal("JSON artifacts cannot masquerade as native image",original_page("image")["error_code"],"artifact_not_image")
                ctx.equal("artifact declaration matches fixed original wire",next(t["function"] for t in wire["tools"] if t["function"]["name"]=="read_tool_artifact"),json.loads((ctx.root/"fixtures/tool-results/v4-original-declaration.json").read_text(encoding="utf-8"))["declaration"])
                ctx.equal(
                    "native model continuation reads same original version",
                    tail[0]["result"]["value"],
                    parts[1]["value"],
                )
                ctx.equal(
                    "short-state write records its original content",
                    tail[1]["result"]["text"],
                    "transient marker",
                )
                q.put((answer(), 0))
                wait(run)
                first_expiry = rows(
                    "SELECT revision,expires FROM short_state WHERE slot=1"
                )
                again = trigger("next activation")
                v, q = receive()
                ctx.check(
                    "new activation sees live cross-turn state",
                    any("transient marker" in m["content"] for m in v["messages"]),
                )
                ctx.equal(
                    "reading cross-turn state never refreshes expiry",
                    rows("SELECT revision,expires FROM short_state WHERE slot=1"),
                    first_expiry,
                )
                q.put(
                    (
                        answer(
                            [
                                call(
                                    "update_short_state",
                                    "clear",
                                    {"slot": 1, "text": ""},
                                )
                            ]
                        ),
                        0,
                    )
                )
                v, q = receive()
                q.put((answer(), 0))
                wait(again)
                cleared = trigger("after clear")
                v, q = receive()
                ctx.check(
                    "cleared slot is absent from later activation",
                    all("transient marker" not in m["content"] for m in v["messages"]),
                )
                q.put((answer(), 0))
                wait(cleared)
            with svc.running():
                paid = rows("SELECT COUNT(*) FROM model_attempts")[0][0]
                ctx.equal(
                    "immutable artifact survives restart",
                    page(again, h, path, 0, 32)["value"],
                    parts[0]["value"],
                )
                op("main", "revoke", "origin")
                page(again, h, path, ok=False)
                ctx.equal(
                    "revoked continuation buys no model request",
                    rows("SELECT count(*) FROM model_attempts")[0][0],
                    paid,
                )
                evidence.append(
                    {
                        "handle": h,
                        "pages": parts,
                        "oversize": arr,
                        "retained_partial": rows(
                            "SELECT state FROM operations WHERE namespace=? AND operation_id=?",
                            run,
                            r[2]["id"],
                        ),
                    }
                )
    ctx.write("read-barriers.json", {"cases": {"H12": barriers}})
    ctx.write("frozen-views.json", {"cases": {"H09": views}})
    ctx.write('original-tool-views.json',{'original_source_commit':original_views['source_commit'],'oracle_sha256':hashlib.sha256((ctx.root/'fixtures/tool-results/v4-tool-views.json').read_bytes()).hexdigest(),'golden_cases':len(original_views['cases']),'native_views':views,'host_code_lookup_unchanged':True})
    from tests.harness.tool_call_isolation import run as isolated_calls
    ctx.write('tool-call-isolation.json',isolated_calls(ctx))
    built=subprocess.run([sys.executable,str(ctx.root/"ci/publish-contract.py"),"build","--out",str(ctx.output/"sdk-distributions"),"--verify-work",str(ctx.working/"artifact-sdk")],capture_output=True,text=True,timeout=120)
    ctx.write("artifact-sdk-build.json",{"exit_code":built.returncode,"stderr":built.stderr})
    ctx.equal("artifact candidate has independently verified SDK packages",built.returncode,0)
    binary=ctx.root/"target/debug"/("yuki-core.exe" if os.name=="nt" else "yuki-core")
    return {
        "binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),
        "host_idl_sha256":hashlib.sha256((ctx.root/"sdk/rust/src/host-preview.json").read_bytes()).hexdigest(),
        "artifact_declaration_sha256":hashlib.sha256((ctx.root/"fixtures/tool-results/v4-original-declaration.json").read_bytes()).hexdigest(),
        "oracle_sha256":hashlib.sha256((ctx.root/"fixtures/tool-results/v4-original-pages.json").read_bytes()).hexdigest(),
        "validated_operations":["get","inspect","search","text","image-json-refusal"],
        "real_extension_consumers":False,
        "cases": {"H15": evidence},
        "source": "original SQLite and Rust owner with physical loopback tool calls",
    }
