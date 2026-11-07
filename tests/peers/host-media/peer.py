"""Installed SDK consumer. The fixture directory is the plugin's own data, never the core DB."""

import base64, hashlib, json, os, queue, sys, threading, time
from pathlib import Path

folder = Path(sys.argv[1])
folder.mkdir(exist_ok=True, parents=True)
sys.path.insert(0, sys.argv[2])
from yuki_plugin_sdk.connection import Binding, Peer, encode, read_frame
from yuki_plugin_sdk.host import Calls

lock = threading.RLock()
pending = {}
seq = 10


def send(f):
    with lock:
        sys.stdout.buffer.write(encode(f))
        sys.stdout.buffer.flush()


def note(name, value):
    stage = folder / (name + ".tmp")
    stage.write_text(json.dumps(value), encoding="utf-8")
    stage.replace(folder / (name + ".json"))


def call(method, p):
    global seq
    with lock:
        seq += 1
        i = seq
        wait = pending[i] = queue.Queue()
        send(rpc.request(i, method, p))
    res = wait.get(timeout=7)
    return res


binding = Binding.from_json(json.loads(os.environ["YUKI_PLUGIN_BINDING"]))
peer = Peer(binding)
send(peer.request(1, binding.offer()))
init = read_frame(sys.stdin.buffer)
confirmed = json.loads(init)
peer.receive(init)
rpc = Calls(confirmed["result"])
note("ready", {"pid": os.getpid(), "methods": sorted(rpc.allowed)})


def publish_frames(handle):
    images = []
    for n in range(2):
        image = (folder / f"image-{n}.png").read_bytes()
        h = hashlib.sha256(image).hexdigest()
        p0 = {
            "handle": handle,
            "key": "image-" + str(n),
            "filename": f"image-{n}.png",
            "mime": "image/png",
            "kind": "image",
            "size": len(image),
            "digest": h,
        }
        for off in range(0, len(image), 16384):
            body = dict(
                p0,
                offset=off,
                bytes=base64.b64encode(image[off : off + 16384]).decode(),
            )
            part = call("media.publish", body)["result"]
            repeated = call("media.publish", body)["result"]
            assert part == repeated
        images.append(
            {"artifact_id": part["artifact_id"], "version": part["version"]}
        )
    return images


def serve(frame):
    p = frame["params"]
    method = frame["method"]
    if method == "gateway.activity":
        with lock, (folder / "activity.ndjson").open("a", encoding="utf-8") as records:
            records.write(json.dumps(p) + "\n")
        return
    try:
        if method == "forget.request":
            note("forget-" + p["key"], {"key": p["key"], "watermark": p["watermark"], "sources": p["sources"]})
            result = {
                "key": p["key"],
                "watermark": p["watermark"],
                "proof": "cleared-local-fixture",
            }
        elif method == "gateway.media":
            idx, offset = p["attachment_index"], p["offset"]
            if p["media_identity"] != json.loads((folder/"authority.json").read_bytes()):
                raise ValueError("native credentials differ from the original application or tenant")
            with lock, (folder/"fetch-calls.ndjson").open("a",encoding="utf-8") as calls:
                calls.write(json.dumps([p["source_event"],idx,offset])+"\n")
            raw = (folder/"native.bin").read_bytes()
            note("fetch-"+str(idx)+"-"+str(offset), p)
            if p["reference"] == "late" and offset:
                deadline = time.monotonic()+7
                while not (folder/"release-page").exists() and time.monotonic()<deadline: time.sleep(.01)
            chunk = raw[offset:offset+p["length"]]
            result = {"transfer":p["transfer"],"offset":offset,"size":len(raw),"sha256":hashlib.sha256(raw).hexdigest(),"data":base64.b64encode(chunk).decode()}
            if p["reference"]=="wrong-digest": result["sha256"]="f"*64
            if p["reference"]=="wrong-offset": result["offset"]+=1
        elif method == "gateway.plan_send":
            chosen = p["arguments"].get("media")
            if (folder / "tamper-plan").exists() and chosen:
                chosen = dict(chosen, version="0" * 64)
            result = {
                "parts": [
                    {"text": "selected result", **({"media": chosen} if chosen else {})}
                ]
            }
            if original_reply := p["arguments"].get("reply"):
                result["parts"][0]["reply"] = original_reply
                if (folder / "tamper-reply").exists():
                    result["parts"][0]["reply"] = {**original_reply, "native_id": "foreign"}
                if (folder / "pause-reply-plan").exists():
                    note("reply-plan-waiting", p)
                    deadline = time.monotonic() + 10
                    while (folder / "pause-reply-plan").exists() and time.monotonic() < deadline:
                        time.sleep(.01)
        elif method == "gateway.dispatch_part":
            args = p["arguments"]
            if args.get("reply"):
                note("reply-dispatched", args)
            offset = 0
            chunks = []
            if args.get("media"):
                while True:
                    r = call(
                        "media.open",
                        {"handle": p["handle"], "offset": offset, "length": 16384},
                    )["result"]
                    val = base64.b64decode(r["bytes"], validate=True)
                    chunks.append(val)
                    offset += len(val)
                    if r["done"]:
                        break
                note(
                    "routed",
                    {
                        "digest": hashlib.sha256(b"".join(chunks)).hexdigest(),
                        "route": args["route"],
                        "version": args["media"]["version"],
                    },
                )
            result = {
                "state": "confirmed",
                "locator": "original-native-" + p["operation"],
            }
        elif method == "gateway.recall":
            with lock, (folder / "recalls.ndjson").open("a", encoding="utf-8") as calls:
                calls.write(json.dumps(p["arguments"]) + "\n")
            if (folder / "recall-lost").exists():
                os._exit(7)
            result = {"state":"failed" if (folder / "recall-failed").exists() else "confirmed"}
        elif method == "media.process":
            handle = p["handle"]
            name = p["operation"]
            note("active-" + name, {"handle": handle})
            data = bytearray()
            offset = 0
            while True:
                r = call(
                    "media.open", {"handle": handle, "offset": offset, "length": 16384}
                )["result"]
                data.extend(base64.b64decode(r["bytes"], validate=True))
                offset = len(data)
                if r["done"]:
                    break
            note(
                "read-" + name,
                {
                    "sha256": hashlib.sha256(data).hexdigest(),
                    "size": len(data),
                    "declared": r["version"],
                },
            )
            until = time.monotonic() + 9
            while (folder / ("hold-" + name)).exists() and time.monotonic() < until:
                time.sleep(0.02)
            result = {
                "ok": True,
                "data": {
                    "text": "controlled transcription /ai admin",
                    "digest": hashlib.sha256(data).hexdigest(),
                },
            }
            if p["name"] == "inspect_conversation_attachment":
                purpose=p["arguments"]["purpose"]
                if purpose=="frames":
                    images=publish_frames(handle)
                    images.append({"artifact_id":"foreign","version":"0"*64})
                    result={"ok":True,"data":{"images":images,"text":"controlled selected frames"}}
                else:
                    result={"ok":True,"data":{"text":"controlled selected PDF page"}}
                note("selected-ready",{"operation":name,"handle":handle,"purpose":purpose,"media":p["arguments"]["media"],"digest":hashlib.sha256(data).hexdigest()})
                deadline=time.monotonic()+9
                while (folder/"hold-selected").exists() and time.monotonic()<deadline:time.sleep(.02)
        elif method == "tool.call":
            images = publish_frames(p["handle"])
            images.append(json.loads((folder / "foreign.json").read_text()))
            file = folder / "paper.dat"
            doc = None
            if file.is_file():
                with file.open("rb") as original:
                    selected = original.read()
                swapped = file.with_suffix(".next")
                swapped.write_bytes(b"replacement at same plugin path")
                swapped.replace(file)
                digest = hashlib.sha256(selected).hexdigest()
                pub = {
                    "handle": p["handle"],
                    "key": "paper",
                    "filename": "paper.bin",
                    "kind": "file",
                    "mime": "application/octet-stream",
                    "digest": digest,
                    "size": len(selected),
                }
                rejected = call(
                    "media.publish",
                    dict(pub, filename="../unbound", offset=0, bytes=""),
                )
                assert "error" in rejected
                for off in range(0, len(selected), 16384):
                    doc = call(
                        "media.publish",
                        dict(
                            pub,
                            offset=off,
                            bytes=base64.b64encode(
                                selected[off : off + 16384]
                            ).decode(),
                        ),
                    )["result"]
            with (folder / "executions.log").open("a") as f:
                f.write(p["handle"] + "\n")
            result = {
                "ok": True,
                "data": {"effect": "occurred", "images": images, "document": doc},
            }
        else:
            raise ValueError("method unavailable")
        send({"jsonrpc": "2.0", "id": frame["id"], "result": result})
    except Exception as e:
        note("fault", {"type": type(e).__name__, "message": str(e)})
        send(
            {
                "jsonrpc": "2.0",
                "id": frame["id"],
                "error": {"code": -32020, "message": "controlled peer rejected"},
            }
        )


def commands():
    while True:
        p = folder / "probe.json"
        if p.is_file():
            raw = json.loads(p.read_bytes())
            p.unlink()
            result = call(raw["method"], raw["params"])
            note(
                "probed", {"denied": "error" in result, "result": result.get("result")}
            )
        time.sleep(0.02)


threading.Thread(target=commands, daemon=True).start()
while raw := read_frame(sys.stdin.buffer):
    f = json.loads(raw)
    if "method" in f:
        threading.Thread(target=serve, args=(f,), daemon=True).start()
    else:
        with lock:
            w = pending.pop(f["id"], None)
            if w is not None:
                try:
                    rpc.receive(f)
                except (ValueError, RuntimeError):
                    pass
                w.put(f)
