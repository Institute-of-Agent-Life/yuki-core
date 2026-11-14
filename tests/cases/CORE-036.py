"""Real Rust admission and SELF sends through installed SDK and OneBot peers."""

import base64
import hashlib
import importlib.util
import json
import os
import queue
import sqlite3
import subprocess
import sys
from contextlib import closing
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
from tests.harness.compact import reply, call
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer as QQ


def load(ctx, card):
    spec = importlib.util.spec_from_file_location(
        card, ctx.root / "tests/cases" / f"{card}.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run(ctx):
    previous = load(ctx, "CORE-018")
    aux = load(ctx, "CORE-050")
    sdk = ctx.working / "sdk"
    build = subprocess.run(
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
    if build.returncode:
        ctx.write("sdk-failure.json", {"stderr": build.stderr})
    ctx.equal("both SDK packages verify participation methods", build.returncode, 0)
    s = previous.Scene(ctx)
    space = json.dumps(["qq", "schedule", "700"], separators=(",", ":"))
    with s.svc.running():
        entry = s.op("entry", "group", space)["entry"]
        other_topic = s.op("entry", "group", space, "--topic", "observed-other")["entry"]
        foreign = s.op("entry", "group", json.dumps(["qq", "schedule", "701"]))["entry"]
        open_entry = s.op("entry", "group", json.dumps(["qq", "schedule", "702"]))[
            "entry"
        ]
    folder = ctx.working / "participant"
    folder.mkdir()
    methods = json.loads(
        (ctx.root / "protocol/plugin/participation/methods.json").read_bytes()
    )["methods"]
    grants = ["observe.metadata", "forget.owner", "llm.complete", "llm.result", *methods]
    env = {**os.environ, "YUKI_SCHEDULE_PEER": "offline-participation"}
    proofs = {}

    def record(key, text="original observation", person=None, accept=False, *,
               native=None, quoted=(), group=space, topic=None, receiver=None, mentions=()):
        frame = {
            "source": key,
            "native_id": native,
            "account": s.handles[person or s.a]["account"],
            "bot_stage": receiver or s.bot["assignment"],
            "reply_bot": "100",
            "space": group,
            "topic": topic,
            "kind": "message",
            "observed": 200,
            "occurred": 200,
            "segments": [{"type": "text", "text": text}, *(
                {"type": "reply", "native_id": target} for target in quoted
            ), *mentions],
        }
        args = ["ingress", "record", json.dumps(frame)]
        if accept:
            args += ["--accept"]
        return s.op(*args)["event"]

    with Recorder() as http, Recorder("/systemone") as jev:
        config = s.svc.data / "yuki.toml"
        with config.open("a", encoding="utf-8") as f:
            f.write(
                f'\n[main]\nendpoint={json.dumps(http.url)}\nmodel="fixture"\nname="Yuki"\nmerge_ms=2\nroot_slots=24\nmax_input_units=4000000\nmax_output=512\n[qq]\nnamespace="schedule"\ndesignated_ingress="100"\n[qq.groups]\n"700"="paused"\n"702"="open"\n[[qq.bots]]\nself_id="100"\nstage={json.dumps(s.bot["assignment"])}\ntoken_env="YUKI_SCHEDULE_PEER"\n'
            )
            f.write(
                '\n[[plugins]]\nid="participation"\nnamespace="participation"\nprogram='
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
                + json.dumps(grants)
                + "\n[plugins.participation]\nentries="
                + json.dumps([entry, open_entry, other_topic])
                + "\n[plugins.participation.self_scenes]\n"
                + json.dumps(entry)
                + "="
                + json.dumps(s.bot["assignment"])
                + "\n"
                + json.dumps(open_entry)
                + "="
                + json.dumps(s.bot["assignment"])
                + "\n"
                + '\n[plugins.aux_routes.review]\npurposes=["review"]\norigins=["event"]\nentries='
                + json.dumps([entry])
                + '\n[plugins.aux_routes.review.profile]\nendpoint='
                + json.dumps(http.url)
                + '\nprotocol="chat"\nmodel="review-fixture"\nname="Jev"\nmerge_ms=0\n'
                + 'root_slots=2\nmax_input_units=100000\nmax_output=128\n'
                + '\n[plugins.aux_routes.jev]\npurposes=["review"]\norigins=["event"]\nentries='
                + json.dumps([entry])
                + '\n[plugins.aux_routes.jev.profile]\nendpoint=' + json.dumps(jev.url)
                + '\nmodel="jev-request-fixture"\nname="Jev"\nmerge_ms=0\n'
                + 'root_slots=2\nmax_input_units=100000\nmax_output=128\nkey_env="YUKI_SCHEDULE_PEER"\n'
                + '[plugins.aux_routes.jev.jev]\nresponse_model="jev-response-fixture"\ntimeout_seconds=12.0\n'
            )
        p = aux.Peer(s, folder)
        with s.svc.running(environment=env):
            s.until((folder / "ready.json").exists)
            opened = p.call(
                "participation.self_propose",
                {
                    "entry": open_entry,
                    "key": "configured-open",
                    "text": "ordinary configured group",
                },
            )
            _, q = s.wire(http)
            reply(q, "quiet configured group")
            s.until(
                lambda: s.rows(
                    "SELECT id FROM executions WHERE id=? AND state='completed'",
                    opened["execution"],
                )
            )
            ctx.equal(
                "configured open group needs no recovery command",
                s.rows(
                    "SELECT count(*) FROM recovered_groups WHERE native_group='702'"
                ),
                [(0,)],
            )
            src = record("race-source")
            p.call("participation.promote", {"event": src}, False)
            proposal = {
                "entry": entry,
                "key": "self-original",
                "text": "start from the original SELF scene",
            }
            p.call("participation.self_propose", proposal, False)
            p.call("participation.sources", {"entry": foreign}, False)
            ctx.equal(
                "paused group has no accepted source",
                s.rows("SELECT count(*) FROM accepted_sources"),
                [(0,)],
            )
            qq = QQ(s.svc.address, "100", "offline-participation")
            # Recover a specific receiver via the real authenticated group command.
            qq.send(
                {
                    "self_id": 100,
                    "user_id": 201,
                    "message_type": "group",
                    "post_type": "message",
                    "group_id": 700,
                    "time": 100,
                    "message_id": 36001,
                    "message": "/ai admin group current enable",
                }
            )
            probe = qq.receive()
            ctx.equal(
                "original SELF membership probe",
                probe["action"],
                "get_group_member_info",
            )
            qq.send(
                {
                    "echo": probe["echo"],
                    "status": "ok",
                    "retcode": 0,
                    "data": {"group_id": 700, "user_id": 100},
                }
            )
            s.deliver(qq, "send_group_msg", 36002)
            page = p.call("participation.sources", {"entry": entry})
            item = next(i for i in page["items"] if i["event"] == src)
            raw = p.call(
                "participation.source", {"event": src, "version": item["version"]}
            )
            ctx.equal(
                "source page has original content",
                json.loads(base64.b64decode(raw["data"]))["source"],
                "race-source",
            )
            origin_query = {"event": src, "version": raw["version"], "include_origin": True}
            origin_page = p.call("participation.source", origin_query)
            first_origin = {
                "entry": entry, "original_person": s.a,
                "actor_assignment": s.handles[s.a]["assignment"], "current_person": s.a,
                "observed_generation": 1, "current_generation": 1,
            }
            ctx.equal("source origin comes from the actual ingress row", origin_page["origin"], first_origin)
            ctx.equal("opting in leaves the original envelope and version intact",
                      {key: value for key, value in origin_page.items() if key != "origin"}, raw)
            p.call("participation.source", {**origin_query, "entry": foreign}, False)
            ctx.equal(
                "preview acquires original owner obligation",
                s.rows(
                    "SELECT source FROM source_holds WHERE owner=? AND item=?",
                    "participation",
                    "participation:" + str(src),
                ),
                [(src,)],
            )
            p.call("participation.source", {"event": src, "version": "wrong"}, False)
            qq_message = {
                "self_id": 100, "message_type": "group", "post_type": "message",
                "group_id": 700, "time": 100,
            }
            qq.send({**qq_message, "user_id": 202, "message_id": 36011,
                     "message": [{"type": "text", "data": {"text": "original quoted message"}}]})
            anchor = s.until(lambda: s.rows(
                "SELECT event_id FROM event_locators WHERE receiver=? AND native_id='36011'",
                s.bot["assignment"],
            ))[0][0]
            qq.send({**qq_message, "user_id": 200, "message_id": 36012, "message": [
                {"type": "reply", "data": {"id": "36011"}},
                {"type": "text", "data": {"text": "original reply"}},
            ]})
            quoted = s.until(lambda: s.rows(
                "SELECT event_id FROM event_locators WHERE receiver=? AND native_id='36012'",
                s.bot["assignment"],
            ))[0][0]
            anchor_page = p.call("participation.source", {"event": anchor})
            plain_reply = p.call("participation.source", {"event": quoted})
            reply_request = {"event": quoted, "version": plain_reply["version"], "include_reply": True}
            quoted_page = p.call("participation.source", reply_request)
            reference = {"event": anchor, "version": anchor_page["version"]}
            ctx.equal("one proven original human reply", quoted_page["reply_to"], reference)
            ctx.equal("reply lookup leaves the source bytes and version intact",
                      {key: value for key, value in quoted_page.items() if key != "reply_to"}, plain_reply)
            p.call("participation.source", {**reply_request, "version": "wrong"}, False)

            qq.send({**qq_message, "user_id": 200, "message_id": 36013, "message": [
                {"type": "text", "data": {"text": "雪🙂 literal @Yuki "}},
                {"type": "at", "data": {"qq": 202, "name": "原群友"}},
                {"type": "at", "data": {"qq": 100}},
                {"type": "at", "data": {"qq": 202, "name": "原群友"}},
                {"type": "at", "data": {"qq": "all"}},
                {"type": "at", "data": {"qq": 999}},
            ]})
            mentioned = s.until(lambda: s.rows(
                "SELECT event_id FROM event_locators WHERE receiver=? AND native_id='36013'",
                s.bot["assignment"],
            ))[0][0]
            _, q = s.wire(http)
            reply(q, "quiet direct mention")
            s.finished(mentioned)
            plain_mention = p.call("participation.source", {"event": mentioned})
            mention_request = {
                "event": mentioned, "version": plain_mention["version"], "mention_cursor": 0,
            }
            mention_page = p.call("participation.source", mention_request)
            ctx.equal("mention lookup keeps original content and its version",
                      {key: value for key, value in mention_page.items() if key != "mentions"}, plain_mention)
            frozen_mentions = mention_page["mentions"]
            mention_rows = frozen_mentions["items"]
            ctx.equal("native repetition and all-member order remain exact",
                      [row["kind"] for row in mention_rows], ["member", "yuki", "member", "all", "member"])
            member = {key: mention_rows[0][key] for key in (
                "account_id", "assignment_id", "person_id", "presence_id", "display_name",
            )}
            ctx.equal("member has the original account, assignment and person", member, {
                "account_id": s.handles[s.c]["account"], "assignment_id": s.handles[s.c]["assignment"],
                "person_id": s.c, "presence_id": None, "display_name": "原群友",
            })
            ctx.equal("only the original bot assignment has presence",
                      (mention_rows[1]["assignment_id"], mention_rows[1]["presence_id"], mention_rows[1]["person_id"]),
                      (s.bot["assignment"], s.bot["assignment"], None))
            ctx.equal("unknown native account has no invented person or assignment",
                      (mention_rows[4]["assignment_id"], mention_rows[4]["person_id"], mention_rows[4]["presence_id"]),
                      (None, None, None))
            mention_text = json.loads(base64.b64decode(plain_mention["data"]))["text"]
            ctx.equal("recorded offsets use the rendered Unicode text",
                      [mention_text[row["text_start"]:row["text_end"]] for row in mention_rows],
                      ["[提及202]", "[提及Agent]", "[提及202]", "[提及全体成员]", "[提及999]"])
            ctx.equal("literal @ is outside the native span", mention_text[:mention_rows[0]["text_start"]],
                      "雪🙂 literal @Yuki ")
            literal = record("literal-at", "@Yuki")
            ctx.equal("literal @ alone yields no original annotation",
                      p.call("participation.source", {"event": literal, "mention_cursor": 0})["mentions"]["items"], [])
            p.call("participation.source", {**mention_request, "mention_cursor": 6}, False)

            long_mentions = record("many-native-mentions", mentions=tuple(
                {"type": "mention", "account": s.handles[s.c]["account"], "native": "202", "label": "member"}
                for _ in range(19)
            ))
            first_mentions = p.call("participation.source", {"event": long_mentions, "mention_cursor": 0})
            first_page = first_mentions["mentions"]
            second_page = p.call("participation.source", {
                "event": long_mentions, "version": first_mentions["version"],
                "mention_cursor": first_page["next"],
            })["mentions"]
            ctx.equal("large mention arrays page without repeating or skipping",
                      (len(first_page["items"]), first_page["complete"], len(second_page["items"]),
                       second_page["complete"], second_page["next"]), (16, False, 3, True, 19))
            ctx.equal("unrendered offsets remain unknown",
                      (second_page["items"][0]["text_start"], second_page["items"][0]["text_end"]), (None, None))

            other_receiver = s.op("account", "open", "qq", "schedule", "101", 1, "--start", 1)
            record("different-receiver-anchor", native="other-receiver", receiver=other_receiver["assignment"])
            record("different-topic-anchor", native="other-topic", topic="unrelated")
            record("different-group-anchor", native="other-group",
                   group=json.dumps(["qq", "schedule", "701"], separators=(",", ":")))
            record("duplicate-anchor-one", native="same-native")
            record("duplicate-anchor-two", native="same-native")
            unsafe_quotes = {
                "other-receiver": ("other-receiver",),
                "other-topic": ("other-topic",),
                "other-group": ("other-group",),
                "ambiguous": ("same-native",),
                "multiple": ("36011", "36011"),
                "future": ("future-anchor",),
                "absent": (),
            }
            unsafe_events = {case: record(f"reply-{case}", quoted=targets)
                             for case, targets in unsafe_quotes.items()}
            record("future-anchor", native="future-anchor")
            for case, event in unsafe_events.items():
                actual_reply = p.call("participation.source", {"event": event, "include_reply": True})
                ctx.equal(f"{case} cannot prove an original reply", actual_reply["reply_to"], None)
            # Rule admission races two invitation callbacks over the exact stored ID.
            a = p.submit("participation.promote", {"event": src, "expected_generation": 1})
            b = p.submit("participation.promote", {"event": src})
            with ThreadPoolExecutor(max_workers=1) as pool:
                same = pool.submit(
                    record, "race-source", "original observation", None, True
                )
                first = p.receive(a)
                second = p.receive(b)
                ctx.equal("rule keeps original transport ID", same.result(), src)
            ctx.equal(
                "one accepted event across rule and invitations",
                s.rows("SELECT count(*) FROM accepted_sources WHERE event_id=?", src),
                [(1,)],
            )
            _, q = s.wire(http)
            reply(q, "private reasoning only")
            s.finished(src)
            feedback = p.call("participation.feedback", {"event": src})
            ctx.check(
                "quiet is actual completed execution without send",
                feedback["executions"][0]["quiet"],
            )
            proofs["promotion"] = {
                "event": src,
                "receipts": [first, second],
                "feedback": feedback,
            }
            # An ordinary promoted event resumes the original armed Work.
            start = record("work-origin", "wait for another real human input")
            p.call("participation.promote", {"event": start})
            _, q = s.wire(http)
            reply(
                q,
                calls=[
                    call(
                        "accept-work",
                        "task_control",
                        {"action": "accept", "goal": "original human Work"},
                    )
                ],
            )
            body, q = s.wire(http)
            work = json.loads(
                next(
                    m["content"]
                    for m in body["messages"]
                    if m.get("tool_call_id") == "accept-work"
                )
            )["result"]["work_id"]
            reply(
                q,
                calls=[
                    call(
                        "wait-human",
                        "task_control",
                        {"action": "wait", "conditions": [{"kind": "conversation"}]},
                    )
                ],
            )
            s.until(
                lambda: s.rows(
                    "SELECT id FROM work_waits WHERE work=? AND state='armed'", work
                )
            )
            answer = record("original-answer", "answer for original human Work")
            p.call("participation.promote", {"event": answer})
            body, q = s.wire(http)
            ctx.check(
                "promoted input reaches original Work",
                "answer for original human Work" in str(body),
            )
            ctx.equal(
                "one matching original Work wake",
                s.rows("SELECT work FROM work_wait_routed WHERE event=?", answer),
                [(work,)],
            )
            reply(
                q,
                calls=[
                    call(
                        "complete-work",
                        "task_control",
                        {"action": "complete", "result": "done"},
                    )
                ],
            )
            s.until(
                lambda: s.rows(
                    "SELECT id FROM work_nodes WHERE id=? AND status='completed'", work
                )
            )
            proofs["work"] = {
                "work": work,
                "source": start,
                "answer": answer,
                "feedback": p.call("participation.feedback", {"event": answer}),
            }
            # The most recent member is deliberately different from the original human.
            unrelated = record("other-member", "nearby group member", s.c)
            admitted = p.call("participation.self_propose", proposal)
            self_exec = admitted["execution"]
            body, q = s.wire(http)
            ctx.equal(
                "SELF has no borrowed person or event",
                s.rows(
                    "SELECT source,original_actor FROM executions WHERE id=?", self_exec
                ),
                [(None, None)],
            )
            ctx.check(
                "SELF model input excludes unrelated member",
                "nearby group member" not in str(body),
            )
            reply(
                q,
                calls=[
                    call(
                        "actual-self-send",
                        "send",
                        {"text": "original SELF group delivery"},
                    )
                ],
            )
            delivered = s.deliver(qq, "send_group_msg", 36003)
            ctx.equal("SELF sends to pinned group", str(delivered["group_id"]), "700")
            _, q = s.wire(http)
            reply(q, "finished")
            s.until(
                lambda: s.rows(
                    "SELECT id FROM executions WHERE id=? AND state='completed'",
                    self_exec,
                )
            )
            actual = p.call("participation.feedback", {"key": proposal["key"]})
            ctx.equal(
                "feedback reports a confirmed physical part",
                actual["executions"][0]["sends"][0]["confirmed_parts"],
                1,
            )
            ctx.check("actual send is not quiet", not actual["executions"][0]["quiet"])
            self_anchor = s.rows(
                "SELECT d.event FROM personal_deliveries d JOIN social_outbound o ON o.id=d.outbound "
                "WHERE o.namespace=? AND o.locator='36003'", self_exec,
            )[0][0]
            self_reply = record("reply-to-confirmed-self", quoted=("36003",))
            read_self_reply = {"event": self_reply, "include_reply": True, "include_self": True}
            ctx.equal("existing human-only read omits confirmed SELF",
                      p.call("participation.source", {"event": self_reply, "include_reply": True})["reply_to"], None)
            self_reference = p.call("participation.source", read_self_reply)["reply_to"]
            ctx.equal("quote uses the original confirmed SELF ledger event", self_reference["event"], self_anchor)
            self_page = p.call("participation.source", {**self_reference, "include_self": True})
            self_body = json.loads(base64.b64decode(self_page["data"]))
            ctx.equal("SELF read preserves the published delivery identity and text",
                      (self_body["event"], self_body["kind"], self_body["text"], self_body["receiver"],
                       self_body["generation"]),
                      (self_anchor, "self", "original SELF group delivery", s.bot["assignment"], 1))
            ctx.equal("the participation owner retains the original delivery source",
                      s.rows("SELECT source FROM source_holds WHERE owner=? AND item=? AND source=?",
                             "participation", "participation:" + str(self_reply), self_anchor), [(self_anchor,)])
            p.call("participation.source", self_reference, False)
            p.call("participation.promote", {"event": self_anchor}, False)
            wrong_topic = record("self-reply-wrong-topic", quoted=("36003",), topic="observed-other")
            ctx.equal("authorized topic cannot select a delivery from another entry",
                      p.call("participation.source", {**read_self_reply, "event": wrong_topic})["reply_to"], None)
            colliding = record("ambiguous-self-locator", native="36003", person=s.c)
            ambiguous = record("ambiguous-self-quote", quoted=("36003",))
            ctx.equal("a confirmed SELF and another original locator stay ambiguous before any echo",
                      p.call("participation.source", {**read_self_reply, "event": ambiguous})["reply_to"], None)
            ctx.check("colliding observation is distinct from the published SELF",
                      colliding != self_anchor)
            qq.send({**qq_message, "user_id": 100, "post_type": "message_sent",
                     "message_id": 36003, "message": "original SELF group delivery"})
            s.until(lambda: s.rows(
                "SELECT echo_event FROM social_outbound WHERE locator='36003' AND echo_event IS NOT NULL"
            ))
            ctx.equal("a late authentic echo does not replace the confirmed SELF event",
                      p.call("participation.source", read_self_reply)["reply_to"], self_reference)
            early_echo = {"event": self_anchor, "include_self": True, "version": self_reference["version"]}
            ctx.equal("SELF content and hash are invariant after late echo",
                      p.call("participation.source", early_echo), self_page)
            duplicate = p.call("participation.self_propose", proposal)
            ctx.equal(
                "same SELF proposal keeps execution",
                duplicate,
                {"execution": self_exec, "newly_accepted": False},
            )
            p.call("participation.self_propose", {**proposal, "text": "changed"}, False)
            p.call("participation.promote", {"event": 999999}, False)
            p.call("participation.feedback", {"event": unrelated}, False)
            proofs["self"] = {
                "proposal": proposal,
                "admitted": admitted,
                "feedback": actual,
                "route": delivered,
                "source": self_reference,
            }
            delivery_origin = record("erasable-delivery-origin", "human input for a reply", s.c)
            p.call("participation.promote", {"event": delivery_origin})
            _, q = s.wire(http)
            reply(q, calls=[call("send-for-erasure", "send", {"text": "retained derived text"})])
            s.deliver(qq, "send_group_msg", 36004)
            _, q = s.wire(http)
            reply(q, "finished")
            derived_focus = record("reply-to-erasable-delivery", quoted=("36004",))
            derived_request = {**read_self_reply, "event": derived_focus}
            native_review = {
                "origin": {"event": derived_focus}, "call_id": "review-original-self-anchor",
                "route": "jev", "purpose": "review",
                "jev": {
                    "state": {"focus": {"event_id": str(derived_focus)}, "context": []},
                    "questions": {"floor_state": {"type": "choice", "instructions": "Who should respond?",
                                                  "criteria": {"yuki": "Yuki", "unknown": "Unknown"}}},
                },
            }
            derived = p.call("participation.source", derived_request)["reply_to"]
            ctx.check("a human-triggered delivery has a readable canonical SELF source", derived is not None)
            derived_page = {**derived, "include_self": True}
            p.call("participation.source", derived_page)
            native_review["sources"] = [derived["event"]]
            native_review["jev"]["state"]["context"] = [{"event_id": str(derived["event"]),
                                                           "text": "retained derived text"}]
            requests_before = s.rows("SELECT count(*) FROM host_aux_requests")
            p.call("llm.complete", {**native_review, "origin": {"event": self_reply}}, False)
            p.call("llm.complete", {**native_review, "origin": {"event": derived["event"]}}, False)
            ctx.equal("unrelated human and SELF origins reserve no auxiliary request",
                      s.rows("SELECT count(*) FROM host_aux_requests"), requests_before)
            ctx.check("unrelated human and SELF origins dispatch no Jev request", jev.wires.empty())
            review_call = p.submit("llm.complete", native_review)
            try:
                wire, channel = jev.receive(timeout=6)
            except queue.Empty as error:
                ctx.write("jev-self-undispatched.json", p.receive(review_call, None))
                raise AssertionError("the exact retained SELF reply must dispatch through Jev") from error
            ctx.equal("native Jev receives the retained SELF context",
                      json.loads(wire), {**native_review["jev"], "model": "jev-request-fixture"})
            review_receipt = p.call("llm.complete", native_review)
            ctx.equal("same-key SELF-context review keeps the physical request in flight",
                      review_receipt["state"], "dispatched")
            erased_delivery = s.op("privacy", "erase", "participation-delivery-origin", s.c,
                                   json.dumps([delivery_origin]))
            ctx.equal("erasure covers the retained delivery derived from the original input",
                      erased_delivery["sources"], [delivery_origin, derived["event"]])
            obligations = s.rows("SELECT sources FROM owner_erasures WHERE watermark=? AND owner='participation'",
                                 erased_delivery["watermark"])
            ctx.equal("the owner receives the original derived delivery ID in its erasure obligation",
                      [json.loads(row[0]) for row in obligations], [erased_delivery["sources"]])
            s.until(lambda: not s.op("privacy", "pending", "participation")["obligations"])
            ctx.equal("erasing the original input removes the retained SELF anchor",
                      p.call("participation.source", derived_request)["reply_to"], None)
            p.call("participation.source", derived_page, False)
            jev_response = {"model": "jev-response-fixture", "answers": {"floor_state": {
                "type": "choice", "choice": "yuki", "probabilities": {"yuki": 1.0, "unknown": 0.0}}},
                "usage": {"input_tokens": 31, "output_tokens": 5}}
            channel.put((jev_response, 0))
            p.receive(review_call, None)
            late = p.call("llm.result", {"request": review_receipt["request"]})
            ctx.equal("erased SELF context retains Jev usage without answer bytes",
                      (late["size"], late["receipt"]["state"], late["receipt"]["erased"], late["receipt"]["usage"]),
                      (0, "response_held", True, {"input": 31, "output": 5}))
            original_attempts = s.rows("SELECT state,response FROM model_attempts WHERE logical=?",
                                      review_receipt["logical"])
            ctx.equal("erased SELF context has exactly one physical attempt and no stored response",
                      original_attempts, [("response_held", None)])
            proofs["self"]["erasure"] = {"origin": delivery_origin, "derived": derived,
                                          "invalidated": erased_delivery["sources"], "jev": late["receipt"]}
            # Erased source is unavailable even though this generation has seen it.
            erase = record("erase-before-promote", "erasure canary", s.c)
            p.call("participation.source", {"event": erase})
            s.op("privacy", "erase", "participation-source", s.c, json.dumps([erase]))
            s.until(
                lambda: not s.op("privacy", "pending", "participation")["obligations"]
            )
            p.call("participation.promote", {"event": erase}, False)
            p.call("participation.source", {"event": erase}, False)
            p.call("participation.source", {"event": erase, "include_origin": True}, False)
            disabled = record("disabled-before-promote", "original disabled actor", s.c)
            s.op("person", "set-enabled", s.c, "false")
            p.call("participation.promote", {"event": disabled}, False)
            p.call("participation.source", {"event": disabled, "include_origin": True}, False)
            ctx.equal("disabled original author blocks the reference",
                      p.call("participation.source", reply_request)["reply_to"], None)
            s.op("person", "set-enabled", s.c, "true")
            proofs["erasure"] = {
                "event": erase,
                "accepted": s.rows(
                    "SELECT event_id FROM accepted_sources WHERE event_id=?", erase
                ),
            }
            reset = record("reset-window", "/ai new", accept=True)
            s.until(lambda: s.rows("SELECT event FROM main_controls WHERE event=?", reset))
            for generation in (1, 2):
                p.call("participation.promote", {"event": unrelated, "expected_generation": generation}, False)
            ctx.equal("a stale observation cannot start a new turn after reset",
                      s.rows("SELECT event_id FROM accepted_sources WHERE event_id=?", unrelated), [])
            fresh = record("new-window-observation", "observed without Main")
            fresh_origin = p.call("participation.source", {"event": fresh, "include_origin": True})["origin"]
            ctx.equal("an unpromoted event records the new generation", fresh_origin,
                      {**first_origin, "observed_generation": 2, "current_generation": 2})
            past_quote = record("reply-before-reset", quoted=("36011",))
            ctx.equal("a reply in the new generation cannot take an old anchor",
                      p.call("participation.source", {"event": past_quote, "include_reply": True})["reply_to"], None)
            old_self = record("self-reply-across-reset", quoted=("36003",))
            ctx.equal("new generation cannot select a prior SELF delivery",
                      p.call("participation.source", {**read_self_reply, "event": old_self})["reply_to"], None)
            p.call("participation.source", early_echo, False)
            ctx.equal("a redelivery after reset keeps the original event", record("race-source"), src)
            s.op("person", "merge", s.a, s.b)
            moved = {**first_origin, "current_person": s.b, "current_generation": 2}
            ctx.equal("current scope changes without replacing the original observation",
                      p.call("participation.source", origin_query)["origin"], moved)
            qq.close()
        # Insert an event with the pre-migration column list in this isolated database.
        # It has original authority but no observation-time generation evidence.
        with closing(sqlite3.connect(s.svc.data / "core.sqlite")) as database, database:
            legacy = database.execute(
                """INSERT INTO events(source_scope,source_key,entry_id,account_id,raw_actor,actor_stage,
                       bot_stage,occurred_at,observed_at,envelope,mentions)
                   SELECT source_scope,'pre-reader-61',entry_id,account_id,raw_actor,actor_stage,
                       bot_stage,occurred_at,observed_at,json_set(envelope,'$.source','pre-reader-61'),mentions
                   FROM events WHERE id=? RETURNING id""", (quoted,)
            ).fetchone()[0]
        with s.svc.running(environment=env):
            # No process-local envelope is recreated after a cold start.
            again = p.call("participation.self_propose", proposal)
            ctx.equal(
                "cold SELF proposal retains original ID", again["execution"], self_exec
            )
            p.call("participation.promote", {"event": 999999}, False)
            cold = p.call("participation.feedback", {"key": proposal["key"]})
            ctx.equal("cold feedback retains physical send", cold, actual)
            ctx.check(
                "cold duplicate does not generate new model attempt", http.wires.empty()
            )
            cold_origin = p.call("participation.source", origin_query)["origin"]
            legacy_origin = p.call("participation.source", {"event": legacy, "include_origin": True})["origin"]
            p.call("participation.promote", {"event": legacy, "expected_generation": 2}, False)
            ctx.equal("cold hydration retains the original identity and generation", cold_origin, moved)
            ctx.equal("older rows without a recorded observation generation remain unknown",
                      legacy_origin, {**moved, "observed_generation": None})
            ctx.equal("an old row with a matching locator cannot prove the reply generation",
                      p.call("participation.source", {"event": legacy, "include_reply": True})["reply_to"], None)
            ctx.equal("cold source read keeps the old same-generation reference",
                      p.call("participation.source", reply_request)["reply_to"], reference)
            isolated_anchor = record("reply-erasure-anchor", person=s.c, native="erased-quote")
            isolated_reply = record("reply-erasure-focus", quoted=("erased-quote",))
            isolated_request = {"event": isolated_reply, "include_reply": True}
            ctx.equal("reply read takes an erasure obligation on the anchor",
                      p.call("participation.source", isolated_request)["reply_to"]["event"], isolated_anchor)
            ctx.equal("quoted original and focus share the plugin dependency",
                      s.rows("SELECT source FROM source_holds WHERE owner=? AND item=? ORDER BY source",
                             "participation", f"participation:{isolated_reply}"),
                      [(isolated_anchor,), (isolated_reply,)])
            s.op("privacy", "erase", "participation-reply-anchor", s.c, json.dumps([isolated_anchor]))
            s.until(lambda: not s.op("privacy", "pending", "participation")["obligations"])
            ctx.equal("erased anchor is unavailable from the unchanged reply envelope",
                      p.call("participation.source", isolated_request)["reply_to"], None)
            s.op("account", "transfer", s.handles[s.c]["assignment"], 200000, 200000, "--person", s.a)
            ctx.equal("cold read after account transfer retains the event-time target",
                      p.call("participation.source", mention_request)["mentions"], frozen_mentions)
            target_person = "00000000-0000-4000-8000-000000003613"
            s.op("person", "create", target_person)
            target = s.op("account", "open", "qq", "schedule", "213", 1,
                          "--person", target_person, "--start", 1)
            target_mention = {"type": "mention", "account": target["account"], "native": "213",
                              "label": "retained original mention"}
            retained = record("retained-mention", mentions=(target_mention,))
            never_read = record("unread-mention", mentions=(target_mention,))
            retained_request = {"event": retained, "mention_cursor": 0}
            ctx.equal("the retained annotation belongs to the original mentioned Person",
                      p.call("participation.source", retained_request)["mentions"]["items"][0]["person_id"],
                      target_person)
            with closing(QQ(s.svc.address, "100", "offline-participation")) as qq:
                p.call("participation.promote", {"event": retained})
                _, q = s.wire(http)
                reply(q, calls=[call("send-with-mentioned-input", "send", {"text": "derived from the mention"})])
                s.deliver(qq, "send_group_msg", 36006)
                _, q = s.wire(http)
                reply(q, "finished")
            review_focus = record("reply-to-mention-derived-self", quoted=("36006",))
            mention_reply_request = {**read_self_reply, "event": review_focus}
            mention_reply = p.call("participation.source", mention_reply_request)["reply_to"]
            mention_review = {**native_review, "origin": {"event": review_focus},
                              "sources": [mention_reply["event"]],
                              "jev": {**native_review["jev"], "state": {
                                  "focus": {"event_id": str(review_focus)}, "context": [
                                      {"event_id": str(mention_reply["event"]), "text": "derived from the mention"}
                                  ],
                              }}}
            mention_call = p.submit("llm.complete", mention_review)
            _, mention_channel = jev.receive()
            mention_receipt = p.call("llm.complete", mention_review)
            review = {
                "call_id": "original-native-mention-review",
                "origin": {"event": retained},
                "route": "review",
                "purpose": "review",
                "text": "review retained original mention",
                "sources": [retained],
            }
            request = p.submit("llm.complete", review)
            body, pending = http.receive()
            ctx.check("a review dispatched once from the original mention source",
                      "review retained original mention" in body)
            logical, = s.rows(
                "SELECT r.id FROM model_requests r JOIN host_aux_requests q ON q.logical=r.id "
                "JOIN host_aux_origins o ON q.plugin=o.plugin AND q.origin=o.original "
                "WHERE q.plugin='participation' AND o.event=?", retained,
            )[0]
            erased = s.op("privacy", "forget", "participation-mentioned-only", target_person)
            obligations = s.rows("SELECT sources FROM owner_erasures WHERE watermark=? AND owner='participation'",
                                 erased["watermark"])
            ctx.equal("a mention-only erasure notifies the owner of the retained source and derived SELF",
                      [json.loads(row[0]) for row in obligations], [[retained, mention_reply["event"]]])
            ctx.equal("mention erasure fences the original derived review execution",
                      s.rows("SELECT f.watermark FROM participation_mention_redactions f "
                             "JOIN host_aux_origins o ON o.execution=f.execution "
                             "JOIN host_aux_requests q ON q.plugin=o.plugin AND q.origin=o.original WHERE q.id=?",
                             mention_receipt["request"]), [(erased["watermark"],)])
            ctx.equal("forget preserves unrelated authors' original event rows",
                      s.rows("SELECT id FROM events WHERE id IN (?,?) AND envelope!='{}' ORDER BY id",
                             retained, never_read), [(retained,), (never_read,)])
            pending.put(({
                "choices": [{"message": {"role": "assistant", "content": "late-mention-private-canary"},
                             "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7},
            }, 0))
            p.receive(request, None)
            attempts = s.until(lambda: s.rows(
                "SELECT r.request_json,t.response,t.usage_json,t.state FROM model_attempts t "
                "JOIN model_requests r ON r.id=t.logical WHERE r.id=? AND t.state!='dispatched'", logical,
            ))
            ctx.equal("late mention review cannot restore its request or response",
                      (len(attempts), attempts[0][0], attempts[0][1], attempts[0][3]),
                      (1, "{}", None, "response_held"))
            ctx.equal("the original physical usage remains accounted",
                      json.loads(attempts[0][2])["total"], 7)
            mention_channel.put((jev_response, 0))
            p.receive(mention_call, None)
            s.until(lambda: not s.op("privacy", "pending", "participation")["obligations"])
            mention_late = p.call("llm.result", {"request": mention_receipt["request"]})
            ctx.equal("forgetting a Person mentioned in the producing turn redacts the SELF-based Jev result",
                      (mention_late["size"], mention_late["receipt"]["state"], mention_late["receipt"]["erased"]),
                      (0, "response_held", True))
            ctx.equal("the derived review retains the original physical usage", mention_late["receipt"]["usage"],
                      {"input": 31, "output": 5})
            p.call("participation.source", retained_request, False)
            p.call("participation.source", {"event": never_read, "mention_cursor": 0}, False)
            p.call("llm.complete", {**review, "call_id": "after-mentioned-person-forget"}, False)
            ctx.equal("the derived reply is no longer available to the original focus",
                      p.call("participation.source", mention_reply_request)["reply_to"], None)
            ctx.check("mention erasure blocks another physical request", http.wires.empty())
            ctx.equal("unrelated sources remain readable after mention erasure",
                      p.call("participation.source", {"event": literal, "mention_cursor": 0})["mentions"]["items"], [])
            proofs["native_mentions"] = {"event": mentioned, "snapshot": frozen_mentions,
                                         "bounded_page_sizes": [16, 3], "literal_at_count": 0,
                                         "erasure_sources": [retained, mention_reply["event"]],
                                         "self_context_receipt": mention_late["receipt"],
                                         "late_review": {"state": attempts[0][3], "physical_attempts": 1,
                                                         "usage_total": 7, "paid_calls": 0}}
            proofs["reply_reference"] = {
                "focus": quoted, "reference": reference,
                "absent_for": [*unsafe_events, "disabled-author", "across-reset", "erased-anchor"],
            }
            proofs["observation_origin"] = {"event": src, "original": first_origin, "cold": cold_origin,
                                            "fresh": fresh_origin, "legacy": legacy_origin}
        import re

        config.write_text(
            re.sub(
                r"\n\[qq\]\n.*?(?=\n\[\[plugins\]\])",
                "\n",
                config.read_text(),
                flags=re.S,
            ),
            encoding="utf-8",
        )
        before = s.rows("SELECT count(*) FROM model_requests")
        with s.svc.running(environment=env):
            p.call(
                "participation.self_propose",
                {**proposal, "key": "globally-disabled"},
                False,
            )
            p.call("participation.sources", {"entry": entry}, False)
            p.call("participation.promote", {"event": src}, False)
            ctx.equal(
                "disabled QQ cannot admit new activation",
                s.rows("SELECT count(*) FROM model_requests"),
                before,
            )
        proofs["global_disabled"] = {"model_requests": before, "new_activations": 0}
    ready = json.loads((folder / "ready.json").read_bytes())
    proofs["negotiated_participation_methods"] = sorted(
        m for m in ready["methods"] if m.startswith("participation.")
    )
    ctx.equal(
        "actual SDK negotiates all participation methods",
        proofs["negotiated_participation_methods"],
        sorted(methods),
    )
    proofs["binary_sha256"] = hashlib.sha256(
        (
            ctx.root
            / "target/debug"
            / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
        ).read_bytes()
    ).hexdigest()
    proofs["host_idl_sha256"] = hashlib.sha256(
        (ctx.root / "sdk/rust/src/host-preview.json").read_bytes()
    ).hexdigest()
    proofs["participation_schema_sha256"] = hashlib.sha256(
        (ctx.root / "protocol/plugin/participation/methods.json").read_bytes()
    ).hexdigest()
    proofs["external"] = external(ctx, sdk, aux)
    proofs["real_extension_consumer"] = "pending EXT-025"
    return proofs


def external(ctx, sdk, aux):
    from tests.harness.compact import Owner

    o = Owner(ctx, "participation-external", 31)
    with o.svc.running():
        bot = o.op("account", "open", "telegram", "scene-test", "100", 1, "--start", 1)
        entry = o.op("entry", "group", json.dumps(["telegram", "scene-test", "room"]))[
            "entry"
        ]
    folder = ctx.working / "external-participant"
    folder.mkdir()
    gateway = ctx.working / "external-gateway"
    gateway.mkdir()
    methods = json.loads(
        (ctx.root / "protocol/plugin/participation/methods.json").read_bytes()
    )["methods"]
    with Recorder() as http:
        # Actual installed gateway peer records every dispatched physical part.
        with (o.svc.data / "yuki.toml").open("a", encoding="utf-8") as f:
            f.write(
                "\n[main]\nendpoint="
                + json.dumps(http.url)
                + '\nmodel="fixture"\nname="Yuki"\nmerge_ms=2\nroot_slots=16\nmax_input_units=4000000\nmax_output=128\n'
            )
            f.write(
                '\n[[plugins]]\nid="external_participant"\nnamespace="external_participant"\nprogram='
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
                + "\n[plugins.participation]\nentries="
                + json.dumps([entry])
                + "\n[plugins.participation.self_scenes]\n"
                + json.dumps(entry)
                + "="
                + json.dumps(bot["assignment"])
                + "\n"
            )
            f.write(
                '\n[[plugins]]\nid="gateway"\nnamespace="scene-test"\nplatform="telegram"\nbot_stage='
                + json.dumps(bot["assignment"])
                + "\nprogram="
                + json.dumps(sys.executable)
                + "\nargs="
                + json.dumps(
                    [
                        str(ctx.root / "tests/peers/host/peer.py"),
                        str(gateway),
                        str(sdk / "python-install"),
                    ]
                )
                + '\ngrants=["gateway.send"]\n'
            )
        p = aux.Peer(o, folder)
        with o.svc.running():
            o.until((folder / "ready.json").exists)
            o.until(
                lambda: o.rows(
                    "SELECT plugin FROM host_installations WHERE plugin='gateway' AND online"
                )
            )
            proposal = {
                "entry": entry,
                "key": "native-two-parts",
                "text": "send from original SELF gateway",
            }
            accepted = p.call("participation.self_propose", proposal)
            _, q = o.wire(http)
            reply(q, calls=[call("native-send", "send", {"text": "part-one|part-two"})])
            _, q = o.wire(http)
            reply(q, "done")
            o.until(
                lambda: o.rows(
                    "SELECT id FROM executions WHERE id=? AND state='completed'",
                    accepted["execution"],
                )
            )
            feedback = p.call("participation.feedback", {"key": proposal["key"]})
            ctx.equal(
                "external feedback has two confirmed physical parts",
                feedback["executions"][0]["sends"][0]["confirmed_parts"],
                2,
            )
            deliveries = [
                json.loads(l)
                for l in (gateway / "deliveries.jsonl").read_text().splitlines()
            ]
            ctx.equal(
                "external receiver is original scene for both parts",
                [v["arguments"]["route"]["chat"] for v in deliveries],
                ["room", "room"],
            )
            ctx.equal(
                "external SELF retains independent original root",
                o.rows(
                    "SELECT source,original_actor,root FROM executions x JOIN budget_members b ON b.execution=x.id WHERE x.id=?",
                    accepted["execution"],
                ),
                [(None, None, accepted["execution"])],
            )
            (gateway / "cut-second").write_text("fixture crash after second dispatch")
            cut = {
                "entry": entry,
                "key": "unknown-part",
                "text": "original partially confirmed send",
            }
            unknown = p.call("participation.self_propose", cut)
            _, q = o.wire(http)
            reply(
                q,
                calls=[
                    call(
                        "unknown-send",
                        "send",
                        {"text": "confirmed-first|unknown-second"},
                    )
                ],
            )
            o.until(
                lambda: o.rows(
                    "SELECT id FROM executions WHERE id=? AND state IN('held','completed')",
                    unknown["execution"],
                )
            )
            ctx.check(
                "offline original gateway prevents another activation",
                http.wires.empty(),
            )
            # Historical feedback is available while the gateway itself is down.
            partial = p.call("participation.feedback", {"key": cut["key"]})
            ctx.equal(
                "partial send keeps one confirmation and unknown state",
                [
                    (v["state"], v["confirmed_parts"])
                    for v in partial["executions"][0]["sends"]
                ],
                [("unknown", 1)],
            )
            ctx.check(
                "unknown send is never quiet", not partial["executions"][0]["quiet"]
            )
            p.call("participation.self_propose", {**cut, "key": "offline-new"}, False)
            before = (gateway / "deliveries.jsonl").read_text().splitlines()
        with o.svc.running():
            o.until(
                lambda: o.rows(
                    "SELECT plugin FROM host_installations WHERE plugin='gateway' AND online"
                )
            )
            recovered = p.call("participation.self_propose", cut)
            ctx.equal(
                "cold unknown SELF proposal keeps execution",
                recovered["execution"],
                unknown["execution"],
            )
            ctx.equal(
                "cold unknown send does not replay parts",
                (gateway / "deliveries.jsonl").read_text().splitlines(),
                before,
            )
            ctx.equal(
                "cold original feedback keeps partial truth",
                p.call("participation.feedback", {"key": cut["key"]}),
                partial,
            )
            ctx.check("cold unknown creates no model request", http.wires.empty())
        return {"normal": feedback, "partial": partial, "physical_parts": len(before)}
