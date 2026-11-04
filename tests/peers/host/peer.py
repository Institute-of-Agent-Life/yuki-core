"""Synthetic installed plugin. It owns its own files and never reads the core database."""

import hashlib
import json
import os
import sys
import threading
import time
from pathlib import Path

folder = Path(sys.argv[1])
folder.mkdir(parents=True, exist_ok=True)
sys.path.insert(0, sys.argv[2])
from yuki_plugin_sdk.connection import Binding, Peer, encode, read_frame  # noqa: E402
from yuki_plugin_sdk.host import Calls  # noqa: E402

mutex = threading.Lock()


def log(kind, **kwargs):
    with mutex, (folder / "observed.jsonl").open("a", encoding="utf-8") as f:
        f.write(
            json.dumps({"pid": os.getpid(), "kind": kind, **kwargs}, ensure_ascii=False)
            + "\n"
        )


def send(frame):
    with mutex:
        sys.stdout.buffer.write(encode(frame))
        sys.stdout.buffer.flush()


def answer(id, body):
    send({"jsonrpc": "2.0", "id": id, "result": body})


binding = Binding.from_json(json.loads(os.environ["YUKI_PLUGIN_BINDING"]))
peer = Peer(binding)
send(peer.request(1, binding.offer()))
if not (raw := read_frame(sys.stdin.buffer)):
    sys.exit(3)
init = json.loads(raw)
peer.receive(raw)
caps = Calls(init["result"])
log(
    "ready",
    methods=sorted(caps.allowed),
    parent_secret_present="YUKI_PARENT_SENTINEL" in os.environ,
    allowlisted=os.environ.get("YUKI_HOST_TEST_PUBLIC"),
)


def inputs():
    while True:
        for p in sorted(folder.glob("rpc-*.json")):
            try:
                frame = json.loads(p.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                continue
            try:
                p.unlink()
            except FileNotFoundError:
                continue
            send(frame)
        time.sleep(0.01)


threading.Thread(target=inputs, daemon=True).start()
pending = {}
while (raw := read_frame(sys.stdin.buffer)) is not None:
    frame = json.loads(raw)
    method = frame.get("method")
    if not method:
        log("reply", frame=frame)
        if frame["id"] in pending:
            caps.receive(frame)
            id, params = pending.pop(frame["id"])
            if (folder / "hold-tool").exists():
                log("holding", handle=params["handle"])
                while (folder / "hold-tool").exists():
                    time.sleep(0.01)
            answer(
                id,
                {
                    "ok": True,
                    "data": {
                        "original": params["arguments"],
                        "status": frame.get("result"),
                    },
                },
            )
        continue
    kind, _, p = caps.receive(frame)
    log("method", method=method, params=p)
    if method == "tool.call":
        # Use the identical ID while an incoming call is outstanding. Request and response directions are independent.
        id = frame["id"]
        pending[id] = (id, p)
        send(caps.request(id, "tool.status", {"handle": p["handle"]}))
    elif method == "command.invoke":
        answer(frame["id"], {"ok": True, "data": {"handled": p["name"]}})
    elif method == "gateway.plan_send":
        parts = [{"text": v} for v in p["arguments"]["message"]["text"].split("|")]
        answer(frame["id"], {"parts": parts})
    elif method == "gateway.dispatch_part":
        n = p["arguments"]["part_ordinal"]
        with (folder / "deliveries.jsonl").open("a", encoding="utf-8") as f:
            f.write(json.dumps(p, ensure_ascii=False) + "\n")
        if n == 1 and (folder / "cut-second").exists():
            os._exit(23)
        answer(
            frame["id"], {"state": "confirmed", "locator": "native-" + p["operation"]}
        )
    elif method == "observer.event":
        while (folder / "stall-observer").exists():
            time.sleep(0.02)
    elif method == "forget.request":
        acquired = folder / "private.item"
        acquired.unlink(missing_ok=True)
        proof = hashlib.sha256(
            (p["key"] + "/" + str(p["watermark"])).encode()
        ).hexdigest()
        receipt = {**{k: p[k] for k in ("key", "watermark")}, "proof": proof}
        (folder / "erased.json").write_text(json.dumps(receipt), encoding="utf-8")
        if (folder / "drop-forget").exists():
            os._exit(24)
        answer(frame["id"], receipt)
