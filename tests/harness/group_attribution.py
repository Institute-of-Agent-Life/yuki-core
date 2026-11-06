"""Actual group ingress and paid model tools; nobody is attributed from a display label."""

import hashlib, json, sqlite3
from contextlib import closing
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def run(ctx):
    o = Owner(ctx, "group-attr", 53)
    path = ctx.root / "fixtures/memory/v4-group-attribution.json"
    oracle = json.loads(path.read_bytes())
    calls = []
    target = {"scope_type": "person_group", "subject_ref": "mentioned_user"}
    self_target = dict(target, subject_ref="current_speaker")
    people = {o.actor: "1001"}

    def invoke(name, args, tag, expected="confirmed"):
        nonlocal q
        reply(q, calls=[call(tag, name, args)])
        wire, q = o.wire(model)
        receipt = json.loads(
            next(m["content"] for m in wire["messages"] if m.get("tool_call_id") == tag)
        )
        ctx.equal(
            tag + " " + str(receipt["result"].get("error", "")),
            receipt["state"],
            expected,
        )
        calls.append((tag, expected))
        return receipt

    def finish(e):
        reply(q, "NO_REPLY")
        o.finished(e)

    def send(text, mentions=(), group="3001", speaker=None):
        o.seq += 1
        args = {
            "source": "attribution-" + str(o.seq),
            "account": (speaker or o.user)["account"],
            "bot_stage": o.bot["assignment"],
            "reply_bot": "100",
            "kind": "message",
            "occurred": o.seq * 100,
            "observed": o.seq * 100,
            "segments": [{"type": "text", "text": text}]
            + [
                {"type": "mention", "label": "同名成员", "account": a["account"]}
                for a in mentions
            ],
        }
        if group:
            args["space"] = group
        return o.op(
            "ingress", "record", json.dumps(args, ensure_ascii=False), "--accept"
        )["event"]

    def read(n, tag):
        return invoke("read_memory", {"fact_id": n, "view": "fact"}, tag)["result"]

    def state():
        return [
            o.rows("SELECT number,revision,status FROM memory_facts ORDER BY number"),
            o.rows(
                "SELECT (SELECT count(*) FROM memory_evidence),(SELECT count(*) FROM memory_versions),(SELECT count(*) FROM memory_attributions)"
            ),
        ]

    def snapshot(fid):
        who, space, status, scope, body, authority = o.rows(
            "SELECT f.person,f.space,s.status,f.kind,v.body,a.authority FROM memory_facts f JOIN memory_fact_states s ON s.id=f.id JOIN memory_versions v ON v.fact=f.id AND v.revision=f.revision JOIN memory_attributions a ON a.fact=f.id WHERE f.number=?",
            fid,
        )[0]
        es = o.rows(
            "SELECT e.author,e.quote,e.attribution FROM memory_evidence e JOIN memory_facts f ON f.id=e.fact WHERE f.number=? ORDER BY e.quote",
            fid,
        )
        return {
            "subject": people[who],
            "group": space,
            "status": status,
            "scope": scope,
            "content": body,
            "authority": authority,
            "evidence": [
                {"author": people[x], "quote": y, "authority": z} for x, y, z in es
            ],
        }

    with Recorder() as model:
        o.config(model, "root_tool_calls=24\n")
        cfg = o.svc.data / "yuki.toml"
        cfg.write_text(cfg.read_text().replace("root_slots=8", "root_slots=24"))
        with o.svc.running():
            participants = []
            for ix in [2, 3]:
                person = "00000000-0000-4000-8000-00000000045" + str(ix)
                people[person] = "200" + str(ix)
                o.op("person", "create", person)
                a = o.op(
                    "account",
                    "open",
                    "qq",
                    "compact",
                    "20" + str(ix),
                    1,
                    "--person",
                    person,
                    "--start",
                    1,
                )
                participants.append(a)
                evt = send("present " + str(ix), speaker=a)
                _, q = o.wire(model)
                finish(evt)
            b, c = participants
            independent = send("Independent group only", group="5001")
            _, q = o.wire(model)
            ctrl = invoke(
                "memory_change",
                {
                    "operation": "create",
                    "target": self_target,
                    "new_content": "Independent group only",
                    "memory_key": "other",
                    "category": "other",
                },
                "prior-independent",
            )["result"]["fact_id"]
            ctrl_before = snapshot(ctrl)
            finish(independent)
            origin = send("我喜欢摄影")
            _, q = o.wire(model)
            create = {
                "operation": "create",
                "target": self_target,
                "new_content": "喜欢摄影",
                "memory_key": "hobby:photography",
                "category": "hobby",
            }
            old = invoke("memory_change", create, "group-original")["result"]
            n = old["fact_id"]
            projection = {"created": snapshot(n)}
            ctx.equal(
                "paid original has explicit provenance",
                read(n, "explicit-read")["authority"],
                "explicit",
            )
            finish(origin)
            move = {"operation": "reassign", "target": target, "fact_id": n}
            for tag, mentions, group, speaker in [
                ("private-target", [b], None, None),
                ("mixed-unproved", [b, {"account": None}], "3001", None),
                ("missing-target", [], "3001", None),
                ("ambiguous-target", [b, c], "3001", None),
                ("another-group", [b], "4001", None),
                ("not-the-source-owner", [b], "3001", c),
            ]:
                evt = send("incorrect attribution", mentions, group, speaker)
                _, q = o.wire(model)
                before = state()
                if tag in [
                    "mixed-unproved",
                    "missing-target",
                    "ambiguous-target",
                    "not-the-source-owner",
                ]:
                    read(n, tag + "-read")
                no = invoke("memory_change", move, tag, "failed")
                if tag == "mixed-unproved":
                    ctx.equal(
                        "unresolved second mention cannot authorize the known person",
                        no["result"]["error"],
                        "unproved_group_mention",
                    )
                if tag == "ambiguous-target":
                    projection["ambiguous_reason"] = no["result"]["error"]
                ctx.equal(tag + " changes neither subject", state(), before)
                finish(evt)
        with closing(sqlite3.connect(o.svc.data / "core.sqlite")) as db:
            db.execute(
                "CREATE TRIGGER group_test_abort BEFORE INSERT ON memory_attributions WHEN NEW.supersedes IS NOT NULL BEGIN SELECT RAISE(ABORT,'injected version link failure'); END"
            )
            db.commit()
        with o.svc.running():
            evt = send("刚才那条其实说的是小明喜欢摄影", [b])
            _, q = o.wire(model)
            before = state()
            invoke("memory_change", move, "missing-paid-read", "failed")
            read(n, "before-link-read")
            fault = invoke("memory_change", move, "late-link-failure", "failed")
            ctx.check(
                "actual writer reached final attribution link",
                "injected version link failure" in str(fault["result"]),
            )
            ctx.equal(
                "supersession, quotes and attribution rolled back", state(), before
            )
            finish(evt)
        with closing(sqlite3.connect(o.svc.data / "core.sqlite")) as db:
            db.execute("DROP TRIGGER group_test_abort")
            db.commit()
        with o.svc.running():
            moved_event = send("刚才那条其实说的是小明喜欢摄影", [b])
            _, q = o.wire(model)
            read(n, "fresh-original")
            m = invoke("memory_change", move, "reattribute")
            new = m["result"]["new_fact_id"]
            ex = o.rows("SELECT id FROM executions WHERE source=?", moved_event)[0][0]
            projection.update(
                superseded=snapshot(n),
                reassigned=snapshot(new),
                distinct_version=n != new,
                reassign_reason=m["result"]["reason_code"],
                reassign_outcome=m["result"]["outcome"],
            )
            ctx.equal(
                "reader exposes superseded original before erasure",
                read(n, "superseded-original")["status"],
                "superseded",
            )
            before = state()
            invoke("memory_change", move, "stale-original", "failed")
            ctx.equal("stale original cannot be moved twice", state(), before)
            finish(moved_event)
            words = "江环是@鬼頭桃菜，你记错了"
            evt = send(words, [b])
            _, q = o.wire(model)
            direct = dict(
                create,
                target=target,
                new_content="江环是鬼頭桃菜",
                memory_key="identity:jianghuan",
                category="identity",
                evidence_quote=words,
            )
            before = state()
            invoke(
                "memory_change",
                dict(direct, evidence_quote="fake quote"),
                "forged-quote",
                "failed",
            )
            ctx.equal(
                "forged third party evidence refused before creation", state(), before
            )
            other = invoke("memory_change", direct, "direct-third-party")["result"]
            projection["direct"] = snapshot(other["fact_id"])
            ctx.equal(
                "original explicit and third-party provenance reproduced exactly",
                projection,
                oracle["projection"],
            )
            direct_exec = o.rows("SELECT id FROM executions WHERE source=?", evt)[0][0]
            ctx.equal(
                "later same-group model used the original source",
                o.rows(
                    "SELECT count(*) FROM main_dependencies WHERE execution=? AND event=?",
                    direct_exec,
                    origin,
                ),
                [(1,)],
            )
            finish(evt)
            kept = state()
        with o.svc.running():
            cold = o.op("effect", "read", ex, m["id"], o.actor)
            ctx.equal(
                "cold original receipt and version link remain immutable",
                cold["result"],
                m["result"],
            )
            ctx.equal("no duplicated group attribution after recovery", state(), kept)
            o.op(
                "privacy",
                "erase",
                "forget-group-attribution",
                o.actor,
                json.dumps([origin]),
            )
            ctx.equal(
                "source erasure clears original and transferred contents",
                o.rows(
                    "SELECT count(*) FROM memory_versions v JOIN memory_facts f ON f.id=v.fact WHERE f.number IN(?,?) AND v.body IS NOT NULL",
                    n,
                    new,
                ),
                [(0,)],
            )
            ctx.equal(
                "derived third-party fact is scrubbed with the private source used by its execution",
                o.rows(
                    "SELECT status FROM memory_facts WHERE number=?", other["fact_id"]
                ),
                [("erased",)],
            )
            ctx.equal(
                "earlier independent group fact survives", snapshot(ctrl), ctrl_before
            )
    return {
        "oracle_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "source_commit": oracle["source_commit"],
        "projection": projection,
        "cases": calls,
        "late_abort_rolled_back": True,
        "cold_receipt": True,
        "withdrawal_scrubbed_both": True,
    }
