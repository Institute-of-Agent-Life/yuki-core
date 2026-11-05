"""An independently installed SDK peer. Its one-shot test mailbox never enters delivered evidence."""

import json
import os
from pathlib import Path
import sys
import threading
import time

root = Path(sys.argv[1])
sys.path.insert(0, sys.argv[2])
from yuki_plugin_sdk.connection import Binding, Peer, encode, read_frame
from yuki_plugin_sdk.host import Calls

lock = threading.Lock()


def send(frame):
    with lock:
        sys.stdout.buffer.write(encode(frame))
        sys.stdout.buffer.flush()


binding = Binding.from_json(json.loads(os.environ["YUKI_PLUGIN_BINDING"]))
peer = Peer(binding)
send(peer.request(1, binding.offer()))
line = read_frame(sys.stdin.buffer)
peer.receive(line)
calls = Calls(json.loads(line)["result"])
(root / "ready.json").write_text(
    json.dumps({"pid": os.getpid(), "methods": sorted(calls.allowed)})
)


def post():
    while True:
        for item in root.glob("cmd-*.json"):
            try:
                v = json.loads(item.read_text())
                item.unlink()
            except (FileNotFoundError, ValueError):
                continue
            send(calls.request(v["id"], v["method"], v["params"]))
        time.sleep(0.01)


threading.Thread(target=post, daemon=True).start()
for line in iter(lambda: read_frame(sys.stdin.buffer), None):
    if not line:
        break
    v = json.loads(line)
    if "id" not in v:
        continue
    if "method" in v:
        continue
    calls.receive(v)
    temp = root / f"out-{v['id']}.tmp"
    temp.write_text(json.dumps(v))
    temp.replace(temp.with_suffix(".json"))
