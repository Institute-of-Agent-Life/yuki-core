"""Forget one historical identity family while another Work is physically at its provider."""

from contextlib import closing
import json
import os
import sqlite3
import time
import uuid
from urllib.error import HTTPError
from urllib.request import Request, urlopen
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def key():
    return str(uuid.uuid4())


def http(o, path, token=None, data=None):
    req = Request(
        "http://" + o.svc.address + path,
        data=json.dumps(data).encode() if data is not None else None,
        headers={
            "Content-Type": "application/json",
            **({"Authorization": "Bearer " + token} if token else {}),
        },
    )
    try:
        r = urlopen(req, timeout=8)
    except HTTPError as err:
        r = err
    with r:
        v = json.load(r)
        return r.status, v.get("data", v)


def pair(o, person):
    code = o.op("personal", "issue", person, key(), "chat")["code"]
    status, auth = http(
        o, "/client/v1/pair", data={"code": code, "device_label": "controlled phone"}
    )
    assert status == 200
    return auth["token"]


def run(ctx):
    o = Owner(ctx, "forget", 25)
    a = o.actor
    alias, b, c, admin = (key() for _ in range(4))
    marker = "A-private-" + key()
    healthy = "B-independent-" + key()
    config = o.svc.data / "yuki.toml"
    with config.open("a") as f:
        f.write(
            "\n[media]\ngateway_transfer_directory="
            + json.dumps(str(o.svc.data / "media-transfer"))
            + "\n"
        )
    counter = 0

    def record(acct, text, space=None, claim=True, clock=None):
        nonlocal counter
        counter += 1
        stamp = 100 + counter if clock is None else clock
        frame = {
            "source": "family-" + str(counter),
            "account": acct,
            "bot_stage": o.bot["assignment"],
            "reply_bot": "100",
            "kind": "message",
            "occurred": stamp,
            "observed": stamp,
            "segments": [{"type": "text", "text": text}],
        }
        if space:
            frame["space"] = space
        e = o.op("ingress", "record", json.dumps(frame), "--accept")["event"]
        return (e, o.op("ingress", "claim", e)["execution"]) if claim else (e, None)

    def add(x, person, e, k, content):
        return o.op(
            "memory",
            "edit",
            x,
            person,
            key(),
            json.dumps(
                {
                    "action": "add",
                    "subject": {"kind": "person", "person": person},
                    "fact_key": k,
                    "body": content,
                    "confidence": 0.9,
                    "evidence": [{"kind": "event", "id": e, "quote": content}],
                }
            ),
        )

    with o.svc.running():
        for person in [alias, b, c, admin]:
            o.op("person", "create", person)
        o.op("person", "merge", alias, a)
        o.op("person", "set-admin", admin, "true")
        ba = o.op(
            "account", "open", "qq", "compact", "233", 1, "--person", b, "--start", 1
        )
        history = o.op(
            "account",
            "open",
            "qq",
            "compact",
            "244",
            1,
            "--person",
            alias,
            "--start",
            1,
        )
        lost, ax = record(o.user["account"], marker)
        alias_source, _ = record(history["account"], "historical alias private")
        o.op("account", "transfer", history["assignment"], 300, 301, "--person", c)
        survivor, _ = record(
            history["account"], "persists with later unrelated owner", clock=305
        )
        record(o.user["account"], "group observation of A", "700")
        record(ba["account"], "group observation of B", "700")
        sole, bx = record(ba["account"], healthy)
        fact = add(ax, a, lost, "A-origin", marker)["fact"]
        fid = o.rows("SELECT number FROM memory_facts WHERE id=?", fact)[0][0]
        bfact = add(bx, b, sole, "B-own", healthy)["fact"]
        a_device, admin_device, b_device = pair(o, a), pair(o, admin), pair(o, b)
        # Two separately authorized versions refer to the same immutable bytes.
        (o.svc.data / "media-inputs" / "shared.bin").write_bytes(
            b"shared-immutable-object"
        )
        (o.svc.data / "media-inputs" / "private.bin").write_bytes(marker.encode())

        def snap(exec, actor, source):
            return o.op(
                "media",
                "snapshot",
                exec,
                actor,
                "snapshot-" + source,
                source,
                source,
                "file",
            )

        own = snap(ax, a, "private.bin")
        shared = snap(ax, a, "shared.bin")
        independent = snap(bx, b, "shared.bin")
        ctx.equal(
            "two owners share bytes but hold different source versions",
            shared["sha256"],
            independent["sha256"],
        )
        o.op("privacy", "owner", "offline-owner", "true")
        o.op("privacy", "acquire", "offline-owner", "material", lost)
        o.op("privacy", "owner", "offline-owner", "false")
    with Recorder() as model:
        o.config(model)
        with o.svc.running():
            incoming, _ = record(
                ba["account"],
                "recall an authorized shared group participant",
                claim=False,
            )

            def selected(predicate):
                for _ in range(32):
                    body, response = o.wire(model)
                    if predicate(body):
                        return body, response
                    reply(response, "unrelated prior turn finished")
                raise AssertionError("selected physical Main request missing")

            _, q = selected(
                lambda body: "recall an authorized shared group participant"
                in str(body)
            )
            reply(
                q,
                calls=[
                    call(
                        "accept",
                        "task_control",
                        {"action": "accept", "goal": "answer B's original Work"},
                    )
                ],
            )
            msg, q = selected(
                lambda b: any(
                    m.get("tool_call_id") == "accept" for m in b.get("messages", [])
                )
            )
            wid = json.loads(
                next(
                    m["content"]
                    for m in msg["messages"]
                    if m.get("tool_call_id") == "accept"
                )
            )["result"]["work_id"]
            reply(
                q,
                calls=[
                    call("derived", "read_memory", {"view": "fact", "fact_id": fid})
                ],
            )
            paid, q = selected(
                lambda b: any(
                    m.get("tool_call_id") == "derived" for m in b.get("messages", [])
                )
            )
            ctx.check(
                "B Work genuinely consumed A private-source fact body",
                marker
                in next(
                    m["content"]
                    for m in paid["messages"]
                    if m.get("tool_call_id") == "derived"
                ),
            )
            exe = o.rows("SELECT owner_execution FROM work_nodes WHERE id=?", wid)[0][0]
            ctx.check(
                "the incoming event joined the original Work execution",
                bool(
                    o.rows(
                        "SELECT 1 FROM executions WHERE id=? AND (source=? OR EXISTS(SELECT 1 FROM main_inputs WHERE execution=? AND event=?))",
                        exe,
                        incoming,
                        exe,
                        incoming,
                    )
                ),
            )
            attempt = o.rows(
                "SELECT t.id FROM model_attempts t JOIN model_requests r ON r.id=t.logical JOIN activations a ON a.id=r.activation WHERE a.execution=? AND t.state='dispatched' ORDER BY t.rowid DESC LIMIT 1",
                exe,
            )[0][0]
            ctx.check(
                "real paid read retains the private source without exposing its quote",
                o.rows(
                    "SELECT event FROM main_dependencies WHERE execution=? AND event=?",
                    exe,
                    lost,
                )
                == [(lost,)],
            )
            path = "/admin/v1/persons/" + alias + "/forget"
            op = key()
            ctx.equal(
                "ordinary authenticated device cannot forget another Person",
                http(o, path, b_device, {"request_id": op})[0],
                403,
            )
            status, receipt = http(o, path, admin_device, {"request_id": op})
            ctx.equal(
                "authenticated original admin route completes logical closure",
                status,
                200,
            )
            ctx.equal(
                "late physical result for B cannot publish A private material",
                receipt["deleted"],
                True,
            )
            reply(q, marker + "-LATE")
            o.until(
                lambda: o.rows(
                    "SELECT id FROM model_attempts WHERE id=? AND diagnostic='source_erased' AND received_at IS NOT NULL AND response IS NULL",
                    attempt,
                )
            )
            ctx.equal(
                "old phone cannot read after family forget",
                http(o, "/client/v1/me", a_device)[0],
                401,
            )
            ctx.equal(
                "unaffected B phone still authenticates",
                http(o, "/client/v1/me", b_device)[0],
                200,
            )
            repeated = http(o, path, admin_device, {"request_id": op})
            ctx.check(
                "same admin operation returns original identity and deletion",
                repeated[0] == 200 and repeated[1]["person_id"] == alias,
            )
            global_key = "admin-forget:" + admin + ":" + op
            report = o.op("privacy", "forget-status", global_key)
            ctx.check(
                "author and original-stage private events selected, later occupant preserved",
                lost in report["sources"]
                and alias_source in report["sources"]
                and survivor not in report["sources"]
                and sole not in report["sources"],
            )
            ctx.equal(
                "quiet disabled owner remains pending",
                report["owners_pending"],
                ["offline-owner"],
            )
            ctx.check(
                "logical versions detached but shared bytes survive",
                o.rows(
                    "SELECT digest FROM media_versions WHERE id=?",
                    shared["artifact_id"],
                )
                == [(None,)]
                and o.rows(
                    "SELECT digest FROM media_versions WHERE id=?",
                    independent["artifact_id"],
                )
                == [(shared["sha256"],)],
            )
            ctx.equal(
                "unrelated B fact remains readable",
                o.op("memory", "read", bx, b, "--fact", bfact)["items"][0]["body"],
                healthy,
            )
            states = o.rows(
                "SELECT id,enabled FROM persons WHERE id IN(?,?,?) ORDER BY id",
                a,
                alias,
                c,
            )
            ctx.check(
                "old family disabled, unrelated current owner remains enabled",
                dict(states) == {a: 0, alias: 0, c: 1},
            )
            ctx.check(
                "other owner's private event body preserved",
                "persists with later unrelated owner"
                in str(o.rows("SELECT envelope FROM events WHERE id=?", survivor)),
            )
            # Verify actual application storage; generated peers, SQLite free pages and user copies have different boundaries.
            dbpath = o.svc.data / "core.sqlite"
            with closing(sqlite3.connect(dbpath.as_uri() + "?mode=ro", uri=True)) as db:
                tables = [
                    r[0]
                    for r in db.execute(
                        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND sql NOT LIKE 'CREATE VIRTUAL TABLE%'"
                    )
                ]
                survivors = []
                for table in tables:
                    for row in db.execute(
                        'SELECT * FROM "' + table.replace('"', '""') + '"'
                    ):
                        if any(
                            (
                                marker in v
                                if isinstance(v, str)
                                else (
                                    marker.encode() in v
                                    if isinstance(v, bytes)
                                    else False
                                )
                            )
                            for v in row
                        ):
                            survivors.append(table)
                            break
            ctx.write("canary-storage-before-gc.json", {"tables": survivors})
            ctx.equal(
                "unique private marker absent across core records including trace, cache and vectors",
                [t for t in survivors if t != "media_objects"],
                [],
            )
        with o.svc.running():
            collected = o.op("media", "gc")
            recovered = o.op("privacy", "forget-status", global_key)
            ctx.equal(
                "managed unreferenced blob no longer contains original bytes after cold GC",
                o.rows(
                    "SELECT count(*) FROM media_objects WHERE instr(bytes,?)>0",
                    marker.encode(),
                ),
                [(0,)],
            )
            ctx.check(
                "cold GC resolves only unreferenced owned paths",
                recovered["managed_storage"]["unreferenced_blobs_pending"] == 0
                and recovered["managed_storage"]["transfer_paths_pending"] == 0
                and recovered["managed_storage"]["shared_blobs_retained"] == 1,
            )
            ctx.equal(
                "pending owner survives cold restart",
                recovered["owners_pending"],
                ["offline-owner"],
            )
            ctx.check(
                "erasure replay cannot produce new model attempt or recover old body",
                not o.rows(
                    "SELECT response FROM model_attempts WHERE response LIKE ?",
                    "%" + marker + "%",
                ),
            )
    from tests.harness.owner_cleanup import exercise

    owner_log = exercise(ctx)
    return {
        "logical": report,
        "owner_lifecycle": owner_log,
        "after_cold_gc": recovered,
        "surviving_person": c,
        "cross_work": wid,
        "affected_source": lost,
        "paid_model_canary": "removed",
        "shared_digest": shared["sha256"],
        "private_version": own["artifact_id"],
        "gc": collected,
    }
