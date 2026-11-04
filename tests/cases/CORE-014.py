"""Search only authorized original fields; exercise a deleted cursor and an immutable result anchor."""

import hashlib
import json
import sqlite3
import time
from contextlib import closing
from tests.peers.chat.recorder import Recorder
from tests.harness.service import Service, previous_schema


def run(ctx):
    svc = Service(ctx, ctx.working / "owner")
    previous_schema(ctx, svc.data, 10)
    ctx.check(
        "existing receipt schema migrates",
        svc.command("migrate")["schema_version"] >= 11,
    )
    a, b, c = (f"00000000-0000-4000-8000-{n:012x}" for n in (14, 15, 16))

    def op(*args, ok=True):
        return svc.command("operator", *map(str, args), accepted=ok)

    old = Service(ctx, ctx.working / "v10")
    previous_schema(ctx, old.data, 10)
    with closing(sqlite3.connect(old.data / "core.sqlite")) as db:
        db.executescript(
            """
          PRAGMA foreign_keys=ON;
          INSERT INTO persons(id) VALUES('00000000-0000-4000-8000-000000000088');
          INSERT INTO accounts VALUES('a','qq','legacy','1'),('b','qq','legacy','2');
          INSERT INTO assignments(id,account_id,original_person,kind,status,first_observed) VALUES('human','a','00000000-0000-4000-8000-000000000088','person','active',1),('self','b',NULL,'self','active',1);
          INSERT INTO entries(id,scope,actor_stage,bot_stage) VALUES('legacy-private','private','human','self');
          INSERT INTO events(id,source_scope,source_key,entry_id,account_id,raw_actor,actor_stage,bot_stage,observed_at,envelope,mentions) VALUES(5,'scope','old','legacy-private','a','00000000-0000-4000-8000-000000000088','human','self',1,'{"kind":"message","text":"舊版臺灣資料","segments":[]}','[]');
          INSERT INTO accepted_sources VALUES(5);
          INSERT INTO executions(id,source,original_actor,state) VALUES('legacy-run',5,'00000000-0000-4000-8000-000000000088','completed');
        """
        )
    old.command("migrate")
    with old.running():
        migrated = old.command(
            "operator",
            "history",
            "query",
            "legacy-run",
            "00000000-0000-4000-8000-000000000088",
            json.dumps({"kind": "search", "text": "台湾"}),
        )
        ctx.equal(
            "real preexisting v10 original is searchable on first access",
            [r["source_event"] for r in migrated["items"]],
            [5],
        )
    with svc.running():
        ctx.write("sqlite-build.json", op("database-info"))
        for actor in (a, b, c):
            op("person", "create", actor)
        handles = [
            op(
                "account",
                "open",
                "qq",
                "hist",
                str(i + 200),
                1,
                "--person",
                actor,
                "--start",
                1,
            )
            for i, actor in enumerate((a, b, c))
        ]
        bot = op("account", "open", "qq", "hist", "100", 1, "--start", 1)

        def record(person, space, key, text):
            idx = (a, b, c).index(person)
            frame = {
                "account": handles[idx]["account"],
                "bot_stage": bot["assignment"],
                "source": key,
                "kind": "message",
                "occurred": 2,
                "observed": 3,
                "segments": [{"type": "text", "text": text}],
            }
            if key == "voice":
                frame["segments"] = [
                    {"type": "media", "kind": "record", "reference": "r0"},
                    {"type": "media", "kind": "record", "reference": "r1"},
                ]
                frame["text"] = text
            if space:
                frame["space"] = space
            event = op("ingress", "record", json.dumps(frame), "--accept")["event"]
            return event, op("ingress", "claim", event)

        first, root = record(a, None, "origin", "original question")
        _, public_a = record(a, "G", "member-a", "public member A")
        note, _ = record(b, "G", "note", "臺灣歷史資料庫")
        skip, _ = record(b, "G", "skip", "臺灣資料，不可計入")
        audio, audio_job = record(b, "G", "voice", "voice event")
        private_b, other = record(b, None, "private-b", "臺灣資料 private B")
        outsider, _ = record(c, "OTHER", "outside", "臺灣資料 outside")
        op(
            "history",
            "transcribe",
            audio_job["execution"],
            b,
            audio,
            json.dumps(
                [
                    {"source": "current", "segment_index": 0, "text": "資料"},
                    {"source": "current", "segment_index": 1, "text": "庫邊界"},
                    {
                        "source": "reply",
                        "segment_index": 0,
                        "text": "quoted-private-marker",
                    },
                ]
            ),
        )

        def query(args, key=None, who=a, run=root):
            tail = ["--operation", key] if key else []
            return op(
                "history",
                "query",
                run["execution"],
                who,
                json.dumps(args, ensure_ascii=False),
                *tail,
            )

        def ids(result):
            return [
                x["source_event"] for x in result["items"] if x["status"] == "visible"
            ]

        req = {"kind": "search", "text": "台湾 资料", "exclude": ["不可"], "limit": 5}
        hit = query(req, "search-original")
        ctx.equal(
            "literal simplification and exclusion retain only authorized row",
            ids(hit),
            [note],
        )
        ctx.equal(
            "explicit source list intersects current readable entries",
            ids(query({**req, "entries": [other["entry"], public_a["entry"]]})),
            [note],
        )
        ctx.equal(
            "single Chinese character is searchable",
            ids(query({"kind": "search", "text": "臺", "limit": 8})),
            [skip, note],
        )
        terms = {"kind": "search", "text": "台湾 歷史"}
        ctx.equal("all requires both literal terms", ids(query(terms)), [note])
        ctx.equal(
            "any admits a partial literal match within the same authorized scope",
            ids(query({**terms, "mode": "any"})),
            [skip, note],
        )
        ctx.equal(
            "phrase cannot span ASR segments",
            ids(query({"kind": "search", "text": "資料庫邊界", "mode": "phrase"})),
            [],
        )
        ctx.check(
            "matched segment retains its own evidence label",
            query({"kind": "search", "text": "庫邊界"})["items"][0]["matched_fields"]
            == ["audio_transcript:1"],
        )
        ctx.equal(
            "quoted transcript is not indexed as current audio",
            ids(query({"kind": "search", "text": "quoted-private-marker"})),
            [],
        )
        ctx.equal(
            "author, date, attachment and entry intersect",
            ids(
                query(
                    {
                        "kind": "recent",
                        "author": b,
                        "attachment": "audio",
                        "entries": [public_a["entry"]],
                        "start_ms": 2,
                        "end_ms": 3,
                    }
                )
            ),
            [audio],
        )
        ctx.equal(
            "recent defaults to the original conversation",
            ids(query({"kind": "recent"})),
            [first],
        )
        trans_args = {"kind": "search", "text": "庫邊界"}
        old_trans = query(trans_args, "transcript-anchor")
        op(
            "history",
            "transcribe",
            root["execution"],
            a,
            audio,
            json.dumps([{"source": "current", "segment_index": 1, "text": "forged"}]),
            ok=False,
        )
        op(
            "history",
            "transcribe",
            audio_job["execution"],
            b,
            audio,
            json.dumps([{"source": "current", "segment_index": 1, "text": "校正後"}]),
        )
        ctx.equal(
            "original search snapshot survives a later transcript revision",
            query(trans_args, "transcript-anchor"),
            old_trans,
        )
        ctx.equal("fresh lookup uses the new revision", ids(query(trans_args)), [])
        with closing(
            sqlite3.connect((svc.data / "core.sqlite").as_uri() + "?mode=ro", uri=True)
        ) as db:
            plan = db.execute(
                "EXPLAIN QUERY PLAN SELECT rowid FROM history_fts WHERE history_fts MATCH ?",
                ("c" + public_a["entry"].replace("-", "") + "g1xe58fb0",),
            ).fetchall()
            stats = {
                "indexed_sources": db.execute(
                    "SELECT count(*) FROM history_fts"
                ).fetchone()[0],
                "utf8_term_bytes": db.execute(
                    "SELECT coalesce(sum(length(CAST(terms AS BLOB))),0) FROM history_fts"
                ).fetchone()[0],
            }
        ctx.check(
            "SQLite used FTS5 for the literal candidate query",
            any("VIRTUAL TABLE INDEX" in row[-1] for row in plan),
        )
        around = query({"kind": "around", "anchor": hit["items"][0]["id"], "limit": 5})
        ctx.equal(
            "around the original operation is identical to its first result anchor",
            query(
                {"kind": "around", "anchor_operation": "search-original", "limit": 5}
            ),
            around,
        )
        ctx.check(
            "around is anchored in the original source",
            note in ids(around) and private_b not in ids(around),
        )
        page = query({"kind": "recent", "entries": [public_a["entry"]], "limit": 2})
        cursor = page["next_cursor"]
        boundary = page["items"][-1]["source_event"]
        op("privacy", "erase", "erase-boundary", b, json.dumps([boundary]))
        later = query(
            {
                "kind": "recent",
                "entries": [public_a["entry"]],
                "limit": 3,
                "cursor": cursor,
            }
        )
        ctx.check(
            "deleted last row does not invalidate numeric keyset",
            ids(later) and all(x["id"] < cursor for x in later["items"]),
        )
        if note != boundary:
            op("privacy", "erase", "erase-note", b, json.dumps([note]))
        replay = query(req, "search-original")
        ctx.equal(
            "old result retains original missing location",
            [r["status"] for r in replay["items"]],
            ["missing"],
        )
        op(
            "history",
            "query",
            root["execution"],
            a,
            json.dumps({**req, "text": "other"}),
            "--operation",
            "search-original",
            ok=False,
        )
        dirs = op("history", "directory", root["execution"], a)
        ctx.check(
            "person directory doesn't create private entry",
            b in [x["person"] for x in dirs["subjects"]]
            and other["entry"] not in [e["entry"] for e in dirs["entries"]],
        )
        ctx.check(
            "private and unrelated groups never leak",
            private_b not in ids(page) and outsider not in ids(page),
        )
        op("person", "set-enabled", a, "false")
        op(
            "history",
            "query",
            root["execution"],
            a,
            json.dumps(req),
            "--operation",
            "search-original",
            ok=False,
        )
    # A model reads only the authorized tool snapshot. The next paid wire is derived from those original sources.
    runner = Service(ctx, ctx.working / "main")

    def run_cli(*args):
        return runner.command(*map(str, args))

    run_cli("init", "--memory-mode", "lexical-only")
    with runner.running():
        run_cli("operator", "person", "create", a)
        acct = run_cli(
            "operator",
            "account",
            "open",
            "qq",
            "main",
            "200",
            1,
            "--person",
            a,
            "--start",
            1,
        )["account"]
        recv = run_cli(
            "operator", "account", "open", "qq", "main", "100", 1, "--start", 1
        )["assignment"]
        past = {
            "source": "past",
            "account": acct,
            "bot_stage": recv,
            "occurred": 2,
            "observed": 2,
            "kind": "message",
            "segments": [{"type": "text", "text": "歷史內容-雪山"}],
        }
        original = run_cli("operator", "ingress", "record", json.dumps(past))["event"]
    with Recorder() as peer:
        with (runner.data / "yuki.toml").open("a") as cfg:
            cfg.write(
                f'\n[main]\nendpoint="{peer.url}"\nmodel="fixture"\nname="Yuki"\nmerge_ms=0\nroot_slots=4\nmax_input_units=160000\nmax_output=128\n'
            )
        with runner.running():
            trigger = {
                **past,
                "source": "now",
                "segments": [{"type": "text", "text": "retrieve history"}],
            }
            current = run_cli(
                "operator", "ingress", "record", json.dumps(trigger), "--accept"
            )["event"]
            raw, reply = peer.receive()
            ctx.check(
                "Main offers the real bound history tool",
                any(
                    t["function"]["name"] == "history_query"
                    for t in json.loads(raw)["tools"]
                ),
            )
            call = {
                "type": "function",
                "id": "history-call",
                "function": {
                    "name": "history_query",
                    "arguments": json.dumps({"kind": "search", "text": "雪山"}),
                },
            }
            reply.put(
                (
                    {
                        "choices": [
                            {
                                "finish_reason": "tool_calls",
                                "message": {
                                    "role": "assistant",
                                    "content": None,
                                    "tool_calls": [call],
                                },
                            }
                        ]
                    },
                    0,
                )
            )
            paid, reply = peer.receive()
            receipt = json.loads(json.loads(paid)["messages"][-1]["content"])
            ctx.equal(
                "model actual tool result is an original authorized row",
                receipt["result"]["items"][0]["source_event"],
                original,
            )
            reply.put(
                (
                    {
                        "choices": [
                            {
                                "finish_reason": "stop",
                                "message": {"role": "assistant", "content": "internal"},
                            }
                        ]
                    },
                    0,
                )
            )
            deadline = time.monotonic() + 5
            with closing(sqlite3.connect(runner.data / "core.sqlite")) as db:
                while not db.execute(
                    "SELECT id FROM executions WHERE source=? AND state='completed'",
                    (current,),
                ).fetchone():
                    if time.monotonic() > deadline:
                        raise AssertionError("Main did not finish after query")
                    time.sleep(0.02)
                ctx.equal(
                    "only original inputs persisted",
                    db.execute("SELECT count(*) FROM events").fetchone()[0],
                    2,
                )
            run_cli(
                "operator",
                "privacy",
                "erase",
                "paid-history",
                a,
                json.dumps([original]),
            )
            with closing(sqlite3.connect(runner.data / "core.sqlite")) as db:
                ctx.check(
                    "erased history cannot survive in a derived paid request",
                    "雪山"
                    not in str(
                        db.execute("SELECT request_json FROM model_requests").fetchall()
                    ),
                )
    from tests.harness.contacts import run as contacts

    contact_result = contacts(ctx)
    ctx.write("original-contact-directory.json", contact_result)
    return {
        "migrated": migrated,
        "match": hit,
        "main_receipt": receipt,
        "source_revision_receipt": old_trans,
        "query_plan": plan,
        "fixture_index": stats,
        "oracle_sha256": hashlib.sha256(
            (ctx.root / "fixtures/history/v4-literals.json").read_bytes()
        ).hexdigest(),
        "native_version": "OpenCC 1.4.2",
        "target_corpus_calibration": "not_provided",
        "cursor_after_deletion": later,
        "around": around,
        "original_replay": replay,
        "original_contacts": contact_result,
        "directory": dirs,
        "method": "v4 NFKC/casefold, OpenCC t2s, 1/2/3-gram candidates with exact per-field verification",
    }
