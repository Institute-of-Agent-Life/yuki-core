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


def serve(frame):
    p = frame["params"]
    method = frame["method"]
    try:
        if method == "forget.request":
            note("forget-" + p["key"], {"key": p["key"], "watermark": p["watermark"], "sources": p["sources"]})
            result = {
                "key": p["key"],
                "watermark": p["watermark"],
                "proof": "cleared-local-fixture",
            }
        elif method == "gateway.plan_send":
            chosen = p["arguments"].get("media")
            if (folder / "tamper-plan").exists() and chosen:
                chosen = dict(chosen, version="0" * 64)
            result = {
                "parts": [
                    {"text": "selected result", **({"media": chosen} if chosen else {})}
                ]
            }
        elif method == "gateway.dispatch_part":
            args = p["arguments"]
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
        elif method == "tool.call":
            images = []
            for n in range(2):
                image = (folder / f"image-{n}.png").read_bytes()
                h = hashlib.sha256(image).hexdigest()
                p0 = {
                    "handle": p["handle"],
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
