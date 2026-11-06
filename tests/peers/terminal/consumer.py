"""Public SDK peer; its test mailbox controls when each actual stdio reply is released."""

import json, os, sys, threading, time
from pathlib import Path

folder = Path(sys.argv[1])
sys.path.insert(0, sys.argv[2])
sys.path.insert(1, str(Path(__file__).resolve().parents[3]))
from tests.harness.json_file import take_command
from yuki_plugin_sdk.connection import Binding, Peer, encode, read_frame
from yuki_plugin_sdk.host import Calls

lock = threading.RLock()


def send(frame):
    with lock:
        sys.stdout.buffer.write(encode(frame))
        sys.stdout.buffer.flush()


def record(name, value):
    dest = folder / (name + ".json")
    stage = dest.with_suffix(".tmp")
    stage.write_text(json.dumps(value), encoding="utf-8")
    stage.replace(dest)


binding = Binding.from_json(json.loads(os.environ["YUKI_PLUGIN_BINDING"]))
peer = Peer(binding)
send(peer.request(1, binding.offer()))
raw = read_frame(sys.stdin.buffer)
peer.receive(raw)
rpc = Calls(json.loads(raw)["result"])
record("ready", {"methods": sorted(rpc.allowed), "pid": os.getpid()})


def mailbox():
    while True:
        for item in folder.glob("cmd-*.json"):
            data = take_command(item)
            if data is None:
                continue
            if "reply" in data:
                send({"jsonrpc": "2.0", "id": data["id"], "result": data["reply"]})
            else:
                with lock:
                    send(rpc.request(data["id"], data["method"], data["params"]))
        time.sleep(0.01)


threading.Thread(target=mailbox, daemon=True).start()
while raw := read_frame(sys.stdin.buffer):
    f = json.loads(raw)
    if f.get("method") == "tool.call":
        record("call-" + f["params"]["handle"], f)
    elif f.get("method") == "forget.request":
        send(
            {
                "jsonrpc": "2.0",
                "id": f["id"],
                "result": {
                    "key": f["params"]["key"],
                    "watermark": f["params"]["watermark"],
                    "proof": "forgotten",
                },
            }
        )
    elif "id" in f and "method" not in f:
        with lock:
            try:
                rpc.receive(f)
            except (ValueError, RuntimeError):
                pass
        record("out-" + str(f["id"]), f)
