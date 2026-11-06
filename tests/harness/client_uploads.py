"""Exercise registered bytes, request namespaces and ordered original event slots through HTTP."""
import hashlib
from http.client import HTTPConnection
import json
import sqlite3
import uuid
from contextlib import closing
from datetime import UTC, datetime
from urllib.parse import quote, urlencode

from tests.harness.compact import Owner, reply
from tests.peers.chat.recorder import Recorder


def run(ctx, wire, routes):
    key = lambda: str(uuid.uuid4())
    sha = lambda b: hashlib.sha256(b).hexdigest()
    o = Owner(ctx, "client-uploads", 47)
    fixture = ctx.root / "fixtures/personal/client-uploads-v4.json"
    stranger = key()

    def raw(method, url, token, content=None, expected=200):
        with closing(HTTPConnection(o.svc.address, timeout=25)) as c:
            c.request(method, url, body=content, headers={"Authorization": "Bearer " + token})
            res = c.getresponse()
            data = res.read()
            ctx.check(f"original upload HTTP {method} expects {expected}, received {res.status}", res.status == expected)
            if res.headers.get_content_type() == "application/octet-stream":
                return data, res.headers
            doc = json.loads(data)
            return doc.get("data", doc)

    def http(path, token=None, body=None, expected=200, method=None):
        status, doc = wire(o.svc.address, path, token, body, method)
        ctx.check(f"upload Client9 {path} expects {expected}, received {status}", status == expected)
        return doc.get("data", doc)

    def pair(person):
        code = o.op("personal", "issue", person, key(), "chat")["code"]
        return http("/client/v1/pair", body={"code": code, "device_label": "upload fixture"})

    def spec(data, filename="測試.bin", kind="file"):
        return dict(request_id=key(), filename=filename, sha256=sha(data), size=len(data), kind=kind)

    def path(s):
        return "/client/v1/uploads?" + urlencode(s)

    def counts():
        return o.rows("SELECT (SELECT count(*) FROM events),(SELECT count(*) FROM model_attempts),(SELECT count(*) FROM client_uploads),(SELECT count(*) FROM media_objects)")[0]

    def sql(statement, *args):
        with closing(sqlite3.connect(o.svc.data / "core.sqlite")) as db:
            db.execute(statement, args)
            db.commit()

    def pending(s, token, chunk):
        c = HTTPConnection(o.svc.address, timeout=15)
        c.putrequest("POST", path(s))
        c.putheader("Authorization", "Bearer " + token)
        c.putheader("Content-Length", str(s["size"]))
        c.endheaders(chunk)
        return c

    # A browser upload can exceed the inline media/tool boundary without boxing its bytes in JSON.
    field = b"undisclosed binary payload\0\xff"
    large = (field * (17 * 1024 * 1024 // len(field) + 1))
    picture = b"\x89PNG\r\n\x1a\nprivate fixture"
    with Recorder() as model:
        o.config(model)
        with o.svc.running():
            o.op("person", "create", stranger)
            a, sibling, outsider = pair(o.actor), pair(o.actor), pair(stranger)
            A, A2, B = a["token"], sibling["token"], outsider["token"]
            initial = counts()
            one = spec(large)
            slow_data = b"still arriving"
            slow = spec(slow_data)
            with closing(pending(slow, A2, slow_data[:3])) as held:
                first = raw("POST", path(one), A, large)
                http("/client/v1/devices/" + sibling["device_id"], A, method="DELETE")
                held.send(slow_data[3:])
                r = held.getresponse()
                ctx.equal("authorization at EOF rejects a revoked credential", (r.status,json.loads(r.read())["problem"]["code"]), (401,"unauthenticated"))
            sibling = pair(o.actor)
            A2 = sibling["token"]
            ctx.equal("RFC UUIDv5 uses original request namespace", first["media_id"], str(uuid.uuid5(uuid.NAMESPACE_URL, "yuki:upload:"+o.actor+":"+one["request_id"])))
            ctx.equal("one upload, no admission or model call", counts(), (initial[0], initial[1], initial[2]+1, initial[3]+1))
            ctx.equal("sibling replay uses original receipt before receiving new body", raw("POST",path(one),A2,b""), {**first,"replay":True})
            req = "/client/v1/requests/" + one["request_id"]
            ctx.equal("lost ACK resolved without another registration",http(req,A2)["result"]["media_id"],first["media_id"])
            for change in ({"filename":"other"},{"kind":"video"},{"size":3},{"sha256":"a"*64}):
                raw("POST",path({**one,**change}),A2,b"",409)
            for change in ({"sha256":"a"*64},{"size":7},{"filename":"../unsafe"},{"kind":"voice"}):
                raw("POST",path({**spec(b"bad"),**change}),A,b"bad",422)
            interrupted=spec(b"interrupted body");pending(interrupted,A,b"inter").close()
            ctx.equal("unfinished request invisible",http("/client/v1/requests/"+interrupted["request_id"],A)["found"],False)
            recovered=raw("POST",path(interrupted),A,b"interrupted body")
            ctx.equal("interrupted upload retry registers original", recovered["replay"],False)
            reject=spec(b"transaction must roll back")
            sql("CREATE TRIGGER fail_upload BEFORE INSERT ON personal_receipts WHEN NEW.operation='"+reject["request_id"]+"' BEGIN SELECT RAISE(ABORT,'rollback upload'); END")
            before=counts()
            raw("POST",path(reject),A,b"transaction must roll back",422)
            ctx.equal("failed receipt atomically rolls back bytes and ownership",counts(),before)
            two=spec(picture,"圖片.png","image")
            second=raw("POST",path(two),A,picture)
            ids=[first["media_id"],second["media_id"]]
            msg={"request_id":key(),"text":"read these","attachments":ids}
            http("/client/v1/messages",B,msg,404)
            http("/client/v1/messages",A,{**msg,"request_id":one["request_id"]},409)
            sent=http("/client/v1/messages",A,msg)
            event=sent["event_id"]
            body,q=o.wire(model)
            prompt=json.dumps(body["messages"],ensure_ascii=False)
            ctx.check("original Main observes selected attachment names and no file content",one["filename"] in prompt and two["filename"] in prompt and "undisclosed binary" not in prompt and "private fixture" not in prompt)
            reply(q,"")
            o.finished(event)
            http("/client/v1/messages",A2,{**msg,"attachments":list(reversed(ids))},409)
            ctx.equal("same ordered request has one event",http("/client/v1/messages",A2,msg)["event_id"],event)
            ctx.equal("normalized source positions preserve order",o.rows("SELECT slot,segment,digest,state FROM media_sources WHERE event=? ORDER BY slot",event),[(0,1,one["sha256"],"cached"),(1,2,two["sha256"],"cached")])
            conversation="/client/v1/conversations/"+sent["conversation_id"]+"/events"
            item=next(i for i in http(conversation,A)["items"] if i["event_id"]==event)
            ctx.equal("history exposes original names, sizes and original indices",[(p["attachment_index"],p["segment_index"],p["filename"],p["size"]) for p in item["attachments"]],[(0,1,one["filename"],len(large)),(1,2,two["filename"],len(picture))])
            link=conversation+"/"+str(event)+"/attachments/"
            data,h=raw("GET",link+"0",A2)
            until=o.rows("SELECT expires FROM media_sources WHERE event=? AND slot=0",event)[0][0]
            ctx.equal("original bytes and immutable digest, current expiry and encoded file name",(len(data),sha(data),h["ETag"],h["X-Yuki-Expires-At"],h["Content-Disposition"]), (len(large),one["sha256"],'"'+one["sha256"]+'"',datetime.fromtimestamp(until/1000,UTC).isoformat(),"attachment; filename*=UTF-8''"+quote(one["filename"])))
            raw("GET",link+"0",B,expected=404)
            raw("GET",link+"2",A,expected=404)
            duplicate=raw("POST",path(spec(large)),B,large)
            ctx.check("same bytes do not share an authorization handle",duplicate["media_id"] != first["media_id"])
            ctx.equal("stored object always has an owner before garbage collection",o.rows("SELECT count(*) FROM media_objects WHERE digest NOT IN(SELECT digest FROM live_media_digests)"),[(0,)])
            only=http("/client/v1/messages",B,{"request_id":key(),"text":"","attachments":[duplicate["media_id"]]})
            _,q=o.wire(model);reply(q,"");o.finished(only["event_id"])
            foreign_link="/client/v1/conversations/"+only["conversation_id"]+"/events/"+str(only["event_id"])+"/attachments/0"
            sql("UPDATE media_objects SET bytes=? WHERE digest=?",b"Z"*len(picture),two["sha256"])
            raw("GET",link+"1",A,expected=404)
            http("/client/v1/messages",A,{**msg,"request_id":key(),"attachments":[second["media_id"]]},404)
            raw("POST",path(spec(picture)),A,picture,422)
            sql("UPDATE client_uploads SET expires=0 WHERE id=?",first["media_id"])
            http("/client/v1/messages",A,{**msg,"request_id":key()},404)
        with o.svc.running():
            ctx.equal("cold replay after upload expiry returns committed event",http("/client/v1/messages",A,msg)["event_id"],event)
            ctx.equal("expired upload receipt remains queryable",http(req,A)["result"]["media_id"],first["media_id"])
            ctx.equal("independent event lifetime still allows original bytes",sha(raw("GET",link+"0",A)[0]),one["sha256"])
            sql("UPDATE media_sources SET expires=0 WHERE event=? AND slot=0",event)
            raw("GET",link+"0",A,expected=404)
            # Exercise the normal original-superuser HTTP forget entry, including upload-only references.
            o.op("person","set-admin",stranger,"true")
            erased=http("/admin/v1/persons/"+o.actor+"/forget",B,{"request_id":key()})
            ctx.check("original authorized forget revokes device and accounts for shared bytes",erased["deleted"] and erased["managed_storage"]["shared_blobs_retained"]>=1)
            raw("GET",link+"0",A,expected=401)
            ctx.equal("removed all forgotten ownership and names",o.rows("SELECT (SELECT count(*) FROM client_uploads WHERE owner=?),(SELECT count(*) FROM client_upload_parts WHERE event=?)",o.actor,event),[(0,0)])
            o.until(lambda: not o.rows("SELECT digest FROM media_objects WHERE digest=?",interrupted["sha256"]))
            ctx.equal("forgotten upload-only bytes physically removed",o.rows("SELECT count(*) FROM media_objects WHERE digest=?",interrupted["sha256"]),[(0,)])
            ctx.equal("unrelated owner's identical content remains downloadable",sha(raw("GET",foreign_link,B)[0]),one["sha256"])
    routes.update(["POST /client/v1/uploads","GET /client/v1/conversations/{identity}/events/{event_id}/attachments/{attachment_index}"])
    return {"original_oracle_sha256":sha(fixture.read_bytes()),"large_bytes":len(large),"sha256":one["sha256"],"ordered_slots":[0,1],"request_replay_event":event,"headers_verified":True,"revoked_during_upload_status":401,"rollback_counts":list(before),"shared_content_survived_forget":True}
