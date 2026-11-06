"""Two separately installed SDK peers receive the same native key, with independent tenant credentials."""
import hashlib
import json
import sqlite3
import sys
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from pathlib import Path
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def refused_media_send(ctx, owner, model, queue, execution, arguments, reason):
    before = owner.rows("SELECT count(*) FROM host_calls WHERE execution=? AND method IN ('gateway.plan_send','gateway.dispatch_part')", execution)
    reply(queue, calls=[call(reason, "send", arguments)])
    wire, queue = owner.wire(model)
    receipt = json.loads(next(m["content"] for m in wire["messages"] if m.get("tool_call_id") == reason))
    ctx.equal(f"{reason} returns to the requesting Main", (receipt["state"], receipt["result"]),
              ("failed", {"error": reason}))
    ctx.equal(f"{reason} does not plan or dispatch any part",
              owner.rows("SELECT count(*) FROM host_calls WHERE execution=? AND method IN ('gateway.plan_send','gateway.dispatch_part')", execution), before)
    ctx.equal(f"{reason} can be read by original operation",
              owner.op("effect", "read", execution, receipt["id"], owner.actor)["result"], receipt["result"])
    return queue


def run(ctx, sdk):
    o=Owner(ctx,"gw-media",48)
    peers={}
    with o.svc.running():
        for n,platform in enumerate(("telegram","lark")):
            name=f"gw-media-{n}"
            bot=o.op("account","open",platform,name,"physical-bot",1,"--start",1)["assignment"]
            o.op("account","open",platform,name,"sender",1,"--person",o.actor,"--start",1)
            folder=ctx.working/name;folder.mkdir()
            identity={"application":"tg-bot" if n==0 else "lark-app","tenant":"realm-"+str(n)}
            data=(bytes([n+19])*30000)+b"native material"
            (folder/"authority.json").write_text(json.dumps(identity))
            (folder/"native.bin").write_bytes(data)
            peers[name]=(folder,bot,identity,data)
            grants = ["gateway.inbound", "gateway.media"] + (["gateway.send"] if n == 1 else [])
            with (o.svc.data/"yuki.toml").open("a") as f:
                f.write(f'\n[[plugins]]\nid="{name}"\nnamespace="{name}"\nplatform="{platform}"\nbot_stage="{bot}"\nprogram='+json.dumps(sys.executable)+'\nargs='+json.dumps([str(ctx.root/"tests/peers/host-media/peer.py"),str(folder),str(sdk/"python-install")])+'\ngrants='+json.dumps(grants)+'\n[plugins.gateway_media]\napplication='+json.dumps(identity["application"])+'\ntenant='+json.dumps(identity["tenant"])+'\n')

    def submit(folder,request,deny=False):
        old=folder/"probed.json"
        if old.exists():old.unlink()
        t=folder/"probe.tmp";t.write_text(json.dumps({"method":"gateway.inbound","params":request}));t.replace(folder/"probe.json")
        o.until(old.exists);out=json.loads(old.read_bytes())
        ctx.equal("inbound original identity accepted or rejected as specified",out["denied"],deny)
        return out["result"]

    def edit(stmt,*args):
        with closing(sqlite3.connect(o.svc.data/"core.sqlite")) as db:db.execute(stmt,args);db.commit()
    with Recorder() as model:
        o.config(model, binding=False)
        with o.svc.running():
            for folder,_,_,_ in peers.values():
                o.until(lambda:(folder/"ready.json").is_file())
                ctx.check("installed SDK negotiated gateway.media","gateway.media" in json.loads((folder/"ready.json").read_bytes())["methods"])
            def frame(identity):
                return {"source":"source-1","native_id":"native-message","native_version":"v1","sender":"sender","kind":"message","occurred":11,"accept":True,"media_identity":identity,"segments":[{"type":"text","text":"select original sources"},*[{"type":"media","kind":"file","reference":ref} for ref in ("same-key","wrong-digest","wrong-offset","late","expired","other-slot")]]}
            def pin(exec_id,event,slot,op,ok=True):
                return o.svc.command("operator","media","pin",exec_id,o.actor,str(event),str(slot),op,"native.bin","file",accepted=ok)
            observations=[]
            for idx,(name,(folder,bot,identity,data)) in enumerate(peers.items()):
                log=folder/"fetch-calls.ndjson"
                count=lambda: len(log.read_text(encoding="utf-8").splitlines()) if log.exists() else 0
                target=frame(identity)
                submit(folder,{**target,"media_identity":{**identity,"tenant":"other-tenant"}},True)
                ctx.equal("wrong native identity starts no download or business event",(count(),o.rows("SELECT count(*) FROM gateway_media_origins WHERE plugin=?",name)),(0,[(0,)]))
                event=submit(folder,target)["event"]; _,q=o.wire(model)
                exec_id=o.rows("SELECT id FROM executions WHERE source=? ORDER BY rowid LIMIT 1",event)[0][0]
                ctx.equal("inbound redelivery reuses the same event",submit(folder,target)["event"],event)
                origin=o.rows("SELECT plugin,stage,json_extract(identity,'$.tenant'),json_extract(locator,'$.native_id') FROM gateway_media_origins WHERE event=?",event)
                ctx.equal("committed original stage, tenant and native message",origin,[(name,bot,identity["tenant"],"native-message")])
                if idx == 0:
                    reply(q, calls=[call("send-without-grant", "send", {"text": "unauthorized outbound"})])
                    wire, q = o.wire(model)
                    refused = json.loads(next(m["content"] for m in wire["messages"]
                                              if m.get("tool_call_id") == "send-without-grant"))
                    ctx.equal("an ingress-only installation returns a final refusal to Main",
                              (refused["state"], refused["result"]), ("failed", {"error": "gateway_send_not_granted"}))
                    ctx.equal("a denied send creates no plan or physical dispatch handles",
                              o.rows("SELECT count(*) FROM host_calls WHERE execution=? AND method IN ('gateway.plan_send','gateway.dispatch_part')", exec_id),
                              [(0,)])
                    ctx.equal("reading the original denied operation returns its immutable receipt",
                              o.op("effect", "read", exec_id, refused["id"], o.actor)["result"], refused["result"])
                else:
                    q = refused_media_send(ctx, o, model, q, exec_id,
                                           {"artifact_id": "unread-version", "attachment_kind": "file"},
                                           "gateway_media_read_not_granted")
                pin(exec_id,event,0,"pin-good")
                h=hashlib.sha256(data).hexdigest()
                ctx.equal("original bound gateway returned selected bytes only",o.rows("SELECT state,digest,segment,acquired_via FROM media_sources WHERE event=? AND slot=0",event),[("cached",h,1,"gateway.media")])
                first=json.loads((folder/"fetch-0-0.json").read_bytes());second=json.loads((folder/"fetch-0-24576.json").read_bytes())
                ctx.equal("callback pinned physical receiver and original segment across both chunks",(first["transfer"],first["source_event"],first["attachment_index"],first["segment_index"],first["reference"],first["namespace"],first["media_identity"],first["bot_stage"]),(second["transfer"],event,0,1,"same-key",name,identity,bot))
                if idx==0:
                    other=peers["gw-media-1"][0]
                    ctx.equal("the other installation never receives the first native key",list(other.glob("fetch-*.json")),[])
                ctx.equal("one physical callback per requested block",count(),2)
                before=count()
                legacy={k:v for k,v in target.items() if k!="media_identity"}
                legacy.update(source="pre-binding",native_id="old-receipt",accept=False)
                old_event=submit(folder,legacy)["event"]
                ctx.equal("retry cannot attach a new identity to unbound ingress",submit(folder,{**legacy,"media_identity":identity})["event"],old_event)
                pin(exec_id,old_event,0,"old-no-binding",False)
                ctx.equal("unbound event makes no callback",count(),before)
                pin(exec_id,event,0,"pin-saved")
                pin(exec_id,event,99,"bad-index",False)
                edit("UPDATE media_sources SET expires=0 WHERE event=? AND slot=4",event)
                pin(exec_id,event,4,"expired",False)
                ctx.equal("cached or expired source and wrong slot start no native fetch",count(),before)
                pin(exec_id,event,1,"bad-digest",False);pin(exec_id,event,2,"bad-offset",False)
                ctx.equal("invalid bytes and offsets cannot commit a cached object",o.rows("SELECT count(*) FROM media_sources WHERE event=? AND slot IN(1,2) AND digest IS NOT NULL",event),[(0,)])
                old=o.rows("SELECT value FROM core_meta WHERE key=?","plugin-approval:"+name)[0][0]
                edit("UPDATE core_meta SET value=json_set(value,'$.deployment','replacement-deployment') WHERE key=?","plugin-approval:"+name)
                before=count();pin(exec_id,event,5,"replacement",False);pin(exec_id,event,0,"replacement-cached",False)
                ctx.equal("changed deployment cannot use fresh or cached original",count(),before)
                edit("UPDATE core_meta SET value=? WHERE key=?",old,"plugin-approval:"+name)
                grants=o.rows("SELECT grants FROM host_installations WHERE plugin=?",name)[0][0]
                edit("UPDATE host_installations SET grants=? WHERE plugin=?",json.dumps(["gateway.inbound"]),name)
                before=count();pin(exec_id,event,5,"revoked-fetch",False);ctx.equal("removed method grant issues zero native reads",count(),before)
                edit("UPDATE host_installations SET grants=? WHERE plugin=?",grants,name)
                o.op("account","enabled",bot,"false",2000)
                before=count();pin(exec_id,event,5,"wrong-stage",False)
                ctx.equal("paused original Bot issues no native reads",count(),before)
                o.op("account","enabled",bot,"true",2001)
                if idx==1:
                    with ThreadPoolExecutor() as pool:
                        pending=pool.submit(pin,exec_id,event,3,"late",False)
                        o.until(lambda:(folder/"fetch-3-24576.json").is_file())
                        o.op("privacy","erase","erase-during-fetch",o.actor,json.dumps([event]))
                        (folder/"release-page").touch();pending.result(timeout=12)
                    ctx.equal("late final page cannot resurrect forgotten original",o.rows("SELECT state,digest FROM media_sources WHERE event=? AND slot=3",event),[("erased",None)])
                    ctx.equal("forget removes bound private native locator",o.rows("SELECT count(*) FROM gateway_media_origins WHERE event=?",event),[(0,)])
                reply(q,"")
                if idx==0:o.finished(event)
                observations.append({"plugin":name,"source_event":event,"native_sha256":h,"segment_index":first["segment_index"],"chunks":[first["offset"],second["offset"]]})
            ctx.check("same native key selects distinct bytes in two origins",observations[0]["native_sha256"]!=observations[1]["native_sha256"])
    receipt={"original_oracle_sha256":hashlib.sha256((ctx.root/"fixtures/media/gateway-media-v4.json").read_bytes()).hexdigest(),"method":"gateway.media","grant":"gateway.media","sources":observations,"wrong_tenant_downloads":0,"expired_downloads":0,"changed_deployment_downloads":0,"late_publish":False,"inbound_only_send":{"state":refused["state"],"result":refused["result"],"gateway_calls":0}}
    ctx.write("gateway-inbound-media.json",receipt)
    return receipt
