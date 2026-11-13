"""A real installed SDK peer consumes only original paged gateway text."""

import base64
import hashlib
from itertools import count
import json
import os
from pathlib import Path
import queue
import sys
import threading
import time

folder = Path(sys.argv[1])
sys.path.insert(0, sys.argv[2])
sys.path.insert(1, str(Path(__file__).resolve().parents[3]))
from tests.harness.json_file import take_command
from yuki_plugin_sdk.connection import Binding, Peer, encode, read_frame
from yuki_plugin_sdk.host import Calls

lock = threading.RLock()
pending = {}
sequence = 10


def note(name, value):
    temporary = folder / (name + ".tmp")
    temporary.write_text(json.dumps(value), encoding="utf8")
    temporary.replace(folder / (name + ".json"))


def record(direction, frame):
    # The Host may terminate this peer at any point during shutdown. Publish
    # one complete record by rename so a cut cannot leave half a trace row.
    with lock:
        entry = folder / "wire" / f"{generation:06d}-{next(records):08d}"
        temporary = entry.with_suffix(".tmp")
        temporary.write_text(
            json.dumps({"direction": direction, "frame": frame}), encoding="utf8"
        )
        temporary.replace(entry.with_suffix(".json"))


def send(frame):
    with lock:
        record("plugin", frame)
        sys.stdout.buffer.write(encode(frame))
        sys.stdout.buffer.flush()


def call(method, params):
    global sequence
    with lock:
        sequence += 1
        ident = sequence
        waiting = pending[ident] = queue.Queue()
        send(rpc.request(ident, method, params))
    return waiting.get(timeout=10)


wire = folder / "wire"
wire.mkdir(exist_ok=True)
generation = 0
while True:
    try:
        (wire / f"{generation:06d}.generation").mkdir()
        break
    except FileExistsError:
        generation += 1
records = count()
binding = Binding.from_json(json.loads(os.environ["YUKI_PLUGIN_BINDING"]))
peer = Peer(binding)
send(peer.request(1, binding.offer()))
raw = read_frame(sys.stdin.buffer)
initial = json.loads(raw)
record("host", initial)
peer.receive(raw)
rpc = Calls(initial["result"])
note("ready", {"methods": sorted(rpc.allowed), "pid": os.getpid()})


def text(handle, size, digest):
    data = bytearray()
    while len(data) < size:
        result = call(
            "gateway.read_text",
            {"handle": handle, "offset": len(data), "length": 12288},
        )["result"]
        assert (
            result["offset"] == len(data)
            and result["total"] == size
            and result["sha256"] == digest
        )
        part = base64.b64decode(result["bytes"], validate=True)
        assert part and len(part) <= 12288
        data.extend(part)
        mode = (folder / "mode").read_text()
        if len(data) == len(part) and mode in {"authority", "erased"}:
            note("reading-" + mode, {"handle": handle, "offset": len(data)})
            deadline = time.monotonic() + 12
            while not (folder / ("resume-" + mode)).exists():
                if time.monotonic() > deadline:
                    raise TimeoutError("test authority barrier")
                time.sleep(0.01)
        assert result["done"] == (len(data) == size)
    assert len(data) == size and hashlib.sha256(data).hexdigest() == digest
    return data.decode("utf8")


def serve(frame):
    params = frame["params"]
    method = frame["method"]
    mode = (folder / "mode").read_text()
    try:
        args = params.get("arguments", {})
        if method == "forget.request":
            note("forgotten", params)
            result = {
                "key": params["key"],
                "watermark": params["watermark"],
                "proof": "volatile-text-cleared",
            }
        elif mode.startswith("mention-") and method in {
            "gateway.plan_send",
            "gateway.dispatch_part",
        }:
            from mentions import serve as original_mentions

            result = original_mentions(call, note, folder, mode, method, params)
        elif method == "gateway.plan_send":
            source = args.get("text_source")
            if source:
                assert "text" not in args["message"]
                assert "error" in call(
                    "gateway.read_text", {"handle": "foreign", "offset": 0, "length": 1}
                )
                body = text(params["handle"], source["bytes"], source["sha256"])
                assert "error" in call(
                    "gateway.read_text",
                    {
                        "handle": params["handle"],
                        "offset": source["bytes"] + 1,
                        "length": 1,
                    },
                )
                note(
                    "planned-" + mode,
                    {
                        "sha256": source["sha256"],
                        "bytes": len(body.encode()),
                        "handle": params["handle"],
                    },
                )
                parts = []
                offset = 0
                for start in range(0, len(body), 40000):
                    piece = body[start : start + 40000].encode()
                    parts.append(
                        {
                            "text_range": {
                                "start": offset,
                                "length": len(piece),
                                "sha256": hashlib.sha256(piece).hexdigest(),
                            }
                        }
                    )
                    offset += len(piece)
                if mode == "gap":
                    parts[1]["text_range"]["start"] += 3
                if mode == "digest":
                    parts[1]["text_range"]["sha256"] = "0" * 64
                if mode == "overlap":
                    parts[1]["text_range"]["start"] = 0
                if mode == "missing-tail":
                    parts.pop()
                if mode == "utf8":
                    parts[0]["text_range"]["length"] -= 1
            else:
                body = args["message"]["text"]
                parts = [{"text": body}]
            if "reply" in args:
                parts[0]["reply"] = args["reply"]
            result = {"parts": parts}
        elif method == "gateway.dispatch_part":
            part = args["part"]
            selection = part.get("text_range")
            body = (
                text(params["handle"], selection["length"], selection["sha256"])
                if selection
                else part["text"]
            )
            index = args["part_ordinal"]
            note(
                "sent-" + mode + "-" + str(index),
                {
                    "sha256": hashlib.sha256(body.encode()).hexdigest(),
                    "bytes": len(body.encode()),
                    "index": index,
                    "reply": args.get("reply"),
                    "route": args["route"],
                },
            )
            if mode == "lost" and index == 1:
                os._exit(0)
            result = (
                {"state": "unknown"}
                if mode == "unknown" and index == 1
                else {
                    "state": "confirmed",
                    "locator": "native-" + mode + "-" + str(index),
                }
            )
        else:
            raise ValueError("unsupported callback")
        send({"jsonrpc": "2.0", "id": frame["id"], "result": result})
    except Exception as error:
        note("fault-" + mode, {"type": type(error).__name__, "error": str(error)})
        send(
            {
                "jsonrpc": "2.0",
                "id": frame["id"],
                "error": {"code": -32020, "message": "original text unavailable"},
            }
        )


def commands():
    while True:
        request = take_command(folder / "probe.json")
        if request is not None:
            note("probed", call(request["method"], request["params"]))
        time.sleep(0.02)


threading.Thread(target=commands, daemon=True).start()
while raw := read_frame(sys.stdin.buffer):
    frame = json.loads(raw)
    record("host", frame)
    if "method" in frame:
        threading.Thread(target=serve, args=(frame,), daemon=True).start()
    else:
        with lock:
            waiting = pending.pop(frame["id"], None)
            if waiting is not None:
                rpc.receive(frame)
                waiting.put(frame)
