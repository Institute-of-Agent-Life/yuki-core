"""Explicit cleanup-only and business entrypoints of one real SDK consumer."""

import hashlib, json, os, sys
from pathlib import Path

purpose, folder, sdk = sys.argv[1:]
folder = Path(folder)
sys.path.insert(0, sdk)
from yuki_plugin_sdk.connection import Binding, Peer, encode, read_frame
from yuki_plugin_sdk.host import Calls


def note(event, **extra):
    with (folder / "phases.jsonl").open("a", encoding="utf-8") as f:
        f.write(
            json.dumps(
                {
                    "event": event,
                    "mode": os.environ["YUKI_PLUGIN_START_MODE"],
                    "has_private": (folder / "private.item").is_file(),
                    **extra,
                }
            )
            + "\n"
        )


def send(f):
    sys.stdout.buffer.write(encode(f))
    sys.stdout.buffer.flush()


note("start")
if purpose == "normal" and (folder / "private.item").exists():
    sys.exit(51)
binding = Binding.from_json(json.loads(os.environ["YUKI_PLUGIN_BINDING"]))
peer = Peer(binding)
send(peer.request(1, binding.offer()))
raw = read_frame(sys.stdin.buffer)
r = json.loads(raw)
peer.receive(raw)
methods = Calls(r["result"]).allowed
note("ready", methods=sorted(methods))
while raw := read_frame(sys.stdin.buffer):
    frame = json.loads(raw)
    if frame.get("method") != "forget.request":
        continue
    if purpose != "cleanup":
        sys.exit(52)
    p = frame["params"]
    (folder / "private.item").unlink(missing_ok=True)
    token = hashlib.sha256((p["key"] + str(p["watermark"])).encode()).hexdigest()
    note("erased", key=p["key"], watermark=p["watermark"], proof=token)
    if (folder / "lose-ack").exists():
        (folder / "lose-ack").unlink()
        sys.exit(53)
    send(
        {
            "jsonrpc": "2.0",
            "id": frame["id"],
            "result": {"key": p["key"], "watermark": p["watermark"], "proof": token},
        }
    )
