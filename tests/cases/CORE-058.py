"""Publish under real installed namespaces and reconcile the actual Social effects."""
import hashlib
import importlib.util
import json
import os
import subprocess
import sys

from tests.harness.compact import Owner, reply, call
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer as QQ


def installed(ctx, owner, sdk, folder, plugin, targets, agent=True, media=False, tool=None):
    folder.mkdir()
    with (owner.svc.data / "yuki.toml").open("a", encoding="utf-8") as stream:
        stream.write(
            "\n[[plugins]]\nid=" + json.dumps(plugin)
            + "\nnamespace=" + json.dumps(plugin)
            + "\nprogram=" + json.dumps(sys.executable)
            + "\nargs=" + json.dumps([
                str(ctx.root / "tests/peers/aux-model/peer.py"), str(folder),
                str(sdk / "python-install"),
            ])
            + '\ngrants=' + json.dumps(["notification.publish","forget.owner","observe.metadata"]
                                         + (["tool.call"] if media or tool else []) + (["media.publish"] if media else [])) + '\n'
            + "[plugins.notifications]\nagent=" + str(agent).lower()
            + "\n[plugins.notifications.targets]\n"
        )
        for entry, receiver in targets.items():
            stream.write(json.dumps(entry) + "=" + json.dumps(receiver) + "\n")
        if media or tool:
            stream.write('[[plugins.tools]]\nname=' + json.dumps(tool or "make_notice_images") + '\ndescription="Create scoped notification images"\ncacheable=false\nparallel_read=false\ncode_direct=false\nschema={type="object",properties={},additionalProperties=false}\n')


def request(entry, space, key, **extra):
    return {
        "event_key": key, "event_type": "subscription_update", "external_source": "feed-fixture",
        "target": {"target_type": "group", "target_id": space, "conversation_id": entry},
        "occurred_at": "2026-11-05T17:00:00+08:00", "summary": "Original update",
        "payload": {"item": "original", "revision": 1}, "text": "Original notification text",
        **extra,
    }


def receive(ctx, peer, action, locator):
    while True:
        frame = peer.receive()
        if frame["action"] == action:
            break
        peer.send({"echo": frame.get("echo"), "status": "ok", "retcode": 0, "data": {}})
    ctx.equal("actual notification receiver action", frame["action"], action)
    peer.send({"echo": frame["echo"], "status": "ok", "retcode": 0,
               "data": {"message_id": locator}})
    return frame


def run(ctx):
    fixture_file_receipts(ctx)
    spec = importlib.util.spec_from_file_location("notification_aux", ctx.root / "tests/cases/CORE-050.py")
    aux = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(aux)
    sdk = ctx.working / "sdk"
    built = subprocess.run([
        sys.executable, str(ctx.root / "ci/publish-contract.py"), "build", "--out",
        str(ctx.output / "sdk-distributions"), "--verify-work", str(sdk),
    ], capture_output=True, text=True)
    if built.returncode:
        ctx.write("sdk-failure.json", {"stderr": built.stderr})
    ctx.equal("both independent SDK distributions verify", built.returncode, 0)
    owner = Owner(ctx, "notifications", 34)
    space = json.dumps(["qq", "compact", "700"], separators=(",", ":"))
    with owner.svc.running():
        entry = owner.op("entry", "group", space, "--topic", "first")["entry"]
        alternate = owner.op("entry", "group", space, "--topic", "second")["entry"]
        private = owner.op("entry", "private", owner.user["assignment"], owner.bot["assignment"])["entry"]
    folders = {name: ctx.working / name for name in ["news-a", "news-b"]}
    environment = {**os.environ, "YUKI_NOTIFICATION_QQ": "offline-notification"}
    with Recorder() as http:
        with (owner.svc.data / "yuki.toml").open("a", encoding="utf-8") as stream:
            stream.write(
                '\n[main]\nendpoint=' + json.dumps(http.url)
                + '\nmodel="fixture"\nname="Yuki"\nmerge_ms=0\nroot_slots=16\nmax_input_units=4000000\nmax_output=1024\n'
                + '[qq]\nnamespace="compact"\ndesignated_ingress="100"\n[qq.groups]\n"700"="open"\n'
                + '[[qq.bots]]\nself_id="100"\nstage=' + json.dumps(owner.bot["assignment"])
                + '\ntoken_env="YUKI_NOTIFICATION_QQ"\n'
            )
        for name, folder in folders.items():
            installed(ctx, owner, sdk, folder, name,
                      {key: owner.bot["assignment"] for key in [entry, alternate, private]})
        a, b = [aux.Peer(owner, folder) for folder in folders.values()]
        with owner.svc.running(environment=environment):
            for folder in folders.values():
                owner.until((folder / "ready.json").exists)
            qq = QQ(owner.svc.address, "100", "offline-notification")
            try:
                original = request(entry, space, "same-external-key")
                first = a.call("notification.publish", original)
                actual = [receive(ctx, qq, "send_group_msg", 5801)]
                owner.until(lambda: owner.rows("SELECT publication FROM notification_deliveries WHERE state='confirmed'"))
                replay = a.call("notification.publish", {**original, "occurred_at": "2026-11-05T09:00:00Z"})
                ctx.equal("UTC-normalized replay retains original publication and event",
                          [replay["notification_id"], replay["source_event_id"], replay["deduplicated"]],
                          [first["notification_id"], first["source_event_id"], True])
                ctx.equal("same publisher replay has one actual send", owner.rows("SELECT count(*) FROM social_outbound"), [(1,)])
                for changed in [
                    {"text": "changed text"}, {"event_type": "other-type"}, {"agent_intent": "different intent"},
                    {"target": {**original["target"], "conversation_id": alternate}},
                    {"ask_agent": True}, {"resume_waiting_work": True},
                ]:
                    a.call("notification.publish", {**original, **changed}, False)
                second = b.call("notification.publish", original)
                actual.append(receive(ctx, qq, "send_group_msg", 5802))
                owner.until(lambda: owner.rows("SELECT publication FROM notification_deliveries WHERE publication=? AND state='confirmed'", second["notification_id"]))
                ctx.check("separate authenticated namespaces retain independent publications",
                          first["notification_id"] != second["notification_id"])
                b.call("notification.read", {"notification_id": first["notification_id"]}, False)
                ctx.equal("direct publications create no Main model requests", owner.rows("SELECT count(*) FROM model_requests"), [(0,)])
                ctx.equal("publisher events have no fabricated human actor",
                          owner.rows("SELECT e.raw_actor,e.actor_stage,a.platform,a.namespace FROM notification_publications p JOIN events e ON e.id=p.event JOIN accounts a ON a.id=e.account_id ORDER BY p.event"),
                          [(None, None, "plugin", "news-a"), (None, None, "plugin", "news-b")])
                quiet = a.call("notification.publish", request(entry, space, "ask-quiet", text="", ask_agent=True, agent_intent="Evaluate quietly"))
                raw, q = http.receive()
                ctx.check("actual Main request includes original publisher", "news-a" in raw)
                reply(q, "quiet evaluation")
                owner.until(lambda: owner.rows("SELECT id FROM executions WHERE id=? AND state='completed'", quiet["agent_execution"]))
                ctx.equal("Main quiet keeps publication separate from public delivery",
                          [quiet["delivery_enqueued"], owner.rows("SELECT count(*) FROM social_outbound")], [False, [(2,)]])
                ctx.equal("Main evaluation keeps its plugin origin",
                          owner.rows("SELECT x.original_actor,o.kind FROM executions x JOIN main_origins o ON o.ref=x.origin WHERE x.id=?", quiet["agent_execution"]), [(None, "plugin")])
                hidden = a.call("notification.publish", request(private, space, "private-original",
                    target={"target_type":"private","target_id":owner.actor,"conversation_id":private}, text="Private notification canary"))
                actual.append(receive(ctx, qq, "send_private_msg", 5803))
                owner.until(lambda: owner.rows("SELECT publication FROM notification_deliveries WHERE publication=? AND state='confirmed'", hidden["notification_id"]))
                ctx.equal("settled and empty delivery executions are closed",
                          owner.rows("SELECT DISTINCT x.state FROM notification_publications p JOIN executions x ON x.id=p.delivery_execution"), [("completed",)])
                owner.op("host", "enable", "news-a", "false")
                owner.until(lambda: owner.rows("SELECT plugin FROM host_installations WHERE plugin='news-a' AND NOT enabled AND NOT online"))
                ctx.equal("revocation closes the authenticated reader connection",
                          owner.rows("SELECT enabled,online FROM host_installations WHERE plugin='news-a'"), [(0,0)])
                owner.op("host", "enable", "news-a", "true")
                ctx.equal("re-enabled exact original key does not create another event",
                          a.call("notification.publish", original)["source_event_id"], first["source_event_id"])
                erased = owner.op("privacy", "forget", "notification-private-erase", owner.actor)
                owner.until(lambda: not owner.op("privacy", "pending", "news-a")["obligations"])
                a.call("notification.read", {"notification_id":hidden["notification_id"]}, False)
                ctx.equal("private notification source content is erased",
                          owner.rows("SELECT envelope FROM events WHERE id=?", hidden["source_event_id"]), [("{}",)])
                ctx.equal("erasure retains the known physical delivery outcome",
                          owner.rows("SELECT state FROM notification_deliveries WHERE publication=?", hidden["notification_id"]), [("confirmed",)])
                ctx.equal("unrelated public publisher receipt survives private erasure",
                          b.call("notification.read", {"notification_id":second["notification_id"]})["state"], "published")
                proof = {"publications":[first,second,quiet,hidden],"physical_receipts":actual,"erasure":erased}
            finally:
                qq.close()
    methods = (ctx.root / "protocol/plugin/notifications/methods.json").read_bytes()
    negotiated = json.loads((folders["news-a"] / "ready.json").read_bytes())["methods"]
    proof.update({
        "notification_schema_sha256":hashlib.sha256(methods).hexdigest(),
        "negotiated_notification_methods":sorted(m for m in negotiated if m.startswith("notification.")),
        "host_idl_sha256":hashlib.sha256((ctx.root / "sdk/rust/src/host-preview.json").read_bytes()).hexdigest(),
        "binary_sha256":hashlib.sha256(owner.svc.binary.read_bytes()).hexdigest(),
        "real_consumer":"pending EXT-027 and EXT-047",
    })
    proof["administrator_grants"] = administrator_grants(ctx, sdk, aux)
    ctx.write("notification-administrator-grants.json", proof["administrator_grants"])
    proof["commit_recovery"] = commit_recovery(ctx, sdk, aux)
    ctx.write("notification-commit-recovery.json", proof["commit_recovery"])
    proof["unknown_delivery"] = unknown_delivery(ctx, sdk, aux)
    ctx.write("notification-unknown-delivery.json", proof["unknown_delivery"])
    proof["original_wait"] = original_wait(ctx, sdk, aux)
    proof["plugin_wait"] = original_wait(ctx, sdk, aux, plugin_root=True)
    proof["media_order_and_expiry"] = media_order_and_expiry(ctx, sdk, aux)
    proof["external_gateway"] = external_gateway(ctx, sdk, aux)
    return proof


def fixture_file_receipts(ctx):
    import time
    from concurrent.futures import ThreadPoolExecutor
    from tests.harness.json_file import read_json

    path = ctx.working / "delayed-fixture-receipt.json"
    path.write_bytes(b'{"original":')
    with ThreadPoolExecutor(max_workers=1) as pool:
        reader = pool.submit(read_json, path, 2)
        time.sleep(0.05)
        ctx.check("partial fixture response waits for the same file", not reader.done())
        path.write_text('{"original":true}')
        ctx.equal("completed fixture response is read without another RPC", reader.result(), {"original":True})
        if os.name == "nt":
            import ctypes
            from ctypes import wintypes
            kernel = ctypes.WinDLL("kernel32", use_last_error=True)
            kernel.CreateFileW.argtypes = [wintypes.LPCWSTR,wintypes.DWORD,wintypes.DWORD,
                wintypes.LPVOID,wintypes.DWORD,wintypes.DWORD,wintypes.HANDLE]
            kernel.CreateFileW.restype = wintypes.HANDLE
            kernel.CloseHandle.argtypes = [wintypes.HANDLE]
            locked = kernel.CreateFileW(str(path),0x80000000,0,None,3,0x80,None)
            ctx.check("actual Windows fixture receipt is exclusively held", locked != wintypes.HANDLE(-1).value)
            try:
                reader = pool.submit(read_json,path,2)
                time.sleep(0.05)
                ctx.check("Windows sharing conflict retains the original file read", not reader.done())
            finally:
                kernel.CloseHandle(locked)
            ctx.equal("released Windows receipt is consumed without another call",reader.result(),{"original":True})
    malformed = ctx.working / "malformed-fixture-receipt.json"
    malformed.write_bytes(b'{')
    try:
        read_json(malformed,0.03)
    except json.JSONDecodeError:
        ctx.check("permanently malformed fixture receipt fails its bound",True)
    else:
        ctx.check("permanently malformed fixture receipt fails its bound",False)


def private_owner(ctx, sdk, name, model=None, media=False, tool=None, granted=True):
    owner = Owner(ctx, name, 34)
    with owner.svc.running():
        entry = owner.op("entry", "private", owner.user["assignment"], owner.bot["assignment"])["entry"]
        alternate = owner.op("entry", "private", owner.user["assignment"], owner.bot["assignment"], "--topic", "other")["entry"]
    with (owner.svc.data / "yuki.toml").open("a", encoding="utf-8") as stream:
        if media:
            stream.write('\n[media]\ngateway_transfer_directory=' + json.dumps(str(owner.svc.data / "notification-transfer")) + '\n')
        if model:
            stream.write('\n[main]\nendpoint=' + json.dumps(model.url)
                         + '\nmodel="fixture"\nname="Yuki"\nmerge_ms=0\nroot_slots=24\nmax_input_units=4000000\nmax_output=1024\n')
        stream.write('[qq]\nnamespace="compact"\ndesignated_ingress="100"\n'
                     + '[[qq.bots]]\nself_id="100"\nstage=' + json.dumps(owner.bot["assignment"])
                     + '\ntoken_env="YUKI_NOTIFICATION_QQ"\n')
    folder = ctx.working / (name + "-peer")
    installed(ctx, owner, sdk, folder, name,
              {key:owner.bot["assignment"] for key in [entry, alternate]} if granted else {}, media=media, tool=tool)
    target = {"target_type":"private","target_id":owner.actor,"conversation_id":entry}
    params = request(entry, "", "original", target=target)
    return owner, folder, entry, alternate, params


def administrator_grants(ctx, sdk, aux):
    with Recorder() as http:
        owner, folder, entry, alternate, params = private_owner(
            ctx, sdk, "grant-news", http, tool="manage_notice_targets", granted=False)
        peer = aux.Peer(owner, folder)
        environment = {**os.environ, "YUKI_NOTIFICATION_QQ":"offline-grants"}
        with owner.svc.running(environment=environment):
            owner.until((folder / "ready.json").exists)
            ctx.equal("empty installed targets permit status without granting a recipient",
                      peer.call("notification.status", {}), {})
            peer.call("notification.publish", params, False)
            human = owner.human("Authorize this subscription target")
            _, q = owner.wire(http)
            reply(q, calls=[call("grant", "manage_notice_targets", {})])
            owner.until(lambda:list(folder.glob("call-*.json")))
            handle = json.loads(next(folder.glob("call-*.json")).read_bytes())["params"]["handle"]
            target = {"target_type":"private", "target_id":owner.actor}
            grant = {"handle":handle, "key":"original-grant", "target":target, "bot_user_id":""}
            peer.call("notification.grant_target", grant, False)
            ctx.equal("an ordinary human cannot create background delivery grants",
                      owner.rows("SELECT count(*) FROM notification_target_grants"), [(0,)])
            owner.op("person", "set-admin", owner.actor, "true")
            peer.call("notification.grant_target", {**grant,"bot_user_id":"forged-bot"}, False)
            peer.call("notification.grant_target", {**grant,"actor":owner.actor}, False)
            peer.call("notification.grant_target", {**grant,"handle":"forged-call"}, False)
            first = peer.call("notification.grant_target", grant)
            ctx.equal("real subscription grant defaults to the original invocation conversation",
                      first, {"target_type":"private","target_id":owner.actor,"conversation_id":entry,
                              "bot_user_id":"100","enabled":True,"created_by_user_id":owner.actor})
            ctx.equal("same mutation key returns its original grant", peer.call("notification.grant_target", grant), first)
            ctx.equal("grant replay leaves one original revision",
                      owner.rows("SELECT revision,source_call FROM notification_target_grants"), [(1,handle)])
            second_target = {**target, "conversation_id":alternate}
            peer.call("notification.grant_target", {**grant,"target":second_target}, False)
            second = peer.call("notification.grant_target", {**grant,"key":"other-topic","target":second_target})
            pending = peer.call("notification.publish", params)
            revoke = {"handle":handle,"key":"revoke-original","target":target}
            ctx.equal("real administrator revokes the original target", peer.call("notification.revoke_target", revoke), True)
            peer.call("notification.read", {"notification_id":pending["notification_id"]}, False)
            peer.call("notification.publish", params, False)
            ctx.equal("old grant acknowledgement remains historical", peer.call("notification.grant_target", grant), first)
            ctx.equal("replaying a grant does not undo a later revoke",
                      owner.rows("SELECT enabled,revision FROM notification_target_grants WHERE entry=?",entry), [(0,2)])
            other = peer.call("notification.publish", {**params,"event_key":"other-topic","target":second_target})
            qq = QQ(owner.svc.address, "100", "offline-grants")
            try:
                actual = receive(ctx, qq, "send_private_msg", 5830)
                owner.until(lambda:owner.rows("SELECT state FROM notification_deliveries WHERE publication=?",other["notification_id"])==[("confirmed",)])
                ctx.equal("revoked pending delivery stays undispatched while another target succeeds",
                          owner.rows("SELECT count(*) FROM social_outbound"), [(1,)])
                ctx.equal("status exposes actual scoped durable delivery states",
                          peer.call("notification.status", {}), {"outbox_pending":1,"outbox_confirmed":1})
            finally:
                qq.close()
            (folder / ("finish-" + handle + ".json")).write_text(json.dumps({"ok":True,"data":{"grants":[first,second]}}))
            _, q = owner.wire(http)
            reply(q, "subscription targets stored")
            owner.finished(human)
            ctx.equal("finished invocation can reconcile its known original mutation",
                      peer.call("notification.grant_target", grant), first)
            peer.call("notification.grant_target", {**grant,"key":"new-after-finish"}, False)
            owner.op("person", "set-admin", owner.actor, "false")
            peer.call("notification.grant_target", grant, False)
            owner.op("person", "set-admin", owner.actor, "true")
        config = owner.svc.data / "yuki.toml"
        config.write_text(config.read_text().replace("agent=true", "agent=false"))
        with owner.svc.running(environment=environment):
            ctx.equal("cold restart and unrelated policy revision retain the accepted other target",
                      peer.call("notification.read", {"notification_id":other["notification_id"]})["source_event_id"], other["source_event_id"])
            ctx.equal("known grant receipt survives process generation and policy revision",
                      peer.call("notification.grant_target", grant), first)
            peer.call("notification.publish", params, False)
            ctx.equal("revocation remains durable without resending either original",
                      owner.rows("SELECT count(*) FROM social_outbound"), [(1,)])
        return {"first":first,"second":second,"pending_revoked":pending,"other":other,
                "actual":actual,"original_call":handle,"human_source":human}


def commit_recovery(ctx, sdk, aux):
    from concurrent.futures import ThreadPoolExecutor
    from tests.harness.crash import kill_writer
    owner, folder, _, _, params = private_owner(ctx, sdk, "cut-news")
    peer = aux.Peer(owner, folder)
    environment = {**os.environ, "YUKI_NOTIFICATION_QQ":"offline-cut"}
    with ThreadPoolExecutor(max_workers=1) as pool:
        killed = pool.submit(kill_writer, ctx, owner.svc.data, "work_delivery_crash_child",
                             "notification-published-before-reply", {"YUKI_NOTIFICATION_QQ":"offline-cut"})
        owner.until((folder / "ready.json").exists)
        peer.submit("notification.publish", params)
        killed.result()
    before = owner.rows("SELECT id,event,delivery_execution FROM notification_publications")
    ctx.equal("publication and delivery intent survive missing plugin acknowledgement", len(before), 1)
    with owner.svc.running(environment=environment):
        qq = QQ(owner.svc.address, "100", "offline-cut")
        try:
            result = peer.call("notification.publish", params)
            actual = receive(ctx, qq, "send_private_msg", 5810)
            owner.until(lambda: owner.rows("SELECT publication FROM notification_deliveries WHERE state='confirmed'"))
            replay = peer.call("notification.publish", params)
            ctx.equal("recovery keeps original publication, source and execution",
                      owner.rows("SELECT id,event,delivery_execution FROM notification_publications"), before)
            ctx.equal("replayed acknowledgement has one original external effect",
                      owner.rows("SELECT count(*) FROM social_outbound"), [(1,)])
            ctx.equal("publication recovery buys no model", owner.rows("SELECT count(*) FROM model_requests"), [(0,)])
            ctx.equal("replayed plugin acknowledges the original durable publication",
                      [result["notification_id"],replay["notification_id"]], [before[0][0],before[0][0]])
            return {"original":before,"receipt":replay,"physical":actual}
        finally:
            qq.close()


def unknown_delivery(ctx, sdk, aux):
    owner, folder, _, _, params = private_owner(ctx, sdk, "unknown-news")
    peer = aux.Peer(owner, folder)
    environment = {**os.environ, "YUKI_NOTIFICATION_QQ":"offline-unknown"}
    with owner.svc.running(environment=environment):
        owner.until((folder / "ready.json").exists)
        qq = QQ(owner.svc.address, "100", "offline-unknown")
        original = peer.call("notification.publish", params)
        while True:
            wire = qq.receive()
            if wire["action"] == "send_private_msg":
                break
            qq.send({"echo":wire.get("echo"),"status":"ok","retcode":0,"data":{}})
        qq.close()
        owner.until(lambda: owner.rows("SELECT publication FROM notification_deliveries WHERE state='unknown'"))
        operations = owner.rows("SELECT operation_id,state FROM operations ORDER BY operation_id")
        result = peer.call("notification.publish", params)
        ctx.equal("lost physical acknowledgement remains unknown", result["deliveries"][0]["state"], "unknown")
    with owner.svc.running(environment=environment):
        qq = QQ(owner.svc.address, "100", "offline-unknown")
        try:
            restored = peer.call("notification.publish", params)
            ctx.equal("restart retains unknown original receipt", restored["notification_id"], original["notification_id"])
            ctx.equal("unknown replay never creates replacement effects",
                      owner.rows("SELECT operation_id,state FROM operations ORDER BY operation_id"), operations)
            fresh = peer.call("notification.publish", {**params,"event_key":"new-business-event","text":"New notification only"})
            sent = receive(ctx, qq, "send_private_msg", 5811)
            ctx.check("reconnected receiver sees only the new business event", "New notification only" in json.dumps(sent))
            owner.until(lambda: owner.rows("SELECT publication FROM notification_deliveries WHERE publication=? AND state='confirmed'", fresh["notification_id"]))
            ctx.equal("original unknown delivery state is immutable",
                      peer.call("notification.read", {"notification_id":original["notification_id"]})["deliveries"][0]["state"], "unknown")
            return {"original":original,"unknown_wire":wire,"operations":operations,"new_wire":sent}
        finally:
            qq.close()


def original_wait(ctx, sdk, aux, plugin_root=False):
    with Recorder() as http:
        plugin = "plugin-wait-news" if plugin_root else "wait-news"
        owner, folder, entry, alternate, params = private_owner(ctx, sdk, plugin, http,
            granted=plugin_root, tool=None if plugin_root else "grant_wait_target")
        peer = aux.Peer(owner, folder)
        environment = {**os.environ,"YUKI_NOTIFICATION_QQ":"offline-wait"}
        with owner.svc.running(environment=environment):
            owner.until((folder / "ready.json").exists)
            preparation_requests = 0
            preparation_roots = 0
            if not plugin_root:
                owner.op("person","set-admin",owner.actor,"true")
                authorization = owner.human("Authorize my subscription continuation")
                _, q = owner.wire(http)
                reply(q,calls=[call("authorize","grant_wait_target",{})])
                owner.until(lambda:list(folder.glob("call-*.json")))
                handle = json.loads(next(folder.glob("call-*.json")).read_bytes())["params"]["handle"]
                for selected in [entry,alternate]:
                    peer.call("notification.grant_target",{"handle":handle,"key":selected,
                        "target":{"target_type":"private","target_id":owner.actor,"conversation_id":selected}})
                (folder / ("finish-"+handle+".json")).write_text(json.dumps({"ok":True,"data":{}}))
                _, q = owner.wire(http)
                reply(q,"subscription granted")
                owner.finished(authorization)
                preparation_requests = 2
                preparation_roots = 1
            if plugin_root:
                initial = peer.call("notification.publish", {**params,"text":"","ask_agent":True})
                event = initial["source_event_id"]
                principal = owner.rows("SELECT origin FROM executions WHERE id=?", initial["agent_execution"])[0][0]
                ctx.equal("plugin Work starts without a fabricated human event",
                          owner.rows("SELECT raw_actor FROM events WHERE id=?", event), [(None,)])
            else:
                event = owner.human("Wait for my original subscription update")
                principal = owner.actor
            _, q = owner.wire(http)
            reply(q, calls=[call("accept","task_control",{"action":"accept","goal":"original subscription Work"})])
            _, q = owner.wire(http)
            work = owner.rows("SELECT id FROM work_nodes")[0][0]
            reply(q, calls=[call("wait","task_control",{"action":"wait","conditions":[{
                "kind":"plugin_event","plugin_id":plugin,"event_type":"subscription_update","external_source":"feed-fixture"}]})])
            owner.until(lambda: owner.rows("SELECT id FROM work_waits WHERE work=? AND state='armed'", work))
            wrong = peer.call("notification.publish", {**params,"event_key":"other-topic","text":"",
                "resume_waiting_work":True,"target":{**params["target"],"conversation_id":alternate}})
            unasked = peer.call("notification.publish", {**params,"event_key":"no-resume","text":""})
            other_type = peer.call("notification.publish", {**params,"event_key":"other-type","text":"",
                "event_type":"other_update","resume_waiting_work":True})
            other_source = peer.call("notification.publish", {**params,"event_key":"other-source","text":"",
                "external_source":"unrelated-feed","resume_waiting_work":True})
            ctx.equal("unmatched notifications leave original Work waiting",
                      owner.rows("SELECT state FROM work_waits WHERE work=?", work), [("armed",)])
            ctx.equal("unmatched notifications buy no continuation", owner.rows("SELECT count(*) FROM model_requests"), [(preparation_requests+2,)])
            right = peer.call("notification.publish", {**params,"event_key":"right-original","text":"",
                "resume_waiting_work":True,"ask_agent":True,"summary":"Original publisher update arrived"})
            raw, q = http.receive()
            ctx.check("original Work continuation sees the actual plugin source",
                      "Original publisher update arrived" in raw and plugin in raw)
            ctx.equal("publication resumes one original Work", owner.rows("SELECT id,principal FROM work_nodes"), [(work,principal)])
            ctx.equal("notification continuation keeps one original paid root", owner.rows("SELECT count(*) FROM root_budgets"), [(preparation_roots+1,)])
            ctx.equal("matched Work consumes the request for Main evaluation", right["agent_execution"], None)
            reply(q, calls=[call("done","task_control",{"action":"complete","result":"Original update processed"})])
            owner.until(lambda: owner.rows("SELECT id FROM work_nodes WHERE id=? AND status='completed'", work))
            ctx.equal("original Work completion buys no confirmation request", owner.rows("SELECT count(*) FROM model_requests"), [(preparation_requests+3,)])
            ctx.check("no additional physical request follows completed Work", http.wires.empty())
            return {"original_source":event,"principal":principal,"work":work,"wrong":wrong,"unasked":unasked,
                    "other_type":other_type,"other_source":other_source,"matched":right,
                    "waits":owner.rows("SELECT id,state,wake_execution FROM work_waits")}


def media_order_and_expiry(ctx, sdk, aux):
    import base64
    import sqlite3
    from contextlib import closing
    spec = importlib.util.spec_from_file_location("notification_png", ctx.root / "tests/cases/CORE-038.py")
    media = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(media)
    with Recorder() as http:
        owner, folder, entry, _, params = private_owner(ctx, sdk, "media-news", http, media=True)
        peer = aux.Peer(owner, folder)
        environment = {**os.environ,"YUKI_NOTIFICATION_QQ":"offline-media"}
        with owner.svc.running(environment=environment):
            owner.until((folder / "ready.json").exists)
            human = owner.human("Create my original notification pictures")
            _, q = owner.wire(http)
            reply(q, calls=[call("make","make_notice_images",{})])
            owner.until(lambda:list(folder.glob("call-*.json")))
            handle = json.loads(next(folder.glob("call-*.json")).read_bytes())["params"]["handle"]
            images = [media.png(pixel) for pixel in [b"\x01\x02\x03", b"\x21\x67\xc9"]]
            digests = [hashlib.sha256(raw).hexdigest() for raw in images]
            versions = []
            for index, raw in enumerate(images):
                result = peer.call("media.publish", {
                    "handle":handle,"key":"image-"+str(index),"filename":"picture-"+str(index)+".png",
                    "kind":"image","mime":"image/png","digest":digests[index],"size":len(raw),
                    "offset":0,"bytes":base64.b64encode(raw).decode(),"ttl_seconds":60,
                })
                versions.append(result["artifact_id"])
            (folder / ("finish-" + handle + ".json")).write_text(json.dumps({"ok":True,"data":{"artifacts":versions}}))
            _, q = owner.wire(http)
            reply(q, "images retained")
            owner.finished(human)
            qq = QQ(owner.svc.address, "100", "offline-media")
            try:
                original = {**params,"media_handles":versions}
                published = peer.call("notification.publish", original)
                wires = [receive(ctx, qq, "send_private_msg", 5820+index) for index in range(3)]
                ctx.equal("publication preserves v4 media-first then text delivery",
                          [wire["params"]["message"][0]["type"] for wire in wires], ["image","image","text"])
                owner.until(lambda: owner.rows("SELECT count(*) FROM notification_deliveries WHERE state='confirmed'")==[(3,)])
                references = [segment["data"]["file"] for wire in wires
                              for segment in wire["params"]["message"] if segment["type"]=="image"]
                ctx.check("original image versions reach the controlled receiver as bounded bytes",
                          all(reference.startswith("base64://") for reference in references))
                ctx.equal("actual receiver gets original image bytes in manifest order",
                          [hashlib.sha256(base64.b64decode(reference.removeprefix("base64://"))).hexdigest() for reference in references], digests)
                peer.call("notification.publish", {**original,"media_handles":list(reversed(versions))}, False)
                ctx.equal("reversed media cannot create a second publication", owner.rows("SELECT count(*) FROM notification_publications"), [(1,)])
                ctx.equal("retained image dependencies include original human source",
                          human in published["sources"]["ids"], True)
                leases = owner.rows("SELECT version,notification_ttl,notification_expires FROM host_media_uploads ORDER BY name")
            finally:
                qq.close()
        # Fault injection advances only the leases in this stopped disposable data store.
        with closing(sqlite3.connect(owner.svc.data / "core.sqlite")) as db:
            db.execute("UPDATE host_media_uploads SET notification_expires=0")
            db.commit()
        with owner.svc.running(environment=environment):
            replay = peer.call("notification.publish", original)
            ctx.equal("expired accepted handles retain their original publication",
                      replay["notification_id"], published["notification_id"])
            peer.call("notification.publish", {**original,"event_key":"new-expired-media"}, False)
            ctx.equal("new expired handles create no event or delivery",
                      owner.rows("SELECT (SELECT count(*) FROM notification_publications),(SELECT count(*) FROM social_outbound)"), [(1,3)])
        # A retained legacy reader may know an exact handle without knowing its old digest.
        # This deliberately downgraded fixture does not claim an imported v4 database.
        with closing(sqlite3.connect(owner.svc.data / "core.sqlite")) as db:
            manifest = json.loads(db.execute("SELECT manifest FROM notification_publications WHERE id=?", (published["notification_id"],)).fetchone()[0])
            manifest["media_handles"][0] = {"index":0,"legacy_handle_id":versions[0]}
            raw = json.dumps(manifest,ensure_ascii=False,sort_keys=True,separators=(",",":"))
            db.execute("UPDATE notification_publications SET manifest=?,digest=? WHERE id=?", (raw,hashlib.sha256(raw.encode()).hexdigest(),published["notification_id"]))
            db.execute("UPDATE notification_media SET digest=NULL WHERE version=?", (versions[0],))
            db.commit()
        with owner.svc.running(environment=environment):
            legacy = peer.call("notification.publish", original)
            ctx.equal("legacy unknown digest accepts only the original proved handle",
                      legacy["notification_id"], published["notification_id"])
            peer.call("notification.publish", {**original,"media_handles":[versions[1],versions[1]]}, False)
            ctx.equal("legacy handle reconciliation never fabricates a missing digest",
                      owner.rows("SELECT digest FROM notification_media WHERE version=?",versions[0]), [(None,)])
            owner.op("privacy","erase","notification-media-source",owner.actor,json.dumps([human]))
            owner.until(lambda:not owner.op("privacy","pending","media-news")["obligations"])
            ctx.equal("derived publication event joins the original erasure closure",
                      owner.rows("SELECT source FROM invalidated_sources ORDER BY source"), [(human,),(published["source_event_id"],)])
            ctx.equal("derived publication metadata retains its identity without the original body",
                      owner.rows("SELECT id,manifest,state FROM notification_publications"), [(published["notification_id"],None,"erased")])
            peer.call("notification.publish", original, False)
            return {"publication":published,"wire":wires,"leases":leases,"fault_expiry":0,
                    "ordered_digests":digests,"source":human,"expired_replay":replay,"legacy_replay":legacy}


def external_gateway(ctx, sdk, aux):
    owner = Owner(ctx, "notification-external", 34)
    with owner.svc.running():
        bot = owner.op("account","open","telegram","news-gateway","100",1,"--start",1)
        user = owner.op("account","open","telegram","news-gateway","200",1,"--person",owner.actor,"--start",1)
        space = json.dumps(["telegram","news-gateway","room"],separators=(",",":"))
        group = owner.op("entry","group",space)["entry"]
        private = owner.op("entry","private",user["assignment"],bot["assignment"])["entry"]
    folder, gateway = ctx.working / "external-news-peer", ctx.working / "notification-gateway"
    gateway.mkdir()
    installed(ctx, owner, sdk, folder, "external-news", {entry:bot["assignment"] for entry in [group,private]})
    with (owner.svc.data / "yuki.toml").open("a",encoding="utf-8") as stream:
        stream.write('\n[[plugins]]\nid="news-gateway"\nnamespace="news-gateway"\nplatform="telegram"\nbot_stage='
                     + json.dumps(bot["assignment"]) + '\nprogram=' + json.dumps(sys.executable)
                     + '\nargs=' + json.dumps([str(ctx.root / "tests/peers/host/peer.py"),str(gateway),str(sdk / "python-install")])
                     + '\ngrants=["gateway.send"]\n')
    peer = aux.Peer(owner, folder)
    with owner.svc.running():
        owner.until((folder / "ready.json").exists)
        owner.until(lambda:owner.rows("SELECT plugin FROM host_installations WHERE plugin='news-gateway' AND online"))
        first = peer.call("notification.publish",request(group,space,"external-group"))
        second = peer.call("notification.publish",request(private,space,"external-private",
            target={"target_type":"private","target_id":owner.actor,"conversation_id":private}))
        owner.until(lambda:owner.rows("SELECT count(*) FROM notification_deliveries WHERE state='confirmed'")==[(2,)])
        wires = [json.loads(line) for line in (gateway / "deliveries.jsonl").read_text().splitlines()]
        ctx.equal("installed external gateway receives two original publication operations",len(wires),2)
        ctx.equal("external gateway reports actual namespace-scoped confirmations",
                  peer.call("notification.status",{}),{"outbox_confirmed":2})
        ctx.equal("external notification receipt remains plugin-origin without a borrowed actor",
                  owner.rows("SELECT raw_actor,actor_stage FROM events WHERE id IN(SELECT event FROM notification_publications)"),[(None,None),(None,None)])
        ctx.equal("gateway publication replay retains its original external event",
                  peer.call("notification.publish",request(group,space,"external-group"))["source_event_id"],first["source_event_id"])
        return {"group":first,"private":second,"actual_gateway_frames":wires}
