"""Installed SDK peer with a test-owned command inbox and actual stdio callbacks."""

import json
import os
import queue
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3]))
from tests.harness.json_file import read_json

folder = Path(sys.argv[1])
sys.path.insert(0, sys.argv[2])
from yuki_plugin_sdk.connection import Binding, Peer, encode, read_frame
from yuki_plugin_sdk.host import Calls

lock = threading.RLock()
pending = {}
sequence = 10


def send(frame):
    with lock:
        sys.stdout.buffer.write(encode(frame))
        sys.stdout.buffer.flush()


def save(name, data):
    path = folder / (name + ".tmp")
    path.write_text(json.dumps(data), encoding="utf-8")
    path.replace(folder / (name + ".json"))


def invoke(method, params):
    global sequence
    with lock:
        sequence += 1
        key = sequence
        frame = rpc.request(key, method, params)
        response = pending[key] = queue.Queue()
        send(frame)
    return response.get(timeout=40)


binding = Binding.from_json(json.loads(os.environ["YUKI_PLUGIN_BINDING"]))
peer = Peer(binding)
send(peer.request(1, binding.offer()))
raw = read_frame(sys.stdin.buffer)
peer.receive(raw)
rpc = Calls(json.loads(raw)["result"])
save("ready", {"pid": os.getpid(), "methods": sorted(rpc.allowed)})


def job(path):
    payload = read_json(path)
    try:
        result = invoke(payload["method"], payload["params"])
    except Exception as error:
        result = {"fixture_error": str(error)}
    save("reply-" + path.stem, result)


def inbox():
    seen = set()
    while True:
        for path in folder.glob("job-*.json"):
            if (
                path.name not in seen
                and not (folder / ("reply-" + path.stem + ".json")).exists()
            ):
                seen.add(path.name)
                threading.Thread(target=job, args=(path,), daemon=True).start()
        time.sleep(0.01)


threading.Thread(target=inbox, daemon=True).start()
while raw := read_frame(sys.stdin.buffer):
    frame = json.loads(raw)
    with lock:
        rpc.receive(frame)
    if "method" not in frame:
        pending.pop(frame["id"]).put(frame)
    elif frame["method"] == "observer.event":
        save("event-" + str(frame["params"]["event"]), frame["params"])
    elif frame["method"] == "forget.request":
        for path in folder.glob("private-*.json"):
            path.unlink()
        p = frame["params"]
        send(
            {
                "jsonrpc": "2.0",
                "id": frame["id"],
                "result": {
                    "key": p["key"],
                    "watermark": p["watermark"],
                    "proof": "fixture private content removed",
                },
            }
        )
    elif frame["method"] in ("tool.call", "command.invoke", "media.process"):
        save("call-" + frame["params"]["handle"], frame)

        # The test explicitly decides when the original plugin call settles.
        def finish(original):
            marker = folder / ("finish-" + original["params"]["handle"] + ".json")
            result = read_json(marker) or {
                "ok": True,
                "data": "auxiliary fixture complete",
            }
            send({"jsonrpc": "2.0", "id": original["id"], "result": result})

        threading.Thread(target=finish, args=(frame,), daemon=True).start()
