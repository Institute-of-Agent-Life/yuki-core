"""Actual structured batch wire, scoped quotes, durable response, and atomic fact publication."""

import json
import sqlite3
import time
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from tests.harness.service import Service, previous_schema
from tests.harness.crash import kill_writer
from tests.peers.chat.recorder import Recorder


def claim(source, scope, ref, key, text, quote):
    return {
        "source_event_id": source,
        "claim": {
            "subject_ref": ref,
            "scope_type": scope,
            "memory_key": key,
            "category": "fixture",
            "content": text,
            "evidence_quote": quote,
        },
    }


def answer(q, claims, missing=0):
    q.put(
        (
            {
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": json.dumps(
                                {"claims": claims}, ensure_ascii=False
                            ),
                        },
                        "finish_reason": "stop",
                    }
                ],
                "usage": {"prompt_tokens": 31, "completion_tokens": 7},
            },
            missing,
        )
    )


class Scene:
    def __init__(self, ctx, label):
        self.ctx = ctx
        self.s = Service(ctx, ctx.working / label)
        previous_schema(ctx, self.s.data, 15)
        (self.s.data / "SOUL.md").write_text("Yuki", encoding="utf-8")
        self.db = self.s.data / "core.sqlite"
        with closing(sqlite3.connect(self.db)) as db:
            db.execute(
                "INSERT INTO entries(id,scope,space) VALUES('original-entry','group','original-space')"
            )
            db.execute(
                "INSERT INTO main_origins(ref,kind,conversation) VALUES('original-client','plugin','original-entry')"
            )
            db.execute(
                "INSERT INTO executions(id,state,origin) VALUES('legacy-exec','completed','original-client')"
            )
            db.execute(
                "INSERT INTO internal_values(origin,target,value,revision,execution) VALUES('original-client','old','1',1,'legacy-exec')"
            )
            db.commit()
        ctx.check(
            "add source-batch origin without losing original children",
            self.s.command("migrate")["schema_version"] >= 20,
        )
        ctx.equal(
            "loaded migrated foreign keys", self.rows("PRAGMA foreign_key_check"), []
        )
        ctx.equal(
            "old origin and execution are intact",
            self.rows(
                "SELECT x.origin,v.value FROM executions x JOIN internal_values v ON v.origin=x.origin WHERE x.id='legacy-exec'"
            ),
            [("original-client", "1")],
        )
        self.seq = 0
        self.a, self.b = (f"00000000-0000-4000-8000-{n:012x}" for n in (27, 127))
        with self.s.running():
            ctx.write("sqlite-build.json", self.op("database-info"))
            self.handles = {}
            for person, native in [(self.a, "200"), (self.b, "201")]:
                self.op("person", "create", person)
                self.handles[person] = self.op(
                    "account",
                    "open",
                    "qq",
                    "extract",
                    native,
                    1,
                    "--person",
                    person,
                    "--start",
                    1,
                )
            self.bot = self.op(
                "account", "open", "qq", "extract", "100", 1, "--start", 1
            )
            self.nearby = self.event(self.a, "只有前文提過玻璃", "700")
        self.http = None

    def op(self, *args):
        return self.s.command("operator", *map(str, args))

    def rows(self, sql, *args):
        with closing(sqlite3.connect(self.db.as_uri() + "?mode=ro", uri=True)) as db:
            return db.execute(sql, args).fetchall()

    def event(self, person, text, group=None, accept=False, segments=None):
        self.seq += 1
        f = {
            "source": "extract-" + str(self.seq),
            "account": self.handles[person]["account"],
            "bot_stage": self.bot["assignment"],
            "occurred": self.seq * 1000,
            "observed": self.seq * 1000,
            "kind": "message",
            "sender": {"nickname": "Alice" if person == self.a else "Bob"},
            "segments": segments
            if segments is not None
            else [{"type": "text", "text": text}],
        }
        if group:
            f["space"] = group
        return self.op(
            "ingress",
            "record",
            json.dumps(f, ensure_ascii=False),
            *(["--accept"] if accept else []),
        )["event"]

    def config(self, http, *, count=2, age=3600, chars=8000, main=None):
        with (self.s.data / "yuki.toml").open("a") as f:
            f.write(
                f'\n[extraction]\nmode="json_schema"\ntrigger_count={count}\nmax_wait_seconds={age}\nmax_characters={chars}\n[extraction.profile]\nendpoint="{http.url}"\nmodel="extract-fixture"\nname="Yuki"\nmerge_ms=0\nroot_slots=1\nmax_input_units=300000\nmax_output=4096\n'
            )
            if main:
                f.write(
                    f'\n[main]\nendpoint="{main.url}"\nmodel="foreground"\nname="Yuki"\nmerge_ms=0\nroot_slots=2\nmax_input_units=200000\nmax_output=128\n'
                )

    def wire(self, http):
        try:
            raw, q = http.receive()
        except Exception:
            self.diagnostic()
            raise
        body = json.loads(raw)
        return body, json.loads(body["messages"][-1]["content"]), q

    def until(self, fn):
        end = time.monotonic() + 12
        while time.monotonic() < end:
            r = fn()
            if r:
                return r
            time.sleep(0.04)
        self.diagnostic()
        raise AssertionError("timed out waiting for original extraction batch")

    def diagnostic(self):
        self.ctx.write(
            "extract-diagnostic.json",
            {
                "events": self.rows("SELECT event,state,batch FROM extraction_events"),
                "batches": self.rows(
                    "SELECT id,state,diagnostic FROM extraction_batches"
                ),
                "attempts": self.rows("SELECT state,diagnostic FROM model_attempts"),
            },
        )


def faults(ctx):
    data = {}
    for fault in ("unknown", "self", "erase_context"):
        s = Scene(ctx, fault)
        with Recorder() as http:
            s.config(http, count=1)
            with s.s.running():
                evt = s.event(s.a, "我散步", "700")
                _, payload, q = s.wire(http)
                if fault == "erase_context":
                    s.op(
                        "privacy",
                        "erase",
                        "erase-batch-context",
                        s.a,
                        json.dumps([s.nearby]),
                    )
                    answer(
                        q,
                        [
                            claim(
                                evt, "person", "speaker", "walking", "本人散步", "散步"
                            )
                        ],
                    )
                    s.until(
                        lambda: s.rows(
                            "SELECT 1 FROM model_attempts WHERE state!='dispatched'"
                        )
                    )
                    ctx.equal(
                        "erased selection scrubs actual paid input and late response",
                        s.rows(
                            "SELECT request_json,response,state FROM model_requests r JOIN model_attempts p ON p.logical=r.id"
                        ),
                        [("{}", None, "response_held")],
                    )
                elif fault == "self":
                    answer(
                        q,
                        [
                            claim(
                                evt, "self", "speaker", "forged", "pretend SELF", "散步"
                            )
                        ],
                    )
                    s.until(
                        lambda: s.rows(
                            "SELECT id FROM extraction_batches WHERE state='held'"
                        )
                    )
                else:
                    answer(q, [], missing=11)
                    s.until(
                        lambda: s.rows(
                            "SELECT id FROM extraction_batches WHERE state='held'"
                        )
                    )
                    ctx.equal(
                        "incomplete physical response stays unknown",
                        s.rows("SELECT state FROM model_attempts"),
                        [("unknown",)],
                    )
                ctx.equal(
                    "invalid or unknown selection publishes no facts or watermark",
                    (
                        s.rows("SELECT count(*) FROM memory_facts"),
                        s.rows("SELECT count(*) FROM extraction_watermarks"),
                    ),
                    ([(0,)], [(0,)]),
                )
            with s.s.running():
                time.sleep(0.35)
                ctx.equal(
                    "reopen does not repurchase original batch",
                    s.rows("SELECT count(*) FROM model_attempts"),
                    [(1,)],
                )
                data[fault] = s.rows("SELECT state,diagnostic FROM extraction_batches")
    return data


def foreground(ctx):
    s = Scene(ctx, "foreground")
    with Recorder() as http, Recorder() as main:
        s.config(http, age=0, main=main)
        with s.s.running():
            first = s.event(s.a, "我喜歡聽雨", accept=True)
            _, q = main.receive()
            time.sleep(0.32)
            ctx.equal(
                "active interactive Main starts before background extraction",
                (http.wires.empty(), s.rows("SELECT count(*) FROM extraction_batches")),
                (True, [(0,)]),
            )
            q.put(
                (
                    {
                        "choices": [
                            {
                                "message": {"role": "assistant", "content": "ok"},
                                "finish_reason": "stop",
                            }
                        ]
                    },
                    0,
                )
            )
            _, payload, q = s.wire(http)
            ctx.equal(
                "age-ready batch uses only original private event",
                [e["source_event_id"] for e in payload["events"]],
                [first],
            )
            answer(q, [])
            s.until(lambda: s.rows("SELECT event FROM extraction_watermarks"))
            ctx.equal(
                "zero facts is a valid stored response",
                s.rows("SELECT trigger_reason,state FROM extraction_batches"),
                [("age", "committed")],
            )
    return first


def asr_case(ctx):
    s = Scene(ctx, "transcript")
    with Recorder() as http:
        s.config(http, count=10, chars=4)
        with s.s.running():
            event = s.event(
                s.a,
                "",
                "700",
                accept=True,
                segments=[
                    {"type": "media", "kind": "record", "reference": "owned-audio"}
                ],
            )
            root = s.op("ingress", "claim", event)["execution"]
            fields = [
                {"source": "current", "segment_index": 0, "text": "我喜歡跑步"},
                {"source": "reply", "segment_index": 0, "text": "引文不是本人說的"},
            ]
            s.op(
                "history",
                "transcribe",
                root,
                s.a,
                event,
                json.dumps(fields, ensure_ascii=False),
            )
            _, payload, q = s.wire(http)
            ctx.equal(
                "only original audio transcript supplies the source",
                payload["events"][0]["content"],
                "我喜歡跑步",
            )
            answer(
                q,
                [
                    claim(
                        event, "person", "speaker", "sport", "本人喜歡跑步", "喜歡跑步"
                    ),
                    claim(
                        event,
                        "person",
                        "speaker",
                        "wrong",
                        "quoted reply",
                        "引文不是本人說的",
                    ),
                ],
            )
            s.until(
                lambda: s.rows(
                    "SELECT id FROM extraction_batches WHERE state='committed'"
                )
            )
            ctx.equal(
                "character threshold and evidence bind the original ASR",
                s.rows(
                    "SELECT trigger_reason,(SELECT count(*) FROM memory_facts) FROM extraction_batches"
                ),
                [("characters", 1)],
            )
            fact = s.rows("SELECT number,id FROM memory_facts")[0][1]
            detail = s.op("memory", "read", root, s.a, "--fact", fact, "--evidence")
            ctx.check(
                "source-authorized reader retrieves the precise stored ASR quote",
                "喜歡跑步" in str(detail) and "引文不是本人說的" not in str(detail),
            )
            return {"event": event, "fact": fact}


def run(ctx):
    s = Scene(ctx, "claims")
    with Recorder() as http:
        s.config(http)
        with s.s.running():
            blank = s.event(s.a, "  ", "700")
            a = s.event(s.a, "我喜歡早餐吃麵包，也喜歡燕麥", "700")
            # An empty event cannot bring forward the paid count threshold.
            time.sleep(0.32)
            ctx.equal(
                "blank and single eligible event do not trigger two-source threshold",
                (
                    s.rows("SELECT count(*) FROM model_requests"),
                    s.rows("SELECT state FROM extraction_events WHERE event=?", blank),
                ),
                ([(0,)], [("blank",)]),
            )
            b = s.event(s.b, "我在台北工作", "700")
            body, payload, q = s.wire(http)
            ctx.equal(
                "same conversation cohort carries original source IDs",
                [e["source_event_id"] for e in payload["events"]],
                [a, b],
            )
            ctx.check(
                "nearby original message is disambiguation only",
                any(
                    e["content"] == "只有前文提過玻璃"
                    for e in payload["conversation_context"]
                ),
            )
            ctx.check(
                "dedicated structured profile has no Main program, send or Work tools",
                body["model"] == "extract-fixture"
                and body.get("response_format", {}).get("type") == "json_schema"
                and not body.get("tools")
                and len(body["messages"]) <= 3,
            )
            claims = [
                claim(a, "person", "speaker", "diet", "愛吃麵包", "早餐吃麵包"),
                claim(a, "person", "speaker", "diet", "愛吃燕麥", "喜歡燕麥"),
                claim(b, "person_group", "speaker", "job", "在臺北工作", "台北工作"),
                claim(a, "person", "speaker", "incorrect", "他人工作", "台北工作"),
                claim(s.nearby, "group", "group", "invalid", "玻璃", "玻璃"),
                claim(a, "group", "group", "discussion", "早餐話題", "早餐"),
            ]
            answer(q, claims)
            s.until(
                lambda: s.rows(
                    "SELECT id FROM extraction_batches WHERE state='committed'"
                )
            )
            ctx.equal(
                "same key inserts two distinct immutable original facts",
                s.rows(
                    "SELECT kind,person,count(*) FROM memory_facts WHERE fact_key='diet' GROUP BY kind,person"
                ),
                [("person", s.a, 2)],
            )
            ctx.equal(
                "adjacent actor cannot supply another event's quote",
                s.rows(
                    "SELECT count(*) FROM memory_facts WHERE fact_key IN('incorrect','invalid')"
                ),
                [(0,)],
            )
            ctx.equal(
                "group and person-group retain source owner",
                s.rows(
                    "SELECT f.kind,f.person,e.author FROM memory_facts f JOIN memory_evidence e ON e.fact=f.id WHERE f.fact_key IN('discussion','job') ORDER BY f.kind"
                ),
                [("group", None, s.a), ("person_group", s.b, s.b)],
            )
            ctx.equal(
                "cursor and facts committed together",
                s.rows("SELECT event FROM extraction_watermarks"),
                [(b,)],
            )
            ctx.equal(
                "one logical root and one original physical purchase",
                s.rows(
                    "SELECT count(*),count(distinct root),(SELECT count(*) FROM model_attempts) FROM model_requests"
                ),
                [(1, 1, 1)],
            )
            original = s.rows("SELECT state,trigger_reason FROM extraction_batches")
            statuses = s.rows(
                "SELECT ordinal,reason FROM extraction_results ORDER BY ordinal"
            )
            ctx.write("extraction-wire.json", body)
    cold = Scene(ctx, "cold")
    with Recorder() as http:
        cold.config(http, count=1)
        # The test owner runs the normal ingress and extractor with its own private IPC.
        with ThreadPoolExecutor(max_workers=1) as pool:
            death = pool.submit(
                kill_writer,
                ctx,
                cold.s.data,
                "extract_crash_child",
                "extract-response-before-facts",
                {},
            )
            cold.until(
                lambda: cold.rows("SELECT enabled FROM extraction_policy WHERE enabled")
            )
            source = cold.event(cold.a, "我喜歡河流")
            _, _, q = cold.wire(http)
            answer(
                q, [claim(source, "person", "speaker", "river", "喜歡河流", "喜歡河流")]
            )
            death.result()
        ctx.equal(
            "true process death after payment and before fact commit",
            (
                cold.rows("SELECT state FROM model_attempts"),
                cold.rows("SELECT count(*) FROM memory_facts"),
                cold.rows("SELECT count(*) FROM extraction_watermarks"),
            ),
            ([("response",)], [(0,)], [(0,)]),
        )
        # Deliberately fail fact persistence on the disposable DB. Release the fault
        # only while the actual writer is stopped; the model receipt stays untouched.
        with closing(sqlite3.connect(cold.db)) as db:
            db.execute(
                "CREATE TRIGGER test_fail_insert BEFORE INSERT ON memory_versions BEGIN SELECT RAISE(ABORT,'original fact fault'); END"
            )
            db.commit()
        with cold.s.running():
            time.sleep(0.7)
            ctx.equal(
                "fact-write failure leaves all facts and watermarks untouched",
                (
                    cold.rows("SELECT count(*) FROM memory_facts"),
                    cold.rows("SELECT count(*) FROM extraction_watermarks"),
                    cold.rows("SELECT count(*) FROM model_attempts"),
                ),
                ([(0,)], [(0,)], [(1,)]),
            )
        with closing(sqlite3.connect(cold.db)) as db:
            db.execute("DROP TRIGGER test_fail_insert")
            db.commit()
        with cold.s.running():
            cold.until(lambda: cold.rows("SELECT event FROM extraction_watermarks"))
            ctx.equal(
                "cold local replay consumes the same paid output without another request",
                (
                    cold.rows("SELECT count(*) FROM memory_facts"),
                    cold.rows("SELECT count(*) FROM model_attempts"),
                    http.wires.empty(),
                ),
                ([(1,)], [(1,)], True),
            )
    return {
        "normal": original,
        "faults": faults(ctx),
        "foreground": foreground(ctx),
        "asr": asr_case(ctx),
        "claim_dispositions": statuses,
        "watermark": b,
        "cold": {"fact_source": source, "paid_attempts": 1},
    }
