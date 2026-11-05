"""Shared native owner and exact-wire helpers for public and private Compact cases."""

import hashlib
import json
import sqlite3
import time
from contextlib import closing
from tests.harness.service import Service, previous_schema


def reply(q, text="", calls=None):
    m = {"role": "assistant", "content": None if calls else text}
    if calls:
        m["tool_calls"] = calls
    q.put(
        (
            {
                "choices": [
                    {"message": m, "finish_reason": "tool_calls" if calls else "stop"}
                ]
            },
            0,
        )
    )


def call(id, name, args):
    return {
        "id": id,
        "type": "function",
        "function": {"name": name, "arguments": json.dumps(args, ensure_ascii=False)},
    }


def approx(body):
    view = {
        "model": body["model"],
        "messages": body["messages"],
        "tool_choice": body["tool_choice"],
        "max_output_tokens": body["max_tokens"],
        "tools": [t["function"] for t in body["tools"]],
    }
    raw = json.dumps(view, ensure_ascii=False)
    ascii = sum(c.isascii() for c in raw)
    return (ascii + 2) // 3 + 2 * (len(raw) - ascii)


def sha(v):
    return hashlib.sha256(
        json.dumps(v, ensure_ascii=False, sort_keys=True).encode()
    ).hexdigest()


class Owner:
    def __init__(self, ctx, path, prior_schema=16):
        self.ctx = ctx
        self.svc = Service(ctx, ctx.working / path)
        previous_schema(ctx, self.svc.data, prior_schema)
        (self.svc.data / "SOUL.md").write_text(
            "Fixed public character.", encoding="utf-8"
        )
        ctx.check(
            "additive migration to stored public fragments",
            self.svc.command("migrate")["schema_version"] >= 17,
        )
        self.actor = "00000000-0000-4000-8000-000000000022"
        self.seq = 0
        with self.svc.running():
            ctx.write("sqlite-build.json", self.op("database-info"))
            self.op("person", "create", self.actor)
            self.user = self.op(
                "account",
                "open",
                "qq",
                "compact",
                "200",
                1,
                "--person",
                self.actor,
                "--start",
                1,
            )
            self.bot = self.op(
                "account", "open", "qq", "compact", "100", 1, "--start", 1
            )

    def op(self, *args):
        return self.svc.command("operator", *map(str, args))

    def rows(self, sql, *args):
        with closing(
            sqlite3.connect(
                (self.svc.data / "core.sqlite").as_uri() + "?mode=ro", uri=True
            )
        ) as db:
            return db.execute(sql, args).fetchall()

    def human(self, text, group=None):
        self.seq += 1
        frame = {
            "source": "P-" + str(self.seq),
            "account": self.user["account"],
            "bot_stage": self.bot["assignment"],
            "reply_bot": "100",
            "occurred": 100 * self.seq,
            "observed": 100 * self.seq,
            "kind": "message",
            "sender": {"nickname": "Alice"},
            "segments": [{"type": "text", "text": text}],
        }
        if group:
            frame["space"] = group
        return self.op(
            "ingress", "record", json.dumps(frame, ensure_ascii=False), "--accept"
        )["event"]

    def config(self, http, extra=""):
        with (self.svc.data / "yuki.toml").open("a") as cfg:
            cfg.write(
                f'\n[main]\nendpoint="{http.url}"\nbinding_endpoint="http://127.0.0.1:{http.server.server_port}/binding"\nmodel="fixture"\nname="Yuki"\nmax_output=128\nroot_slots=8\nmax_input_units=4000000\nmerge_ms=2\n'
                + extra
            )

    def until(self, fn):
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            result = fn()
            if result:
                return result
            time.sleep(0.02)
        raise AssertionError(
            str(
                self.rows(
                    "SELECT x.state,t.diagnostic FROM executions x LEFT JOIN main_turns t ON x.id=t.execution"
                )
            )
        )

    def wire(self, http):
        try:
            raw, q = http.receive()
            return json.loads(raw), q
        except Exception:
            self.ctx.write(
                "compact-diagnostic.json",
                {
                    "turns": self.rows(
                        "SELECT x.state,t.diagnostic FROM executions x JOIN main_turns t ON t.execution=x.id"
                    ),
                    "jobs": self.rows(
                        "SELECT id,state,diagnostic,last_item FROM compact_jobs"
                    ),
                    "attempts": self.rows(
                        "SELECT state,diagnostic FROM model_attempts"
                    ),
                },
            )
            raise

    def finished(self, event):
        self.until(
            lambda: self.rows(
                "SELECT id FROM executions WHERE source=? AND state='completed'", event
            )
        )
