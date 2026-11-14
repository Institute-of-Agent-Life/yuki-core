"""Installed maintenance scopes exercise the original Rust writer and SDK boundary."""

import hashlib
import importlib.util
import json
import os
import sqlite3
import subprocess
import sys
from contextlib import closing
from tests.harness.compact import Owner, reply
from tests.peers.chat.recorder import Recorder


def run(ctx):
    spec = importlib.util.spec_from_file_location(
        "aux_fixture", ctx.root / "tests/cases/CORE-050.py"
    )
    aux = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(aux)
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
    ctx.equal(
        "both SDK packages expose the maintenance read contract", built.returncode, 0
    )
    o = Owner(ctx, "maintenance", 32)
    with o.svc.running():
        source = o.human("Original maintenance quote 035.")
        execution = o.op("ingress", "claim", source)["execution"]
        entry = o.rows("SELECT entry_id FROM events WHERE id=?", source)[0][0]
        edit = {
            "action": "add",
            "subject": {"kind": "person", "person": o.actor},
            "fact_key": "maintenance-fixture",
            "body": "preserve original quote",
            "evidence": [
                {
                    "kind": "event",
                    "id": source,
                    "quote": "Original maintenance quote 035.",
                }
            ],
        }
        fact = o.op(
            "memory", "edit", execution, o.actor, "initial-fact", json.dumps(edit)
        )["fact"]
    methods = json.loads(
        (ctx.root / "protocol/plugin/maintenance/methods.json").read_bytes()
    )["methods"]
    folders = {}
    for name, evidence, policy in [
        ("approved", True, True),
        ("fact_only", False, True),
        ("subscription", False, False),
    ]:
        folder = ctx.working / name
        folder.mkdir()
        folders[name] = folder
        allowed = [m for m in methods if evidence or m != "maintenance.evidence"]
        with (o.svc.data / "yuki.toml").open("a", encoding="utf-8") as f:
            f.write(
                "\n[[plugins]]\nid="
                + json.dumps(name)
                + "\nnamespace="
                + json.dumps(name)
                + "\nprogram="
                + json.dumps(sys.executable)
                + "\nargs="
                + json.dumps(
                    [
                        str(ctx.root / "tests/peers/aux-model/peer.py"),
                        str(folder),
                        str(sdk / "python-install"),
                    ]
                )
                + "\ngrants="
                + json.dumps(["forget.owner", *methods])
                + "\n"
            )
            if policy:
                f.write(
                    "\n[plugins.maintenance.scopes.original]\nactor="
                    + json.dumps(o.actor)
                    + "\nentry="
                    + json.dumps(entry)
                    + '\npurposes=["audit"]\nmethods='
                    + json.dumps(allowed)
                    + "\n"
                )
    params = {"scope": "original", "purpose": "audit"}
    with o.svc.running():
        for folder in folders.values():
            o.until((folder / "ready.json").exists)
        negotiated = sorted(
            m
            for m in json.loads((folders["approved"] / "ready.json").read_bytes())[
                "methods"
            ]
            if m.startswith("maintenance.")
        )
        ctx.equal(
            "actual installed SDK negotiates every maintenance method",
            negotiated,
            sorted(methods),
        )
        peers = {name: aux.Peer(o, folder) for name, folder in folders.items()}
        approved = peers["approved"]
        fact_only = peers["fact_only"]
        subscription = peers["subscription"]
        subscription.call("maintenance.history.page", params, False)
        approved.call("maintenance.history.page", {**params, "actor": o.actor}, False)
        approved.call(
            "maintenance.history.page", {**params, "purpose": "unapproved"}, False
        )
        history = approved.call("maintenance.history.page", params)
        row = history["items"][0]
        ctx.equal("history keeps original internal ID", row["event"], source)
        ctx.equal(
            "history keeps actual original text",
            row["text"],
            "Original maintenance quote 035.",
        )
        approved.call(
            "maintenance.history.page",
            {**params, "event": source, "version": "changed"},
            False,
        )
        narrow = fact_only.call("maintenance.fact.page", {**params, "fact": fact})[
            "items"
        ][0]
        ctx.check("installed maintenance projection excludes undeclared metadata", "metadata" not in narrow)
        ctx.check(
            "fact-only grant has no evidence quote or proof",
            not any(k in narrow for k in ["evidence", "readable_evidence", "proof"]),
        )
        fact_only.call("maintenance.evidence", {**params, "fact": fact}, False)
        fact_only.call("maintenance.tool.page", params, False)
        evidence = approved.call("maintenance.evidence", {**params, "fact": fact})[
            "items"
        ][0]
        ctx.equal(
            "explicit evidence grant returns original quote",
            evidence["evidence"][0]["quote"],
            "Original maintenance quote 035.",
        )
        ctx.equal(
            "fact readers retain source erasure obligations",
            o.rows(
                "SELECT DISTINCT owner FROM source_holds WHERE item=? ORDER BY owner",
                "maintenance-fact:" + fact,
            ),
            [("approved",), ("fact_only",)],
        )
        before = o.rows("SELECT count(*) FROM model_requests")
        scan = approved.call("maintenance.audit", {**params, "fact": fact})
        ctx.equal(
            "audit is unpaid", o.rows("SELECT count(*) FROM model_requests"), before
        )
        ctx.equal(
            "audit identifies empty semantic profile coverage",
            scan["items"][0]["vector_profiles"],
            [],
        )
        o.op("privacy", "erase", "maintenance-erasure", o.actor, json.dumps([source]))
        for name in ("approved", "fact_only"):
            o.until(lambda: not o.op("privacy", "pending", name)["obligations"])
        approved.call("maintenance.history.page", {**params, "event": source}, False)
        fact_only.call("maintenance.fact.page", {**params, "fact": fact}, False)
        read_scope = {
            "history": history,
            "fact": narrow,
            "evidence": evidence,
            "audit": scan,
        }
    paid = paid_previews(ctx, sdk, aux)
    vectors = reembed(ctx, sdk, aux)
    quality_erasure = retained_quality(ctx, sdk, aux)
    historic = historical(ctx, sdk, aux)
    fact_plans = fact_inputs(ctx, sdk, aux)
    tool_map = tool_receipts(ctx, sdk, aux)
    tool_snapshot = tool_snapshot_sources(ctx, sdk, aux)
    return {
        "tool_receipts": tool_map,
        "tool_snapshot_sources": tool_snapshot,
        "reflection_window": reflection_context_window(ctx, sdk, aux),
        "paid_previews": paid,
        "reembed": vectors,
        "quality_erasure": quality_erasure,
        "historical": historic,
        "fact_inputs": fact_plans,
        "read_scope": read_scope,
        "implementation_stage": "scoped reads, paid reviewed writes, archived sources, reembed, source audit",
        "host_idl_sha256": hashlib.sha256(
            (ctx.root / "sdk/rust/src/host-preview.json").read_bytes()
        ).hexdigest(),
        "maintenance_schema_sha256": hashlib.sha256(
            (ctx.root / "protocol/plugin/maintenance/methods.json").read_bytes()
        ).hexdigest(),
        "negotiated_maintenance_methods": negotiated,
        "binary_sha256": hashlib.sha256(
            (
                ctx.root
                / "target/debug"
                / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
            ).read_bytes()
        ).hexdigest(),
    }


def tool_snapshot_sources(ctx, sdk, aux):
    o = Owner(ctx, "maintenance-snapshot", 60)
    with o.svc.running():
        written = o.human("Original value producer.")
        read = o.human("Read the saved value.")
        producer = o.op("ingress", "claim", written)["execution"]
        consumer = o.op("ingress", "claim", read)["execution"]
        entry = o.rows("SELECT entry_id FROM events WHERE id=?", read)[0][0]
        original = {"value": "Retained original value."}
        o.op("effect", "intent", producer, "write-value", o.actor, "put", "saved",
             json.dumps(original))
        o.op("effect", "local", producer, "write-value", o.actor)
        o.op("effect", "intent", consumer, "read-value", o.actor, "get", "saved", "{}")
        receipt = o.op("effect", "local", consumer, "read-value", o.actor)
        ctx.equal("actual get result includes the earlier value", receipt["result"]["value"], original)
        group = '["qq","compact","900"]'
        other_entry = o.op("entry", "group", group)["entry"]
        other_event = o.human("Check the other entry.", group)
        other = o.op("ingress", "claim", other_event)["execution"]
        o.op("effect", "intent", other, "read-other", o.actor, "get", "saved", "{}")
        o.op("effect", "local", other, "read-other", o.actor)

    folder = ctx.working / "maintenance-snapshot-peer"
    folder.mkdir()
    methods = ["maintenance.tool.page", "maintenance.evidence", "maintenance.dependencies",
               "maintenance.history.page"]
    args = [str(ctx.root / "tests/peers/aux-model/peer.py"), str(folder),
            str(sdk / "python-install")]
    with (o.svc.data / "yuki.toml").open("a", encoding="utf-8") as config:
        config.write(
            '\n[[plugins]]\nid="snapshot-reader"\nnamespace="snapshot-reader"\nprogram='
            + json.dumps(sys.executable) + "\nargs=" + json.dumps(args)
            + "\ngrants=" + json.dumps(["forget.owner", *methods])
            + "\n[plugins.maintenance.scopes.original]\nactor=" + json.dumps(o.actor)
            + "\nentry=" + json.dumps(entry) + '\npurposes=["reflect"]\nmethods='
            + json.dumps(methods)
            + "\n[plugins.maintenance.scopes.group]\nactor=" + json.dumps(o.actor)
            + "\nentry=" + json.dumps(other_entry) + '\npurposes=["reflect"]\nmethods='
            + json.dumps(methods) + "\n"
        )
    peer = aux.Peer(o, folder)
    params = {"scope": "original", "purpose": "reflect"}
    query = {**params, "execution": consumer}
    with o.svc.running():
        o.until((folder / "ready.json").exists)
        snapshot, = peer.call("maintenance.tool.page", query)["items"]
        by_event = peer.call("maintenance.tool.page", {**params, "event": read})
        ctx.equal("trigger event selects the same original receipt", by_event["items"], [snapshot])
        ctx.equal("receipt cursor resumes within the original event",
                  peer.call("maintenance.tool.page", {**params, "event": read, "after": by_event["next"]}),
                  {"items": [], "next": snapshot["receipt"]})
        ctx.equal("event and execution selectors intersect",
                  peer.call("maintenance.tool.page", {**query, "event": written})["items"], [])
        ctx.equal("foreign event exposes no tool result",
                  peer.call("maintenance.tool.page", {**params, "event": other_event})["items"], [])
        context_query = {**params, "before": read}
        context = peer.call("maintenance.history.page", context_query)
        ctx.equal("reflection context uses the original entry and prior events",
                  [item["event"] for item in context["items"]], [written])
        ctx.equal("reflection context leaves its cursor at the anchor", context["next_event"], read)
        ctx.equal("reflection context cannot cross an installed entry",
                  peer.call("maintenance.history.page", {**params, "before": other_event}, ok=False)["error"]["code"], -32003)
        ctx.equal("reflection context cannot mix with a forward or explicit read",
                  [peer.call("maintenance.history.page", {**context_query, selector: read}, ok=False)["error"]["code"]
                   for selector in ("after", "event")], [-32003, -32003])
        ctx.equal("group scope cannot use a private trigger",
                  peer.call("maintenance.tool.page", {**params, "scope": "group", "event": read})["items"], [])
        ctx.equal("snapshot keeps both real original events", snapshot["source_ids"], [written, read])
        ctx.equal("snapshot source count is exact", snapshot["source_count"], 2)
        refs = peer.call(
            "maintenance.dependencies",
            {**params, "kind": "tool", "id": str(snapshot["receipt"]), "source_version": snapshot["version"]},
        )
        ctx.equal("dependency page agrees with the original tool receipt", refs["sources"], [written, read])
        ctx.equal("plugin retains obligations for both original events",
                  o.rows("SELECT source FROM source_holds WHERE owner='snapshot-reader' AND item=? ORDER BY source",
                         f"maintenance-tool:{snapshot['receipt']}"), [(written,), (read,)])
        ctx.equal("group scope cannot project a snapshot of a private value",
                  peer.call("maintenance.tool.page", {**params, "scope": "group", "execution": other})["items"],
                  [])
        o.op("privacy", "erase", "erase-only-value-source", o.actor, json.dumps([written]))
        o.until(lambda: not o.op("privacy", "pending", "snapshot-reader")["obligations"])
        ctx.equal("erased value source makes its snapshot unavailable",
                  peer.call("maintenance.tool.page", query)["items"], [])
        ctx.equal("event selection also fences the erased dependency",
                  peer.call("maintenance.tool.page", {**params, "event": read})["items"], [])
        ctx.equal("reflection context excludes an erased original source",
                  peer.call("maintenance.history.page", context_query), {"items": [], "next_event": read})
        ctx.equal("clearing the value keeps the separate original read event",
                  o.rows("SELECT source FROM invalidated_sources ORDER BY source"), [(written,)])
    return {"write_event": written, "read_event": read, "source_ids": refs["sources"],
            "original_receipt": snapshot["receipt"], "event_page": by_event,
            "reflection_context": context,
            "invalidated_source": written}


def reflection_context_window(ctx, sdk, aux):
    o = Owner(ctx, "reflection-window", 60)
    with o.svc.running():
        old = o.human("Earlier reflection generation.")
        entry = o.rows("SELECT entry_id FROM events WHERE id=?", old)[0][0]
    folder = ctx.working / "reflection-window-peer"
    folder.mkdir()
    args = [str(ctx.root / "tests/peers/aux-model/peer.py"), str(folder),
            str(sdk / "python-install")]
    with (o.svc.data / "yuki.toml").open("a", encoding="utf-8") as config:
        config.write(
            '\n[[plugins]]\nid="reflection-window"\nnamespace="reflection-window"\nprogram='
            + json.dumps(sys.executable) + "\nargs=" + json.dumps(args)
            + '\ngrants=["forget.owner","maintenance.history.page"]'
            + "\n[plugins.maintenance.scopes.original]\nactor=" + json.dumps(o.actor)
            + "\nentry=" + json.dumps(entry) + '\npurposes=["reflect"]'
            + '\nmethods=["maintenance.history.page"]\n'
        )
    peer = aux.Peer(o, folder)

    def previous(anchor):
        page = peer.call("maintenance.history.page",
                         {"scope": "original", "purpose": "reflect", "before": anchor})
        ctx.equal("reflection context leaves the original anchor intact", page["next_event"], anchor)
        return [item["event"] for item in page["items"]]

    with Recorder() as http:
        o.config(http)
        with o.svc.running():
            o.until((folder / "ready.json").exists)

            def settle(event):
                _, answer = o.wire(http)
                reply(answer, "Recorded.")
                o.finished(event)

            settle(old)
            old_anchor = o.human("Another message in the original generation.")
            settle(old_anchor)
            ctx.equal("the original generation has its prior message", previous(old_anchor), [old])
            reset = o.human("/ai new")
            o.until(lambda: o.rows("SELECT event FROM main_controls WHERE event=?", reset))
            first = o.human("First message after the reset.")
            settle(first)
            ctx.equal("a reset starts an empty reflection context", previous(first), [])
            help_event = o.human("/ai help")
            o.until(lambda: o.rows("SELECT event FROM main_controls WHERE event=?", help_event))
            next_events = []
            for index in range(5):
                event = o.human(f"New-generation reflection message {index}.")
                settle(event)
                next_events.append(event)
            ctx.equal("control input is absent from the new context",
                      previous(next_events[0]), [first])
            context = previous(next_events[-1])
            ctx.equal("the last four events stay in order within the generation",
                      context, next_events[:-1])
            ctx.equal("an original earlier anchor retains its own context", previous(old_anchor), [old])
    return {"reset": reset, "first": first, "control": help_event,
            "anchor": next_events[-1], "context": context}


def paid_previews(ctx, sdk, aux):
    from tests.peers.chat.recorder import Recorder
    from tests.harness.compact import reply

    o = Owner(ctx, "maintenance-paid", 32)
    quote = "Paid source remains original."
    with o.svc.running():
        source = o.human(quote)
        entry = o.rows("SELECT entry_id FROM events WHERE id=?", source)[0][0]
        extra = o.human("Separate empty review source.")
    folder = ctx.working / "paid-maintainer"
    folder.mkdir()
    methods = json.loads(
        (ctx.root / "protocol/plugin/maintenance/methods.json").read_bytes()
    )["methods"]
    params = {"scope": "original", "purpose": "review"}
    with Recorder() as http:
        with (o.svc.data / "yuki.toml").open("a", encoding="utf-8") as f:
            f.write(
                '\n[[plugins]]\nid="maintainer"\nnamespace="maintainer"\nprogram='
                + json.dumps(sys.executable)
                + "\nargs="
                + json.dumps(
                    [
                        str(ctx.root / "tests/peers/aux-model/peer.py"),
                        str(folder),
                        str(sdk / "python-install"),
                    ]
                )
                + "\ngrants="
                + json.dumps(["forget.owner", "llm.complete", "llm.result", *methods])
                + "\n"
            )
            f.write(
                "\n[plugins.maintenance.scopes.original]\nactor="
                + json.dumps(o.actor)
                + "\nentry="
                + json.dumps(entry)
                + '\npurposes=["review"]\nroutes=["review"]\nmethods='
                + json.dumps(list(methods))
                + '\n[plugins.maintenance.scopes.original.write_subject]\nkind="person"\nperson='
                + json.dumps(o.actor)
                + "\n"
            )
            f.write(
                '\n[plugins.aux_routes.review]\npurposes=["review"]\norigins=["event"]\nentries='
                + json.dumps([entry])
                + "\n[plugins.aux_routes.review.profile]\nendpoint="
                + json.dumps(http.url)
                + '\nmodel="fixture"\nname="Reviewer"\nmerge_ms=0\nroot_slots=1\nmax_input_units=1000000\nmax_output=1024\n'
            )
        p = aux.Peer(o, folder)
        task = {
            "mode": "text_json",
            "schema": {
                "type": "object",
                "properties": {"actions": {"type": "array"}},
                "required": ["actions"],
                "additionalProperties": False,
            },
            "validation_retries": 0,
        }

        def purchase(event, key, actions):
            ref = p.call("maintenance.history.page", {**params, "event": event})[
                "items"
            ][0]
            job = p.submit(
                "llm.complete",
                {
                    "origin": {"event": event},
                    "call_id": key,
                    "route": "review",
                    "purpose": "review",
                    "text": "Review only this original source",
                    "task": task,
                },
            )
            _, q = o.wire(http)
            reply(q, json.dumps({"actions": actions}))
            receipt = p.receive(job)
            ctx.equal("paid structured attempt is saved", receipt["state"], "response")
            preview = {
                **params,
                "key": key,
                "request": receipt["request"],
                "format": "edits",
                "sources": [
                    {"event": event, "version": ref["version"], "role": "primary"}
                ],
            }
            return preview, p.call("maintenance.extract.preview", preview)

        with o.svc.running():
            o.until((folder / "ready.json").exists)
            actions = [
                {
                    "action": "add",
                    "subject": {"kind": "person", "person": o.actor},
                    "fact_key": "approved-" + str(i),
                    "body": "reviewed item " + str(i),
                    "evidence": [{"kind": "event", "id": source, "quote": quote}],
                }
                for i in range(2)
            ]
            preview, ready = purchase(source, "two-proposals", actions)
            ctx.equal(
                "preview dry-run writes no facts",
                o.rows("SELECT count(*) FROM memory_facts"),
                [(0,)],
            )
            ctx.equal(
                "preview dry-run writes no memory effects",
                o.rows("SELECT count(*) FROM memory_operations"),
                [(0,)],
            )
            first = {
                **params,
                "preview": ready["preview"],
                "operation": "approve-one",
                "approved": [1],
            }
            applied = p.call("maintenance.extract.apply", first)
            ctx.equal(
                "approval applies only original selected ordinal",
                [v["ordinal"] for v in applied["applied"]],
                [1],
            )
            ctx.equal(
                "unapproved item remains outside formal memory",
                o.rows("SELECT fact_key FROM memory_facts"),
                [("approved-1",)],
            )
            ctx.equal(
                "repeated apply returns original receipt",
                p.call("maintenance.extract.apply", first),
                applied,
            )
            p.call("maintenance.extract.apply", {**first, "approved": [0]}, False)
            ctx.equal(
                "full original root still permits DB finalization",
                o.rows("SELECT reserved_requests,max_requests FROM root_budgets"),
                [(1, 1)],
            )
            original = p.call("maintenance.extract.preview", preview)
            ctx.equal(
                "paid preview resumes without repurchase",
                original["preview"],
                ready["preview"],
            )
            ctx.check("same preview has no physical model request", http.wires.empty())
            empty_request, empty = purchase(extra, "empty-result", [])
            no_actions = {
                **params,
                "preview": empty["preview"],
                "operation": "empty-approval",
                "approved": [],
            }
            empty_result = p.call("maintenance.extract.apply", no_actions)
            ctx.equal(
                "empty approval records a real original receipt",
                empty_result["applied"],
                [],
            )
        with o.svc.running():
            ctx.equal(
                "cold original apply returns identical result",
                p.call("maintenance.extract.apply", first),
                applied,
            )
            ctx.equal(
                "cold empty apply returns identical result",
                p.call("maintenance.extract.apply", no_actions),
                empty_result,
            )
            ctx.equal(
                "cold preview uses original paid result",
                p.call("maintenance.extract.preview", preview)["preview"],
                ready["preview"],
            )
            ctx.check("cold receipt recovery issues no HTTP", http.wires.empty())
            o.op(
                "privacy",
                "erase",
                "paid-preview-erasure",
                o.actor,
                json.dumps([source]),
            )
            o.until(lambda: not o.op("privacy", "pending", "maintainer")["obligations"])
            p.call(
                "maintenance.extract.apply",
                {**first, "operation": "late-other-item", "approved": [0]},
                False,
            )
            p.call("maintenance.extract.preview", preview, False)
            ctx.equal(
                "old committed key remains only its receipt after erasure",
                p.call("maintenance.extract.apply", first),
                applied,
            )
            ctx.equal(
                "erased preview cannot restore its unapproved fact",
                o.rows("SELECT count(*) FROM memory_facts WHERE fact_key='approved-0'"),
                [(0,)],
            )
            return {
                "preview": ready["preview"],
                "applied": applied,
                "empty": empty_result,
                "root_budgets": o.rows(
                    "SELECT reserved_requests,max_requests FROM root_budgets"
                ),
                "physical_attempts": o.rows("SELECT count(*) FROM model_attempts"),
            }


def reembed(ctx, sdk, aux):
    from tests.peers.chat.recorder import Recorder

    o = Owner(ctx, "maintenance-vectors", 32)
    facts = []
    with o.svc.running():
        for i in range(3):
            text = "scoped vector source " + str(i)
            event = o.human(text)
            ex = o.op("ingress", "claim", event)["execution"]
            item = {
                "action": "add",
                "subject": {"kind": "person", "person": o.actor},
                "fact_key": "vector-" + str(i),
                "body": text,
                "evidence": [{"kind": "event", "id": event, "quote": text}],
            }
            fact = o.op(
                "memory", "edit", ex, o.actor, "create-" + str(i), json.dumps(item)
            )["fact"]
            facts.append((event, ex, fact))
        entry = o.rows("SELECT entry_id FROM events WHERE id=?", facts[0][0])[0][0]
    with closing(sqlite3.connect(o.svc.data / "core.sqlite")) as fixture, fixture:
        fixture.execute(
            "UPDATE memory_retrieval_fts SET content=? WHERE rowid=(SELECT number FROM memory_facts WHERE id=?)",
            ("outdated index fixture", facts[0][2]),
        )
        fixture.execute("DELETE FROM memory_evidence WHERE fact=?", (facts[1][2],))
        fixture.execute(
            "UPDATE memory_evidence SET source_version=? WHERE fact=?",
            ("0" * 64, facts[2][2]),
        )
    folder = ctx.working / "reembed-peer"
    folder.mkdir()
    methods = json.loads(
        (ctx.root / "protocol/plugin/maintenance/methods.json").read_bytes()
    )["methods"]
    params = {"scope": "original", "purpose": "audit"}
    with Recorder("/v1/embeddings") as http:
        with (o.svc.data / "yuki.toml").open("a", encoding="utf-8") as f:
            f.write(
                '\n[[plugins]]\nid="reembed"\nnamespace="reembed"\nprogram='
                + json.dumps(sys.executable)
                + "\nargs="
                + json.dumps(
                    [
                        str(ctx.root / "tests/peers/aux-model/peer.py"),
                        str(folder),
                        str(sdk / "python-install"),
                    ]
                )
                + "\ngrants="
                + json.dumps(["forget.owner", *methods])
                + "\n"
            )
            f.write(
                "\n[plugins.maintenance.scopes.original]\nactor="
                + json.dumps(o.actor)
                + "\nentry="
                + json.dumps(entry)
                + '\npurposes=["audit"]\nembedding_routes=["dense-a","dense-b"]\nmethods='
                + json.dumps(list(methods))
                + '\n[plugins.maintenance.scopes.original.write_subject]\nkind="person"\nperson='
                + json.dumps(o.actor)
                + "\n"
            )
            for route in ("dense-a", "dense-b"):
                f.write(
                    "\n[plugins.maintenance.embedding_routes."
                    + route
                    + ']\nprovider="openai_compatible"\nmodel='
                    + json.dumps(route)
                    + "\nbase_url="
                    + json.dumps(http.url.removesuffix("/embeddings"))
                    + "\ndimensions=2\n"
                )
        p = aux.Peer(o, folder)
        with o.svc.running():
            o.until((folder / "ready.json").exists)
            audit = p.call("maintenance.audit", params)
            audit_params = {**params, "integrity": True}
            audit_before = p.call("maintenance.audit", audit_params)
            quality = {item["fact"]: item["integrity"] for item in audit_before["items"]}
            ctx.equal("legacy audit omits optional integrity report",
                      ["integrity" in item for item in audit["items"]], [False, False, False])
            ctx.equal("audit observes the mismatched FTS row",
                      quality[facts[0][2]]["lexical"], "mismatch")
            ctx.equal("audit observes missing evidence without inventing it",
                      quality[facts[1][2]]["evidence_missing"], True)
            ctx.equal("audit checks event evidence against its original revision",
                      quality[facts[2][2]]["invalid_event_evidence"], 1)
            ctx.equal("audit states the number of source proofs it checked",
                      [quality[f[2]]["event_evidence_checked"] for f in facts], [1, 0, 1])
            ctx.equal("audit binds missing embeddings to both exact profiles",
                      [r["coverage"] for r in quality[facts[0][2]]["vectors"]], ["missing", "missing"])
            ctx.check("audit is content-free and does not buy a request",
                      "scoped vector source" not in str(audit_before) and http.wires.empty())
            routes = {r["route"]: r["profile"] for r in audit["embedding_routes"]}
            ctx.check(
                "same dimension models retain separate semantic profiles",
                routes["dense-a"] != routes["dense-b"],
            )
            request = {
                **params,
                "operation": "selected-one",
                "fact": facts[0][2],
                "revision": 1,
                "route": "dense-a",
                "profile": routes["dense-a"],
            }
            job = p.submit("maintenance.reembed", request)
            raw, q = http.receive()
            body = json.loads(raw)
            ctx.equal("manual reembed sends one approved fact", len(body["input"]), 1)
            ctx.check(
                "manual reembed excludes unrelated source",
                "source 1" not in str(body) and "source 2" not in str(body),
            )
            q.put(
                (
                    {
                        "data": [{"index": 0, "embedding": [1.0, 0.0]}],
                        "usage": {"prompt_tokens": 5},
                    },
                    0,
                )
            )
            receipt = p.receive(job)
            ctx.equal("original scoped vector stored", receipt["state"], "done")
            ctx.equal(
                "only approved subset has vectors",
                o.rows("SELECT fact FROM fact_vectors"),
                [(facts[0][2],)],
            )
            ctx.equal(
                "repeat operation retains original cost",
                p.call("maintenance.reembed", request),
                receipt,
            )
            p.call(
                "maintenance.reembed", {**request, "profile": routes["dense-b"]}, False
            )
            ctx.check("stale profile never makes HTTP", http.wires.empty())
            second = {
                **request,
                "operation": "same-dimension-new-model",
                "route": "dense-b",
                "profile": routes["dense-b"],
            }
            job = p.submit("maintenance.reembed", second)
            raw, q = http.receive()
            ctx.equal(
                "changed semantic model really calls configured model",
                json.loads(raw)["model"],
                "dense-b",
            )
            q.put(
                (
                    {
                        "data": [{"index": 0, "embedding": [0.0, 1.0]}],
                        "usage": {"prompt_tokens": 7},
                    },
                    0,
                )
            )
            other = p.receive(job)
            ctx.equal(
                "same dimension produces two separate profile rows",
                o.rows("SELECT count(*) FROM fact_vectors WHERE fact=?", facts[0][2]),
                [(2,)],
            )
            p.call(
                "maintenance.audit.apply",
                {
                    **params,
                    "operation": "stale-audit",
                    "fingerprint": audit["fingerprint"],
                    "approved": [facts[0][2]],
                },
                False,
            )
            fresh = p.call("maintenance.audit", audit_params)
            original_fts = o.rows(
                "SELECT rowid,content,memory_key,category FROM memory_retrieval_fts ORDER BY rowid"
            )
            repair = {
                **audit_params,
                "operation": "approved-fts",
                "fingerprint": fresh["fingerprint"],
                "approved": [facts[0][2]],
            }
            repaired = p.call("maintenance.audit.apply", repair)
            ctx.equal(
                "audit rebuild touches only approved semantic document",
                repaired["rebuilt"],
                [facts[0][2]],
            )
            revised_fts = o.rows("SELECT rowid,content,memory_key,category FROM memory_retrieval_fts ORDER BY rowid")
            expected_fts = [tuple("scoped vector source 0" if i == 1 and value == "outdated index fixture" else value
                                  for i, value in enumerate(row)) for row in original_fts]
            ctx.equal("scoped rebuild repairs only the approved index row", revised_fts, expected_fts)
            after = p.call("maintenance.audit", audit_params)
            refreshed = next(item["integrity"] for item in after["items"] if item["fact"] == facts[0][2])
            ctx.write("quality-coverage.json", {"before": audit_before, "after": after})
            ctx.equal("repaired lexical row has current coverage", refreshed["lexical"], "current")
            ctx.equal("two semantic profiles retain separate current coverage",
                      [r["coverage"] for r in refreshed["vectors"]], ["current", "current"])
            p.call("maintenance.audit.apply", {**repair, "operation": "stale-index-state"}, False)
            ctx.equal(
                "same audit key returns original write receipt",
                p.call("maintenance.audit.apply", repair),
                repaired,
            )
            ctx.check("audit rebuild performs no model request", http.wires.empty())
            late = {**request, "operation": "erase-in-flight", "fact": facts[1][2]}
            job = p.submit("maintenance.reembed", late)
            _, q = http.receive()
            o.op(
                "privacy",
                "erase",
                "maintenance-vector-erasure",
                o.actor,
                json.dumps([facts[1][0]]),
            )
            q.put(
                (
                    {
                        "data": [{"index": 0, "embedding": [1.0, 1.0]}],
                        "usage": {"prompt_tokens": 9},
                    },
                    0,
                )
            )
            p.receive(job, False)
            ctx.equal(
                "erased source has no late vector",
                o.rows("SELECT count(*) FROM fact_vectors WHERE fact=?", facts[1][2]),
                [(0,)],
            )
            ctx.check(
                "late erased vector retains known usage",
                any(
                    "9" in str(v)
                    for v in o.rows("SELECT usage_json FROM model_attempts")
                ),
            )
            original = o.rows("SELECT id FROM model_attempts ORDER BY rowid")
        with o.svc.running():
            ctx.equal(
                "cold reembed keeps exact receipt",
                p.call("maintenance.reembed", request),
                receipt,
            )
            ctx.equal(
                "cold reembed preserves original attempt IDs",
                o.rows("SELECT id FROM model_attempts ORDER BY rowid"),
                original,
            )
            ctx.check(
                "cold reembed does not buy replacement vectors", http.wires.empty()
            )
        with closing(sqlite3.connect(o.svc.data / "core.sqlite")) as fixture, fixture:
            fixture.execute(
                "INSERT INTO fact_vectors SELECT fact,profile,purpose,dimension,?,revision,vector "
                "FROM fact_vectors WHERE fact=? AND profile=?",
                ("0" * 64, facts[0][2], routes["dense-a"]),
            )
        with o.svc.running():
            checked = p.call("maintenance.audit", {**audit_params, "fact": facts[0][2]})
            ctx.equal("an exact vector still counts when a later stale row exists",
                      checked["items"][0]["integrity"]["vectors"][0]["coverage"], "current")
            ctx.check("vector inspection sends no physical request", http.wires.empty())
            return {
                "profiles": routes,
                "first": receipt,
                "second": other,
                "attempts": original,
                "integrity_before": audit_before,
                "integrity_after_rebuild": after,
            }


def retained_quality(ctx, sdk, aux):
    owner = Owner(ctx, "maintenance-quality-retention", 59)
    data = owner.svc.data
    facts = []
    with owner.svc.running():
        for label in ["erased", "unrelated"]:
            text = "original quality " + label
            source = owner.human(text)
            execution = owner.op("ingress", "claim", source)["execution"]
            edit = {
                "action": "add", "subject": {"kind": "person", "person": owner.actor},
                "fact_key": "quality-" + label, "body": text,
                "evidence": [{"kind": "event", "id": source, "quote": text}],
            }
            fact = owner.op("memory", "edit", execution, owner.actor, label, json.dumps(edit))["fact"]
            facts.append((source, fact))
        entry = owner.rows("SELECT entry_id FROM events WHERE id=?", facts[0][0])[0][0]

    folder = ctx.working / "quality-retention-peer"
    folder.mkdir()
    methods = ["maintenance.audit", "maintenance.audit.apply"]
    owner_id = "quality-retention"
    installation = {
        "id": owner_id, "namespace": owner_id, "program": sys.executable,
        "args": [str(ctx.root / "tests/peers/aux-model/peer.py"),
                 str(folder), str(sdk / "python-install")],
        "grants": ["forget.owner", *methods],
    }
    with (data / "yuki.toml").open("a", encoding="utf-8") as config:
        config.write("\n[[plugins]]\n" + "\n".join(f"{key}={json.dumps(value)}" for key,value in installation.items()))
        config.write(
            '\n[plugins.maintenance.scopes.original]\nactor=' + json.dumps(owner.actor)
            + '\nentry=' + json.dumps(entry) + '\npurposes=["audit"]\nmethods=' + json.dumps(methods)
            + '\nwrite_subject={kind="person",person=' + json.dumps(owner.actor) + '}\n'
        )
    peer = aux.Peer(owner, folder)
    scope = {"scope": "original", "purpose": "audit", "integrity": True}
    with owner.svc.running():
        owner.until((folder / "ready.json").exists)
        plain = peer.call("maintenance.audit", scope)
        ctx.equal("read-only audit still creates no retention association",
                  owner.rows("SELECT count(*) FROM maintenance_quality_dependencies WHERE owner=?", owner_id), [(0,)])
        audit = peer.call("maintenance.audit", {**scope, "retain": True})
        refs = {item["fact"]: item["erasure_refs"] for item in audit["items"]}
        ctx.equal("retention leaves the original scan fingerprint intact", audit["fingerprint"], plain["fingerprint"])
        ctx.equal("two original sources have distinct opaque references",
                  sorted(map(len, refs.values())), [1, 1])
        ctx.equal("quality retention never acquires a source-reading hold",
                  owner.rows("SELECT count(*) FROM source_holds WHERE owner=?", owner_id), [(0,)])
        ctx.check("retained reports avoid source text or native event identity",
                  "original quality" not in str(audit) and not any("event" in item or "source_ids" in item for item in audit["items"]))
        ctx.equal("the same source keeps its reference across audits",
                  peer.call("maintenance.audit", {**scope, "retain": True}), audit)
        apply = {**scope, "retain": True, "operation": "approved-retention",
                 "fingerprint": audit["fingerprint"], "approved": [facts[0][1]]}
        applied = peer.call("maintenance.audit.apply", apply)
        ctx.equal("repair carries only the selected original source reference",
                  applied["erasure_refs"], refs[facts[0][1]])
        ctx.equal("cold replay keeps the original approved effect",
                  peer.call("maintenance.audit.apply", apply), applied)
    with owner.svc.running():
        owner.until(lambda: any(plugin["online"] for plugin in owner.op("host", "status")["plugins"]))
        erased = owner.op("privacy", "erase", "erase-quality-report", owner.actor, json.dumps([facts[0][0]]))
        ctx.write("quality-erasure-pending.json", owner.op("privacy", "pending", owner_id))
        callback_path = folder / f"forget-{erased['watermark']}.json"
        owner.until(callback_path.exists)
        owner.until(lambda: not owner.op("privacy", "pending", owner_id)["obligations"])
        callback = json.loads(callback_path.read_bytes())
        ctx.equal("forget passes the exact opaque dependency without another fact's reference",
                  callback["erasure_refs"], refs[facts[0][1]])
        ctx.equal("quality retention grants no access to event locators", callback["sources"], [])
        peer.call("maintenance.audit.apply", apply, False)
        remaining = peer.call("maintenance.audit", {**scope, "retain": True, "fact": facts[1][1]})
        ctx.equal("the unrelated original report remains authorized",
                  remaining["items"][0]["erasure_refs"], refs[facts[1][1]])
        ctx.equal("the erased receipt leaves its original operation fence",
                  owner.rows("SELECT result FROM maintenance_audit_apply WHERE operation=?", apply["operation"]),
                  [('{"erased":true}',)])
    evidence = {"audit": audit, "apply": applied, "forget": callback, "unrelated": remaining}
    ctx.write("quality-erasure.json", evidence)
    return evidence


def historical(ctx, sdk, aux):
    from tests.peers.chat.recorder import Recorder
    from tests.harness.compact import reply

    o = Owner(ctx, "maintenance-archived", 32)
    quote = "我每天喝烏龍茶。"
    successor = "00000000-0000-4000-8000-000000003500"
    with o.svc.running():
        source = o.human(quote)
        execution = o.op("ingress", "claim", source)["execution"]
        entry = o.rows("SELECT entry_id FROM events WHERE id=?", source)[0][0]
        o.op("person", "create", successor)
        o.op(
            "account", "transfer", o.user["assignment"], 300, 300, "--person", successor
        )
        ctx.check(
            "historical private entry really closed",
            o.rows("SELECT closed_at FROM entries WHERE id=?", entry)[0][0] is not None,
        )
        o.svc.command(
            "operator",
            "model",
            "activate",
            execution,
            "closed-human",
            "old-profile",
            accepted=False,
        )
    folder = ctx.working / "historical-maintainer"
    folder.mkdir()
    methods = json.loads(
        (ctx.root / "protocol/plugin/maintenance/methods.json").read_bytes()
    )["methods"]
    params = {"scope": "original", "purpose": "review"}
    with Recorder() as http:
        with (o.svc.data / "yuki.toml").open("a", encoding="utf-8") as f:
            f.write(
                '\n[[plugins]]\nid="historical"\nnamespace="historical"\nprogram='
                + json.dumps(sys.executable)
                + "\nargs="
                + json.dumps(
                    [
                        str(ctx.root / "tests/peers/aux-model/peer.py"),
                        str(folder),
                        str(sdk / "python-install"),
                    ]
                )
                + "\ngrants="
                + json.dumps(["forget.owner", "llm.complete", "llm.result", *methods])
                + "\n"
            )
            for scope, subject in [
                ("original", {"kind": "person", "person": o.actor}),
                ("reflection", {"kind": "self", "visibility": "global"}),
            ]:
                f.write(
                    "\n[plugins.maintenance.scopes."
                    + scope
                    + "]\nactor="
                    + json.dumps(o.actor)
                    + "\nentry="
                    + json.dumps(entry)
                    + '\npurposes=["review"]\nroutes=["review"]\nmethods='
                    + json.dumps(list(methods))
                    + "\n[plugins.maintenance.scopes."
                    + scope
                    + ".write_subject]\n"
                )
                for k, v in subject.items():
                    f.write(k + "=" + json.dumps(v) + "\n")
            f.write(
                '\n[plugins.aux_routes.review]\npurposes=["review"]\norigins=["maintenance"]\nentries='
                + json.dumps([entry])
                + "\n[plugins.aux_routes.review.profile]\nendpoint="
                + json.dumps(http.url)
                + '\nmodel="fixture"\nname="Archive reviewer"\nmerge_ms=0\nroot_slots=1\nmax_input_units=1000000\nmax_output=1024\n'
            )
        p = aux.Peer(o, folder)
        with o.svc.running():
            o.until((folder / "ready.json").exists)
            row = p.call("maintenance.history.page", {**params, "event": source})[
                "items"
            ][0]
            ctx.equal(
                "historical source retains its original actor", row["actor"], o.actor
            )
            refs = [{"event": source, "version": row["version"], "role": "primary"}]
            request = {
                **params,
                "key": "original-rebuild",
                "format": "extraction",
                "route": "review",
                "mode": "text_json",
                "sources": refs,
            }
            job = p.submit("maintenance.extract.preview", request)
            raw, q = http.receive()
            body = json.loads(raw)
            ctx.check(
                "real old envelope enters original extraction request",
                quote in str(body) and str(source) in str(body),
            )
            claim = {
                "subject_ref": "speaker",
                "scope_type": "person",
                "memory_key": "tea",
                "category": "preference",
                "content": "每天喝烏龍茶",
                "evidence_quote": quote,
                "importance": 3,
                "confidence": 0.9,
            }
            reply(
                q,
                json.dumps(
                    {"claims": [{"source_event_id": source, "claim": claim}]},
                    ensure_ascii=False,
                ),
            )
            preview = p.receive(job)
            ctx.equal(
                "review returns original extraction metadata",
                preview["extraction_details"][0]["claim"]["category"],
                "preference",
            )
            ctx.equal(
                "archive preview creates no human event or fact",
                o.rows("SELECT count(*) FROM events"),
                [(1,)],
            )
            ctx.equal(
                "archive preview remains a dry-run",
                o.rows("SELECT count(*) FROM memory_facts"),
                [(0,)],
            )
            approved = {
                **params,
                "preview": preview["preview"],
                "operation": "archive-apply",
                "approved": [0],
            }
            applied = p.call("maintenance.extract.apply", approved)
            fact = applied["applied"][0]["receipt"]["fact"]
            ctx.equal(
                "rebuild fact keeps original Person after account transfer",
                o.rows("SELECT person FROM memory_facts WHERE id=?", fact),
                [(o.actor,)],
            )
            ctx.equal(
                "reviewed extraction retains original typed metadata",
                o.rows(
                    "SELECT category,kind,importance,temporal_mode FROM extract_fact_details WHERE fact=?",
                    fact,
                ),
                [("preference", "fact", 3, "persistent")],
            )
            roots = o.rows(
                "SELECT x.source,x.original_actor FROM executions x JOIN maintenance_roots r ON r.execution=x.id"
            )
            ctx.equal(
                "background root does not borrow original human execution",
                roots,
                [(None, None)],
            )
            reflected = {
                **params,
                "scope": "reflection",
                "key": "private-reflection",
                "route": "review",
                "sources": refs,
            }
            opened = p.call("maintenance.model.open", reflected)
            task = {
                "mode": "text_json",
                "schema": {
                    "type": "object",
                    "properties": {"actions": {"type": "array"}},
                    "required": ["actions"],
                    "additionalProperties": False,
                },
                "validation_retries": 0,
            }
            job = p.submit(
                "llm.complete",
                {
                    "origin": opened["origin"],
                    "call_id": "original-reflection",
                    "route": "review",
                    "purpose": "review",
                    "text": "Review this explicitly selected private source",
                    "task": task,
                },
            )
            _, q = http.receive()
            reply(
                q,
                json.dumps(
                    {
                        "actions": [
                            {
                                "action": "add",
                                "subject": {"kind": "self", "visibility": "global"},
                                "fact_key": "forbidden-global",
                                "body": "private inference",
                                "evidence": [
                                    {"kind": "event", "id": source, "quote": quote}
                                ],
                            }
                        ]
                    },
                    ensure_ascii=False,
                ),
            )
            paid = p.receive(job)
            p.call(
                "maintenance.extract.preview",
                {
                    **params,
                    "scope": "reflection",
                    "key": "reflect-preview",
                    "request": paid["request"],
                    "format": "edits",
                    "sources": refs,
                },
                False,
            )
            ctx.equal(
                "private paid reflection creates no global SELF",
                o.rows("SELECT count(*) FROM memory_facts WHERE kind='self'"),
                [(0,)],
            )
            attempt_ids = o.rows("SELECT id FROM model_attempts ORDER BY rowid")
        with o.svc.running():
            again = p.call("maintenance.extract.preview", request)
            ctx.equal(
                "cold archive review uses original candidate",
                again["preview"],
                preview["preview"],
            )
            ctx.equal(
                "cold archive apply returns exact receipt",
                p.call("maintenance.extract.apply", approved),
                applied,
            )
            ctx.equal(
                "cold archive preserves physical attempts",
                o.rows("SELECT id FROM model_attempts ORDER BY rowid"),
                attempt_ids,
            )
            ctx.check(
                "cold archive never repurchases paid extraction", http.wires.empty()
            )
            return {
                "source": source,
                "entry": entry,
                "preview": preview,
                "applied": applied,
                "roots": roots,
                "attempts": attempt_ids,
            }


def fact_inputs(ctx, sdk, aux):
    from tests.peers.chat.recorder import Recorder
    from tests.harness.compact import reply

    o = Owner(ctx, "maintenance-fact-plans", 32)
    facts = []
    with o.svc.running():
        source = o.human("Original fact cluster quote.")
        execution = o.op("ingress", "claim", source)["execution"]
        entry = o.rows("SELECT entry_id FROM events WHERE id=?", source)[0][0]
        for i in range(2):
            item = {
                "action": "add",
                "subject": {"kind": "person", "person": o.actor},
                "fact_key": "cluster-" + str(i),
                "body": "original cluster value " + str(i),
                "evidence": [
                    {
                        "kind": "event",
                        "id": source,
                        "quote": "Original fact cluster quote.",
                    }
                ],
            }
            facts.append(
                o.op(
                    "memory",
                    "edit",
                    execution,
                    o.actor,
                    "cluster-" + str(i),
                    json.dumps(item),
                )["fact"]
            )
    folder = ctx.working / "fact-plan-peer"
    folder.mkdir()
    methods = json.loads(
        (ctx.root / "protocol/plugin/maintenance/methods.json").read_bytes()
    )["methods"]
    params = {"scope": "original", "purpose": "review"}
    with Recorder() as http:
        with (o.svc.data / "yuki.toml").open("a", encoding="utf-8") as f:
            f.write(
                '\n[[plugins]]\nid="fact-plan"\nnamespace="fact-plan"\nprogram='
                + json.dumps(sys.executable)
                + "\nargs="
                + json.dumps(
                    [
                        str(ctx.root / "tests/peers/aux-model/peer.py"),
                        str(folder),
                        str(sdk / "python-install"),
                    ]
                )
                + "\ngrants="
                + json.dumps(["forget.owner", "llm.complete", "llm.result", *methods])
                + "\n"
            )
            f.write(
                "\n[plugins.maintenance.scopes.original]\nactor="
                + json.dumps(o.actor)
                + "\nentry="
                + json.dumps(entry)
                + '\npurposes=["review"]\nroutes=["review"]\nmethods='
                + json.dumps(list(methods))
                + '\n[plugins.maintenance.scopes.original.write_subject]\nkind="person"\nperson='
                + json.dumps(o.actor)
                + "\n"
            )
            f.write(
                '\n[plugins.aux_routes.review]\npurposes=["review"]\norigins=["maintenance"]\nentries='
                + json.dumps([entry])
                + "\n[plugins.aux_routes.review.profile]\nendpoint="
                + json.dumps(http.url)
                + '\nmodel="fixture"\nname="Fact review"\nmerge_ms=0\nroot_slots=1\nmax_input_units=1000000\nmax_output=4096\n'
            )
        p = aux.Peer(o, folder)
        task = {
            "mode": "text_json",
            "schema": {
                "type": "object",
                "properties": {"actions": {"type": "array"}},
                "required": ["actions"],
                "additionalProperties": False,
            },
            "validation_retries": 0,
        }

        def selection():
            page = p.call("maintenance.fact.page", params)["items"]
            return page, [
                {
                    "fact": r["fact"],
                    "revision": r["revision"],
                    "source_version": r["source_version"],
                    "role": "primary",
                }
                for r in page
            ]

        with o.svc.running():
            o.until((folder / "ready.json").exists)
            page, refs = selection()
            origin = p.call(
                "maintenance.model.open",
                {
                    **params,
                    "key": "stale-cluster",
                    "route": "review",
                    "sources": [],
                    "facts": refs,
                },
            )["origin"]
            job = p.submit(
                "llm.complete",
                {
                    "origin": origin,
                    "call_id": "original-cluster",
                    "route": "review",
                    "purpose": "review",
                    "text": json.dumps(page),
                    "task": task,
                },
            )
            _, q = http.receive()
            current = o.op("memory", "read", execution, o.actor, "--fact", facts[0])[
                "items"
            ][0]
            correction = {
                "action": "correct",
                "fact": facts[0],
                "revision": current["revision"],
                "proof": current["proof"],
                "body": "external change with the same fact count",
            }
            o.op(
                "memory",
                "edit",
                execution,
                o.actor,
                "outside-plan",
                json.dumps(correction),
            )
            reply(q, json.dumps({"actions": []}))
            p.receive(job, False)
            ctx.equal(
                "same-count source change holds the original paid response",
                o.rows("SELECT state FROM model_attempts"),
                [("response_held",)],
            )
            ctx.equal(
                "fact count alone cannot validate a preview",
                o.rows("SELECT count(*) FROM memory_facts"),
                [(2,)],
            )
            page, refs = selection()
            origin = p.call(
                "maintenance.model.open",
                {
                    **params,
                    "key": "original-partial-plan",
                    "route": "review",
                    "sources": [],
                    "facts": refs,
                },
            )["origin"]
            actions = [
                {
                    "action": "correct",
                    "fact": r["fact"],
                    "revision": r["revision"],
                    "proof": r["proof"],
                    "body": "approved planned value " + str(i) + ("x" * 20000),
                }
                for i, r in enumerate(page)
            ]
            job = p.submit(
                "llm.complete",
                {
                    "origin": origin,
                    "call_id": "original-cluster",
                    "route": "review",
                    "purpose": "review",
                    "text": json.dumps(page),
                    "task": task,
                },
            )
            _, q = http.receive()
            reply(q, json.dumps({"actions": actions}))
            paid = p.receive(job)
            request = {
                **params,
                "key": "partial-plan",
                "request": paid["request"],
                "format": "edits",
                "sources": [],
                "facts": refs,
            }
            p.call("maintenance.extract.preview", {**request, "facts": refs[:1]}, False)
            preview = p.call("maintenance.extract.preview", request)
            ctx.equal(
                "large paid preview uses bounded content pages",
                preview["inline"],
                False,
            )
            desc = preview["content"]
            offset, chunks = 0, []
            while True:
                block = p.call(
                    "maintenance.content",
                    {
                        **params,
                        **{k: desc[k] for k in ["kind", "id", "sha256"]},
                        "offset": offset,
                    },
                )
                import base64

                chunks.append(base64.b64decode(block["bytes"]))
                if block["next_offset"] is None:
                    break
                offset = block["next_offset"]
            raw = b"".join(chunks)
            ctx.equal(
                "large paid preview preserves all original actions",
                [a["body"] for a in json.loads(raw)["actions"]],
                [a["body"] for a in actions],
            )
            ctx.equal(
                "large paid preview content digest",
                hashlib.sha256(raw).hexdigest(),
                desc["sha256"],
            )
            first = p.call(
                "maintenance.extract.apply",
                {
                    **params,
                    "preview": preview["preview"],
                    "operation": "first-subset",
                    "approved": [0],
                },
            )
            snapshot = o.rows("SELECT id,revision FROM memory_facts ORDER BY id")
            original_requests = o.rows("SELECT id FROM model_requests ORDER BY rowid")
        with o.svc.running():
            second = p.call(
                "maintenance.extract.apply",
                {
                    **params,
                    "preview": preview["preview"],
                    "operation": "remaining-subset",
                    "approved": [1],
                },
            )
            ctx.equal(
                "partial plan resumes the unapproved original ordinal",
                [r["ordinal"] for r in second["applied"]],
                [1],
            )
            ctx.equal(
                "partial plan never repurchases its model result",
                o.rows("SELECT id FROM model_requests ORDER BY rowid"),
                original_requests,
            )
            ctx.check(
                "known original partial plan performs no HTTP", http.wires.empty()
            )
            p.call("maintenance.extract.preview", request)
            # Large retained bodies are read through byte pages under the same digest.
            long_body = "retained content " + ("x" * 20000)
            for index in range(4):
                item = {
                    "action": "add",
                    "subject": {"kind": "person", "person": o.actor},
                    "fact_key": "large-" + str(index),
                    "body": long_body,
                    "evidence": [
                        {
                            "kind": "event",
                            "id": source,
                            "quote": "Original fact cluster quote.",
                        }
                    ],
                }
                o.op(
                    "memory",
                    "edit",
                    execution,
                    o.actor,
                    "large-" + str(index),
                    json.dumps(item),
                )
            summary = p.call("maintenance.fact.page", params)
            large = next(
                r
                for r in summary["items"]
                if r.get("fact_key", "").startswith("large-")
            )
            descriptor = large["content"]
            offset = 0
            chunks = []
            while True:
                block = p.call(
                    "maintenance.content",
                    {
                        **params,
                        **{k: descriptor[k] for k in ("kind", "id", "sha256")},
                        "offset": offset,
                    },
                )
                import base64

                chunks.append(base64.b64decode(block["bytes"]))
                if block["next_offset"] is None:
                    break
                offset = block["next_offset"]
            raw = b"".join(chunks)
            ctx.equal(
                "large fact content pages retain exact digest",
                hashlib.sha256(raw).hexdigest(),
                descriptor["sha256"],
            )
            ctx.equal(
                "large fact remains complete behind bounded previews",
                json.loads(raw)["body"],
                long_body,
            )
            deps = p.call(
                "maintenance.dependencies",
                {
                    **params,
                    "kind": "fact",
                    "id": large["fact"],
                    "source_version": large["source_version"],
                },
            )
            ctx.equal(
                "retained fact identifies original erasure dependency",
                deps["sources"],
                [source],
            )
            current = o.op("memory", "read", execution, o.actor, "--fact", facts[0])[
                "items"
            ][0]
            o.op(
                "memory",
                "edit",
                execution,
                o.actor,
                "post-review-change",
                json.dumps(
                    {
                        "action": "correct",
                        "fact": facts[0],
                        "revision": current["revision"],
                        "proof": current["proof"],
                        "body": "outside change after approved checkpoint",
                    }
                ),
            )
            p.call(
                "maintenance.extract.apply",
                {
                    **params,
                    "preview": preview["preview"],
                    "operation": "stale-empty-review",
                    "approved": [],
                },
                False,
            )
            ctx.equal(
                "applied original operation keeps its receipt after external revision",
                p.call(
                    "maintenance.extract.apply",
                    {
                        **params,
                        "preview": preview["preview"],
                        "operation": "first-subset",
                        "approved": [0],
                    },
                ),
                first,
            )
            return {
                "stale_response": "response_held",
                "preview": preview["preview"],
                "first": first,
                "second": second,
                "before_resume": snapshot,
                "attempts": original_requests,
                "content": descriptor,
                "dependencies": deps,
            }


def tool_receipts(ctx, sdk, aux):
    from tests.peers.chat.recorder import Recorder
    from tests.harness.compact import reply, call
    import base64

    o = Owner(ctx, "maintenance-tools", 32)
    with Recorder() as http:
        o.config(http)
        with o.svc.running():
            source = o.human("Reflect on the original private worker result.")
            _, q = o.wire(http)
            reply(
                q,
                calls=[
                    call(
                        "accept",
                        "task_control",
                        {"action": "accept", "goal": "original work"},
                    )
                ],
            )
            _, q = o.wire(http)
            reply(
                q,
                calls=[
                    call(
                        "child",
                        "subagent_start",
                        {"goal": "make original receipt", "context": "private worker"},
                    )
                ],
            )
            a, qa = o.wire(http)
            b, qb = o.wire(http)
            if any(m.get("tool_call_id") == "child" for m in a["messages"]):
                parent_q, child_q = qa, qb
            else:
                parent_q, child_q = qb, qa
            reply(
                child_q,
                calls=[
                    call(
                        "original-tool",
                        "put",
                        {
                            "target": "reflection",
                            "value": "Original child receipt 035.",
                        },
                    )
                ],
            )
            wire, child_q = o.wire(http)
            receipt = json.loads(
                next(
                    m["content"]
                    for m in wire["messages"]
                    if m.get("tool_call_id") == "original-tool"
                )
            )
            ctx.equal("actual Work child tool succeeded", receipt["state"], "confirmed")
            execution, operation = o.rows(
                "SELECT execution,operation_id FROM model_tool_intents WHERE call_id='original-tool'"
            )[0]
            ctx.check(
                "tool belongs to a real delegated Work execution",
                o.rows(
                    "SELECT source,original_actor,origin FROM executions WHERE id=?",
                    execution,
                )[0][:2]
                == (None, None),
            )
            reply(
                child_q,
                calls=[
                    call(
                        "done",
                        "task_control",
                        {"action": "complete", "result": "child retained result"},
                    )
                ],
            )
            o.until(
                lambda: o.rows("SELECT state FROM executions WHERE id=?", execution)
                == [("completed",)]
            )
            reply(
                parent_q,
                calls=[
                    call(
                        "parent-done",
                        "task_control",
                        {"action": "complete", "result": "retained parent result"},
                    )
                ],
            )
            import time, queue

            deadline = time.monotonic() + 12
            while not o.rows(
                "SELECT id FROM executions WHERE source=? AND state='completed'", source
            ):
                if time.monotonic() > deadline:
                    ctx.write(
                        "tool-settlement-diagnostic.json",
                        {
                            "attempts": o.rows(
                                "SELECT state,diagnostic FROM model_attempts"
                            ),
                            "work": o.rows("SELECT id,status FROM work_nodes"),
                        },
                    )
                    raise AssertionError("parent settlement did not complete")
                try:
                    _, fresh = http.receive(timeout=0.25)
                except queue.Empty:
                    continue
                reply(
                    fresh,
                    calls=[
                        call(
                            "fresh-complete",
                            "task_control",
                            {"action": "complete", "result": "retained parent result"},
                        )
                    ],
                )
            entry = o.rows("SELECT entry_id FROM events WHERE id=?", source)[0][0]
            # Close the old account route. Original historical evidence stays with its Person.
            successor = "00000000-0000-4000-8000-000000003501"
            o.op("person", "create", successor)
            o.op(
                "account",
                "transfer",
                o.user["assignment"],
                2000,
                2000,
                "--person",
                successor,
            )
        folder = ctx.working / "tool-review-peer"
        folder.mkdir()
        methods = json.loads(
            (ctx.root / "protocol/plugin/maintenance/methods.json").read_bytes()
        )["methods"]
        with (o.svc.data / "yuki.toml").open("a", encoding="utf-8") as f:
            f.write(
                '\n[[plugins]]\nid="tool-review"\nnamespace="tool-review"\nprogram='
                + json.dumps(sys.executable)
                + "\nargs="
                + json.dumps(
                    [
                        str(ctx.root / "tests/peers/aux-model/peer.py"),
                        str(folder),
                        str(sdk / "python-install"),
                    ]
                )
                + "\ngrants="
                + json.dumps(["forget.owner", "llm.complete", "llm.result", *methods])
                + "\n"
            )
            f.write(
                "\n[plugins.maintenance.scopes.original]\nactor="
                + json.dumps(o.actor)
                + "\nentry="
                + json.dumps(entry)
                + '\npurposes=["reflect"]\nroutes=["review"]\nmethods='
                + json.dumps(list(methods))
                + '\n[plugins.maintenance.scopes.original.write_subject]\nkind="self"\nvisibility="private"\nentry='
                + json.dumps(entry)
                + "\n"
            )
            f.write(
                '\n[plugins.aux_routes.review]\npurposes=["reflect"]\norigins=["maintenance"]\nentries='
                + json.dumps([entry])
                + "\n[plugins.aux_routes.review.profile]\nendpoint="
                + json.dumps(http.url)
                + '\nmodel="fixture"\nname="Reflection"\nmerge_ms=0\nroot_slots=1\nmax_input_units=1000000\nmax_output=4096\n'
            )
        p = aux.Peer(o, folder)
        params = {"scope": "original", "purpose": "reflect"}
        with o.svc.running():
            o.until((folder / "ready.json").exists)
            page = p.call("maintenance.tool.page", {**params, "execution": execution})
            tool = next(t for t in page["items"] if t["operation"] == operation)
            by_event = p.call("maintenance.tool.page", {**params, "event": source})
            ctx.check("the parent event selects its delegated Work receipt", tool in by_event["items"])
            ctx.equal(
                "archived Work receipt retains original human author",
                tool["actor"],
                o.actor,
            )
            desc = tool["content"]
            block = p.call(
                "maintenance.content",
                {**params, **{k: desc[k] for k in ["kind", "id", "sha256"]}},
            )
            raw = base64.b64decode(block["bytes"])
            ctx.equal(
                "tool content has its exact digest",
                hashlib.sha256(raw).hexdigest(),
                desc["sha256"],
            )
            ctx.equal(
                "full tool content contains the actual retained value",
                json.loads(raw)["result"]["value"]["value"],
                "Original child receipt 035.",
            )
            refs = [
                {
                    "receipt": tool["receipt"],
                    "version": tool["version"],
                    "role": "primary",
                }
            ]
            root = p.call(
                "maintenance.model.open",
                {
                    **params,
                    "key": "reflection",
                    "route": "review",
                    "sources": [],
                    "tools": refs,
                },
            )
            task = {
                "mode": "text_json",
                "schema": {
                    "type": "object",
                    "properties": {"actions": {"type": "array"}},
                    "required": ["actions"],
                    "additionalProperties": False,
                },
                "validation_retries": 0,
            }
            actions = [
                {
                    "action": "add",
                    "subject": {
                        "kind": "self",
                        "visibility": "private",
                        "entry": entry,
                    },
                    "fact_key": "reflection",
                    "body": "retain original scoped lesson",
                    "evidence": [
                        {
                            "kind": "tool",
                            "execution": execution,
                            "operation": operation,
                            "quote": "Original child receipt 035.",
                        }
                    ],
                }
            ]
            job = p.submit(
                "llm.complete",
                {
                    "origin": root["origin"],
                    "call_id": "original-reflection",
                    "route": "review",
                    "purpose": "reflect",
                    "text": raw.decode(),
                    "task": task,
                },
            )
            _, q = http.receive()
            reply(q, json.dumps({"actions": actions}))
            paid = p.receive(job)
            request = {
                **params,
                "key": "review",
                "request": paid["request"],
                "format": "edits",
                "sources": [],
                "tools": refs,
            }
            p.call(
                "maintenance.extract.preview",
                {**request, "tools": [{**refs[0], "role": "context"}]},
                False,
            )
            preview = p.call("maintenance.extract.preview", request)
            applied = p.call(
                "maintenance.extract.apply",
                {
                    **params,
                    "preview": preview["preview"],
                    "operation": "reviewed",
                    "approved": [0],
                },
            )
            fact = applied["applied"][0]["receipt"]["fact"]
            proof = p.call("maintenance.evidence", {**params, "fact": fact})["items"][
                0
            ]["evidence"][0]
            ctx.equal(
                "review binds exact original tool receipt",
                json.loads(proof["key"]),
                [execution, operation],
            )
            ctx.equal(
                "private reflection keeps exact entry visibility",
                o.rows(
                    "SELECT visibility,visibility_entry FROM memory_facts WHERE id=?",
                    fact,
                ),
                [("private", entry)],
            )
            dependencies = p.call(
                "maintenance.dependencies",
                {
                    **params,
                    "kind": "tool",
                    "id": str(tool["receipt"]),
                    "source_version": tool["version"],
                },
            )
            ctx.equal(
                "Work receipt erasure includes original human source",
                dependencies["sources"],
                [source],
            )
            o.op(
                "privacy", "erase", "tool-source-erasure", o.actor, json.dumps([source])
            )
            ctx.equal(
                "erased tool is excluded before paging",
                p.call("maintenance.tool.page", {**params, "execution": execution})[
                    "items"
                ],
                [],
            )
            p.call(
                "maintenance.content",
                {**params, **{k: desc[k] for k in ["kind", "id", "sha256"]}},
                False,
            )
            ctx.equal(
                "tool-derived fact erased with actual source",
                o.rows("SELECT status FROM memory_facts WHERE id=?", fact),
                [("erased",)],
            )
            return {
                "receipt": tool,
                "preview": preview["preview"],
                "applied": applied,
                "proof": proof,
                "dependencies": dependencies,
            }
