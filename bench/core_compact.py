"""Observe both publications and the subsequent normal request from one native owner."""

from bench.compact_trace import turns, summary, observed
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def run(ctx, refuse=False):
    o = Owner(ctx, "p-deny" if refuse else "p-twice")
    trace = []
    jobs = []
    try:
        with Recorder() as peer:
            o.config(peer)
            with o.svc.running():
                o.op("person", "set-admin", o.actor, "true")
                for i, (kind, text) in enumerate(turns()):
                    event = o.human(text, group="700")
                    body, q = o.wire(peer)
                    trace.append(
                        {"turn": i, "kind": kind, **observed(body["messages"])}
                    )
                    compact = kind == "public_compact"
                    if i == 0:
                        original_tools = body["tools"]
                    if compact:
                        ctx.equal(
                            "original tool declarations retained at public checkpoint "
                            + str(i),
                            body.get("tools"),
                            original_tools,
                        )
                    rejected = refuse and compact and len(jobs) == 1
                    if rejected:
                        reply(
                            q,
                            calls=[
                                call(
                                    "forbidden-public-send",
                                    "send",
                                    {"text": "FORBIDDEN_COMPACT_SEND"},
                                )
                            ],
                        )
                    else:
                        reply(q, summary(len(jobs) + 1) if compact else "NO_REPLY")
                    o.finished(event)
                    if compact:
                        record = o.rows(
                            "SELECT state,summary FROM compact_jobs ORDER BY rowid DESC LIMIT 1"
                        )
                        ctx.equal(
                            "checkpoint disposition " + str(i),
                            record,
                            [("rejected", None)]
                            if rejected
                            else [("published", summary(len(jobs) + 1))],
                        )
                        jobs.append(record[0][1])
                ledger = o.rows(
                    "SELECT state,count(*) FROM model_attempts GROUP BY state ORDER BY state"
                )
                ctx.write(
                    ("refuse" if refuse else "adopt") + "-compact-attempts.json", ledger
                )
                ctx.equal(
                    "each logical request has exactly one physical attempt",
                    o.rows(
                        "SELECT count(*),count(distinct logical) FROM model_attempts"
                    ),
                    [(15, 15)],
                )
                ctx.equal(
                    "physical outcome of each paid attempt",
                    dict(ledger),
                    {"response": 14, "response_held": 1}
                    if refuse
                    else {"response": 15},
                )
                ctx.equal(
                    "checkpoint log retains actual outcome",
                    o.rows("SELECT state,summary FROM compact_jobs ORDER BY rowid"),
                    [
                        ("published", summary(1)),
                        ("rejected", None) if refuse else ("published", summary(2)),
                    ],
                )
                ctx.equal(
                    "summary cannot dispatch QQ send",
                    o.rows("SELECT count(*) FROM operations WHERE target='send'"),
                    [(0,)],
                )
        return trace
    finally:
        ctx.write(
            "rust-public-compact-refused.json"
            if refuse
            else "rust-public-compact.json",
            {"trace": trace, "publications": jobs},
        )
