"""Exercise the actual Rust read path, provider and existing authorization graph."""

import hashlib
import gzip
import json
import sqlite3
import struct
import time
import uuid
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def populate(db, gold, owner, foreign, ex, profile):
    for i in range(1, 507):
        if i <= 6:
            key, body, category, date, vec, importance = gold["rows"][i - 1]
            person = owner
        else:
            key, body, category, date, vec, importance = (
                "outside",
                "private 桂花烏龍",
                "foreign",
                "2024-01-07T00:00:00Z",
                [1, 0],
                5,
            )
            person = foreign
        fid = f"fact-{i:04}"
        db.execute(
            "INSERT INTO memory_facts(id,kind,person,visibility,fact_key,created_execution,status) VALUES(?,'person',?,'subject',?,?,'active')",
            (fid, person, key, ex),
        )
        db.execute(
            "INSERT INTO memory_versions(fact,revision,body,confidence,execution) VALUES(?,1,?,0.8,?)",
            (fid, body, ex),
        )
        db.execute(
            "INSERT INTO extract_fact_details(fact,category,kind,importance,valid_from) VALUES(?,?,'fact',?,?)",
            (fid, category, importance, date),
        )
        projection = f"Kind: fact\nCategory: {category}\nKey: {key}\nFact: {body}"
        db.execute(
            "INSERT INTO fact_vectors VALUES(?,?,'document',2,?,1,?)",
            (
                fid,
                profile,
                hashlib.sha256(projection.encode()).hexdigest(),
                struct.pack("<2f", *vec),
            ),
        )
    db.commit()


def run(ctx):
    gold = json.loads((ctx.root / "fixtures/retrieval/v4-gold.json").read_bytes())
    o = Owner(ctx, "ranking", 22)
    stranger = str(uuid.uuid4())
    with o.svc.running():
        o.op("person", "create", stranger)
        acct = o.op(
            "account",
            "open",
            "qq",
            "compact",
            "202",
            1,
            "--person",
            stranger,
            "--start",
            1,
        )
        e = o.human("read authorized memory")
        ex = o.op("ingress", "claim", e)["execution"]
        o.op("model", "root", ex, 20, 100000)
        frame = {
            "source": "foreign",
            "account": acct["account"],
            "bot_stage": o.bot["assignment"],
            "occurred": 100,
            "observed": 100,
            "kind": "message",
            "segments": [{"type": "text", "text": "separate retrieval"}],
        }
        fe = o.op("ingress", "record", json.dumps(frame), "--accept")["event"]
        fx = o.op("ingress", "claim", fe)["execution"]
        o.op("model", "root", fx, 20, 100000)
    config = o.svc.data / "yuki.toml"
    lex = config.read_bytes()
    with Recorder("/v1/embeddings") as peer, ThreadPoolExecutor(max_workers=2) as pool:
        config.write_text(
            f'format_version=1\nlisten="127.0.0.1:0"\n[memory]\nmode="hybrid"\n[memory.embedding]\nprovider="openai_compatible"\nmodel="rank"\nbase_url="{peer.url.removesuffix("/embeddings")}"\ndimensions=2\n'
        )
        with o.svc.running():
            profile = o.op("memory", "vector-status")["profile"]
        with closing(sqlite3.connect(o.svc.data / "core.sqlite")) as db:
            populate(db, gold, o.actor, stranger, ex, profile)

        def get(exec, person, op, text, *kw):
            return o.op(
                "memory", "retrieve", exec, person, op, text, "--backend", "exact", *kw
            )

        def receive():
            raw, q = peer.receive()
            body = json.loads(raw)
            ctx.equal("embedding receives query only", body["input"], [gold["query"]])
            q.put(
                (
                    {
                        "data": [{"index": 0, "embedding": [1, 0]}],
                        "usage": {"prompt_tokens": 3},
                    },
                    0,
                )
            )

        with o.svc.running():
            pending = pool.submit(
                get, ex, o.actor, "first", gold["query"], "--limit", "10"
            )
            receive()
            actual = pending.result()
            ctx.equal(
                "500 unauthorized vectors were excluded before selection",
                (actual["authorized"], actual["covered"]),
                (6, 6),
            )
            ctx.equal(
                "full rank, signals and reasons agree with fixed v4",
                [
                    (
                        r["fact_id"],
                        r["relevance_band"],
                        r["selection_reason"],
                        r["lexical_rank"],
                        r["semantic_rank"],
                    )
                    for r in actual["items"]
                ],
                [
                    (
                        r["fact_id"],
                        r["band"],
                        r["reason"],
                        r["lexical_rank"],
                        r["semantic_rank"],
                    )
                    for r in gold["expected"]
                ],
            )
            ctx.check(
                "weighted RRF values match original across pool",
                all(
                    abs(h["fusion_score"] - r["fusion"]) < 1e-12
                    for h, r in zip(actual["items"], gold["expected"])
                ),
            )
            second = get(fx, stranger, "fresh-foreign", gold["query"], "--limit", "2")
            ctx.check(
                "new caller reuses only vector; independently selected own 500 facts",
                second["authorized"] == 500
                and all(r["fact_id"] > 6 for r in second["items"])
                and peer.wires.empty(),
            )
            native = o.op(
                "memory",
                "retrieve",
                fx,
                stranger,
                "vec1-page",
                gold["query"],
                "--backend",
                "vec1",
                "--limit",
                "2",
                "--candidates",
                "2",
            )
            ctx.equal(
                "bounded Vec1 candidates report approximation separately from failure",
                (
                    native["authorized"],
                    native["covered"],
                    native["approximate"],
                    native["truncated"],
                    native["partial_failure"],
                ),
                (500, 500, True, True, False),
            )
            full_native = o.op(
                "memory",
                "retrieve",
                ex,
                o.actor,
                "vec1-complete",
                gold["query"],
                "--backend",
                "vec1",
            )
            explained = o.op(
                "memory", "retrieve", ex, o.actor, "plan", gold["query"], "--explain"
            )
            ctx.check(
                "default exact backend and real native SQLite query plan",
                explained["backend"] == "exact"
                and any(
                    "eligible" in line or "fact_vectors" in line
                    for line in explained["semantic_eqp"]
                ),
            )
            ctx.equal(
                "complete Vec1 reranking agrees with original stable order",
                [h["fact_id"] for h in full_native["items"]],
                [h["fact_id"] for h in actual["items"]],
            )
            ctx.equal(
                "exact paged result truncation is not an index fault",
                (second["truncated"], second["approximate"], second["partial_failure"]),
                (True, False, False),
            )
            dated = get(
                ex,
                o.actor,
                "dated",
                gold["query"],
                "--start",
                "2024-01-01T00:00:00Z",
                "--end",
                "2024-02-01T00:00:00Z",
            )
            ctx.equal(
                "date evaluated before ranking; unknown and exclusive right endpoint excluded",
                sorted(h["fact_id"] for h in dated["items"]),
                [1, 2, 5, 6],
            )
            one = get(
                ex,
                o.actor,
                "one-date",
                gold["query"],
                "--start",
                "2024-01-08T00:00:00Z",
                "--end",
                "2024-01-09T00:00:00Z",
                "--limit",
                "1",
                "--candidates",
                "1",
            )
            ctx.equal(
                "one authorized record survives 500 nearer unauthorized peers",
                [r["fact_id"] for r in one["items"]],
                [5],
            )
        with closing(sqlite3.connect(o.svc.data / "core.sqlite")) as db:
            db.execute("DELETE FROM memory_retrieval_fts WHERE rowid=2")
            db.execute(
                "UPDATE fact_vectors SET content_hash='deliberately-stale' WHERE fact='fact-0003'"
            )
            db.execute("DELETE FROM fact_vectors WHERE fact='fact-0006'")
            db.commit()
        with o.svc.running():
            pending = pool.submit(get, ex, o.actor, "coverage", gold["query"])
            # Independent cold worker can request the one missing document; hold it until the read completes.
            doc = None
            query_seen = False
            while not query_seen:
                raw, q = peer.receive()
                v = json.loads(raw)
                if v["input"] == [gold["query"]]:
                    q.put(({"data": [{"index": 0, "embedding": [1, 0]}]}, 0))
                    query_seen = True
                else:
                    doc = q
            damaged = pending.result()
            ctx.equal(
                "authorized missing and stale coverage distinct from approximation and paging",
                (
                    damaged["partial_failure"],
                    damaged["approximate"],
                    damaged["truncated"],
                    damaged["stale_count"],
                    damaged["lexical_missing"],
                    damaged["covered"],
                ),
                (True, False, False, 1, 1, 4),
            )
            native_damage = o.op(
                "memory",
                "retrieve",
                ex,
                o.actor,
                "native-coverage",
                gold["query"],
                "--backend",
                "vec1",
            )
            ctx.equal(
                "native candidate selection filters stale index rows before K",
                (
                    native_damage["covered"],
                    native_damage["stale_count"],
                    native_damage["partial_failure"],
                ),
                (4, 1, True),
            )
            if doc:
                doc.put(({"data": []}, 0))
            denied = pool.submit(get, ex, o.actor, "outage", "different-query")
            while True:
                raw, q = peer.receive()
                if json.loads(raw)["input"] == ["different-query"]:
                    break
                q.put(({"data": []}, 0))
            q.put(({"error": "unavailable"}, 0, 503))
            fallback = denied.result()
            ctx.check(
                "authenticated lexical query remains usable on physical failure",
                fallback["provider_failure"] and fallback["partial_failure"],
            )
            revoked = pool.submit(
                lambda: o.svc.command(
                    "operator",
                    "memory",
                    "retrieve",
                    fx,
                    stranger,
                    "erased-inflight",
                    "stranger-query",
                    accepted=False,
                )
            )
            while True:
                raw, q = peer.receive()
                if json.loads(raw)["input"] == ["stranger-query"]:
                    break
                q.put(({"data": []}, 0))
            o.op("privacy", "erase", "revoke-query-source", stranger, json.dumps([fe]))
            q.put(({"data": [{"index": 0, "embedding": [1, 0]}]}, 0))
            refusal = revoked.result()
            ctx.check(
                "erasing the original event during query HTTP prevents returning stale authorized rows",
                bool(refusal) and "private 桂花烏龍" not in refusal,
            )
        config.write_bytes(lex)
        with o.svc.running():
            lexical = get(ex, o.actor, "lex", "茶")
            ctx.equal(
                "explicit lexical mode refrains from buying embeddings",
                (lexical["mode"], lexical["covered"], lexical["approximate"]),
                ("lexical-only", None, False),
            )
            ctx.check(
                "two-character and one-character fallback",
                any(row["fact_id"] == 1 for row in lexical["items"]),
            )
    with Recorder() as chat:
        o.config(chat)
        with o.svc.running():
            ev = o.human("find my remembered tea")
            wire, response = o.wire(chat)
            definition = next(
                t["function"]
                for t in wire["tools"]
                if t["function"]["name"] == "search_memory"
            )
            ctx.equal(
                "normal Main exposes the original search schema",
                definition["parameters"]["required"],
                ["query"],
            )
            reply(
                response,
                calls=[
                    call(
                        "recall",
                        "search_memory",
                        {
                            "query": gold["query"],
                            "target": {
                                "scope": "person",
                                "subject_ref": "current_speaker",
                            },
                            "limit": 2,
                        },
                    )
                ],
            )
            wire, response = o.wire(chat)
            receipt = json.loads(
                next(
                    m["content"]
                    for m in wire["messages"]
                    if m.get("tool_call_id") == "recall"
                )
            )
            paid = receipt["result"]
            ctx.check(
                "actual Main result preserves original scope and facts",
                receipt["state"] == "confirmed"
                and paid["result_scope"] == "explicit_targets"
                and len(paid["memories"]) == 2
                and all(row["fact_id"] <= 6 for row in paid["memories"]),
            )
            reply(
                response,
                calls=[
                    call(
                        "outside",
                        "search_memory",
                        {
                            "query": gold["query"],
                            "target": {"scope": "person", "person_id": stranger},
                        },
                    )
                ],
            )
            wire, response = o.wire(chat)
            refusal = json.loads(
                next(
                    m["content"]
                    for m in wire["messages"]
                    if m.get("tool_call_id") == "outside"
                )
            )
            ctx.equal(
                "unrelated explicit Person cannot fall back to global results",
                refusal["state"],
                "failed",
            )
            reply(response, "done")
            o.finished(ev)
    decision = json.loads((ctx.root / "bench/retrieval/decision.json").read_bytes())
    for name in ("core_matrix", "reference_matrix"):
        claim = decision["sources"][name]
        archived = gzip.decompress((ctx.root / claim["raw_gzip"]).read_bytes())
        ctx.equal(
            "raw " + name + " matches the measurement digest",
            hashlib.sha256(archived).hexdigest(),
            claim["sha256"],
        )
    ctx.check(
        "backend selection is bound to exact recorded matrix",
        decision["selected"] == "exact"
        and decision["matches"] == decision["cases"] == 36,
    )
    return {
        "original": gold["source_commit"],
        "full": actual,
        "backend_decision": decision,
        "main_result": paid,
        "during_query_erasure": "refused",
        "restricted_date": dated,
        "one_among_500": one,
        "partial_coverage": damaged,
        "provider_failure": fallback,
        "other_actor": {
            "authorized": second["authorized"],
            "result_ids": [x["fact_id"] for x in second["items"]],
        },
        "lexical": lexical,
    }
