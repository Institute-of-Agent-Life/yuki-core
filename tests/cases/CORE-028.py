"""Physical embedding requests race committed facts, corrections, erasure and lost replies."""

import json
import time
from concurrent.futures import ThreadPoolExecutor
from tests.harness.compact import Owner
from tests.harness.service import Service
from tests.peers.chat.recorder import Recorder


def configure(o, peer, model):
    cfg = o.svc.data / "yuki.toml"
    cfg.write_text(
        f'format_version=1\nlisten="127.0.0.1:0"\n[memory]\nmode="hybrid"\n[memory.embedding]\nprovider="openai_compatible"\nmodel="{model}"\nbase_url="{peer.url.removesuffix("/embeddings")}"\ndimensions=2\n'
    )


def vector_result(reply, values, tokens=5):
    reply.put(
        (
            {
                "data": [{"index": i, "embedding": v} for i, v in values],
                "usage": {"prompt_tokens": tokens},
            },
            0,
        )
    )


def run(ctx):
    fresh = Service(ctx, ctx.working / "hybrid")
    fresh.command("init")
    ctx.check(
        "standard installation selects hybrid",
        'mode = "hybrid"' in (fresh.data / "yuki.toml").read_text(),
    )
    ctx.check(
        "missing hybrid provider rejects start and doctor",
        "hybrid memory requires" in fresh.command("doctor", accepted=False)
        and "hybrid memory requires" in fresh.command("run", accepted=False),
    )
    o = Owner(ctx, "vectors", 20)
    wire_log = []

    def wire(peer):
        raw, response = peer.receive()
        data = json.loads(raw)
        wire_log.append(data)
        return data, response

    def add(text, key):
        event = o.human(text)
        ex = o.op("ingress", "claim", event)["execution"]
        edit = {
            "action": "add",
            "subject": {"kind": "person", "person": o.actor},
            "fact_key": key,
            "body": text,
            "evidence": [{"kind": "event", "id": event, "quote": text}],
        }
        id = o.op(
            "memory", "edit", ex, o.actor, key, json.dumps(edit, ensure_ascii=False)
        )["fact"]
        return event, ex, id

    def correct(ex, fid, new, op):
        receipt = o.op("memory", "read", ex, o.actor, "--fact", fid)["items"][0]
        update = {
            "action": "correct",
            "fact": fid,
            "revision": receipt["revision"],
            "proof": receipt["proof"],
            "body": new,
        }
        return o.op(
            "memory", "edit", ex, o.actor, op, json.dumps(update, ensure_ascii=False)
        )

    with o.svc.running():
        ctx.equal(
            "lexical-only boot without semantic provider",
            o.op("memory", "vector-status")["embedding_enabled"],
            False,
        )
        tea = add("飲茶 １２３４５", "tea")
        book = add("書架朝東", "book")
        o.op("model", "root", book[1], 16, 100000)
    with Recorder("/v1/embeddings") as peer:
        configure(o, peer, "first-model")
        with o.svc.running(), ThreadPoolExecutor(max_workers=2) as pool:
            body, q = wire(peer)
            ctx.equal(
                "only committed, bounded documents without platform IDs",
                body["input"],
                [
                    "Kind: fact\nCategory: \nKey: tea\nFact: 飲茶 [id]",
                    "Kind: fact\nCategory: \nKey: book\nFact: 書架朝東",
                ],
            )
            ctx.equal(
                "wire reproduces original provider contract",
                (body["dimensions"], body["encoding_format"], body["model"]),
                (2, "float", "first-model"),
            )
            ctx.equal(
                "fact available while physical request held",
                o.op("memory", "read", tea[1], o.actor, "--fact", tea[2])["items"][0][
                    "body"
                ],
                "飲茶 １２３４５",
            )
            vector_result(q, [(0, [1, 0])])
            o.until(
                lambda: o.rows(
                    "SELECT count(*) FROM embedding_items WHERE state='missing'"
                )
                == [(1,)]
            )
            partial = o.op("memory", "vector-status")
            ctx.equal(
                "partial indexed only valid original slot",
                (partial["covered"], partial["degraded"]),
                (1, True),
            )
            ctx.equal(
                "only one missing slot is released",
                o.op("memory", "retry-missing")["released_missing"],
                1,
            )
            body, q = wire(peer)
            ctx.equal("repair sends only missing document", len(body["input"]), 1)
            ctx.check("repair contains original book", "書架朝東" in body["input"][0])
            vector_result(q, [(0, [0, 1])])
            o.until(lambda: o.op("memory", "vector-status")["covered"] == 2)
            correct(tea[1], tea[2], "新茶", "rev-2")
            body, q = wire(peer)
            ctx.check(
                "revision two sent from selected new body", "新茶" in body["input"][0]
            )
            correct(tea[1], tea[2], "最後的茶", "rev-3")
            vector_result(q, [(0, [0.5, 0.5])])
            body, q = wire(peer)
            ctx.equal(
                "stale in-flight result was not published",
                o.rows("SELECT count(*) FROM fact_vectors WHERE fact=?", tea[2]),
                [(0,)],
            )
            ctx.check(
                "new physical request contains only revised projection",
                "最後的茶" in body["input"][0],
            )
            vector_result(q, [(0, [1, 0])])
            o.until(lambda: o.op("memory", "vector-status")["covered"] == 2)

            def query(op, text, ok=True):
                return o.svc.command(
                    "operator",
                    "memory",
                    "vector-query",
                    book[1],
                    o.actor,
                    op,
                    text,
                    accepted=ok,
                )

            pending = pool.submit(query, "q1", " 茶 １２３４５ ")
            body, q = wire(peer)
            ctx.equal("query uses bounded normalized text", body["input"], ["茶 [id]"])
            concurrent = pool.submit(query, "q-parallel", "茶 12345")
            vector_result(q, [(0, [0.5, 0.75])])
            ctx.equal("actual query vector", pending.result()["vector"], [0.5, 0.75])
            ctx.equal(
                "concurrent identical query joined one provider request",
                concurrent.result()["cached"],
                True,
            )
            cached = query("q2", "茶 12345")
            ctx.check(
                "cache-hit original key cannot change payload",
                "changed" in query("q2", "different", False),
            )

            ctx.check(
                "query cache contains only the pure vector",
                cached["cached"] and "items" not in cached and "facts" not in cached,
            )
            o.op("privacy", "erase", "erase-tea", o.actor, json.dumps([tea[0]]))
            refreshed = pool.submit(query, "q3", "茶 12345")
            body, q = wire(peer)
            ctx.equal("forget cleared cached query vector", body["input"], ["茶 [id]"])
            vector_result(q, [(0, [0.75, 0.5])])
            ctx.equal(
                "fresh vector consumed", refreshed.result()["vector"], [0.75, 0.5]
            )
            late = pool.submit(query, "q4", "短期查詢")
            _, q = wire(peer)
            forgotten = o.human("erase query-cache epoch")
            o.op("privacy", "erase", "forget-flight", o.actor, json.dumps([forgotten]))
            vector_result(q, [(0, [1, 0])])
            ctx.check(
                "query erasure epoch fences late publication",
                late.result().get("vector") is None,
            )
            ctx.equal(
                "erased document removed",
                o.rows("SELECT count(*) FROM fact_vectors WHERE fact=?", tea[2]),
                [(0,)],
            )
            disappearing = add("document in flight", "gone")
            _, q = wire(peer)
            o.op(
                "privacy", "erase", "forget-doc", o.actor, json.dumps([disappearing[0]])
            )
            vector_result(q, [(0, [0.125, 0.5])])
            o.until(
                lambda: o.rows(
                    "SELECT count(*) FROM model_attempts WHERE diagnostic='source_erased' AND received_at IS NOT NULL"
                )[0][0]
            )
            ctx.equal(
                "late document response leaves no erased fact or physical response material",
                o.rows(
                    "SELECT count(*) FROM fact_vectors WHERE fact=?", disappearing[2]
                ),
                [(0,)],
            )
            first = o.op("memory", "vector-status")
        configure(o, peer, "second-model")
        with o.svc.running():
            body, q = wire(peer)
            ctx.equal(
                "equal dimensions new profile must physically reindex",
                (body["model"], len(body["input"])),
                ("second-model", 1),
            )
            ctx.equal(
                "old profile vector stays isolated before new response",
                o.op("memory", "vector-status")["covered"],
                0,
            )
            vector_result(q, [(0, [1, 0])])
            o.until(lambda: o.op("memory", "vector-status")["covered"] == 1)
            second = o.op("memory", "vector-status")
            ctx.check(
                "reembed has independent physical usage",
                second["profile"] != first["profile"]
                and any(
                    v["purpose"] == "reembed" and v["known_input_tokens"] == 5
                    for v in second["usage"]
                ),
            )
            _, _, new = add("network-disconnect", "lost")
            _, q = wire(peer)
            q.put((b'{"data":[', 9))
            o.until(
                lambda: o.rows(
                    "SELECT count(*) FROM embedding_requests WHERE state='unknown'"
                )
                == [(1,)]
            )
            ctx.equal(
                "no retry right for unknown",
                o.op("memory", "retry-missing")["released_missing"],
                0,
            )
            ctx.check(
                "runtime disconnection marks degraded",
                o.op("memory", "vector-status")["degraded"],
            )
        with o.svc.running():
            time.sleep(0.4)
            ctx.check(
                "restart never purchases unknown document again", peer.wires.empty()
            )
            ctx.equal(
                "actual unknown physical attempt retained",
                o.rows("SELECT count(*) FROM model_attempts WHERE ever_unknown"),
                [(1,)],
            )
    ctx.write("physical-embedding-wire.json", wire_log)
    return {
        "first_profile": first,
        "second_profile": second,
        "partial": partial,
        "source": "fixtures/embedding/v4-wire.json",
        "requests": len(wire_log),
        "unknown_fact": new,
    }
