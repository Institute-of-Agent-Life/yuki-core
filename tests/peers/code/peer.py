"""Installed-SDK opaque snapshot fixture. This process has no real VM or core DB access."""

import base64
import hashlib
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
folder.mkdir(parents=True, exist_ok=True)
sys.path.insert(0, sys.argv[2])
from yuki_plugin_sdk.connection import Binding, Peer, encode, read_frame
from yuki_plugin_sdk.host import Calls

CODEC = "2" * 64
lock = threading.RLock()
pending = {}
erased = set()
sequence = 20


def send(frame):
    with lock:
        sys.stdout.buffer.write(encode(frame))
        sys.stdout.buffer.flush()


def note(name, value):
    file = folder / (name + ".tmp")
    file.write_text(json.dumps(value, ensure_ascii=False), encoding="utf-8")
    file.replace(folder / (name + ".json"))


threading.excepthook = lambda failure: note("private-thread-fault", {
    "type":failure.exc_type.__name__, "message":str(failure.exc_value),
    "thread":failure.thread.name if failure.thread else None,
})


def rpc_call(method, params, accepted=True):
    global sequence
    with lock:
        sequence += 1
        request_id = sequence
        answer = pending[request_id] = queue.Queue()
        send(rpc.request(request_id, method, params))
    reply = answer.get(timeout=10)
    if accepted and "error" in reply:
        raise RuntimeError(method + " rejected " + json.dumps(reply))
    return reply.get("result") if accepted else reply


binding = Binding.from_json(json.loads(os.environ["YUKI_PLUGIN_BINDING"]))
peer = Peer(binding)
send(peer.request(1, binding.offer()))
raw = read_frame(sys.stdin.buffer)
initialized = json.loads(raw)
peer.receive(raw)
rpc = Calls(initialized["result"])
note("ready", {"pid": os.getpid(), "methods": sorted(rpc.allowed)})


def upload(handle, state, name):
    raw = json.dumps({**state, "padding": "opaque fixture bytes." * 4000}, ensure_ascii=False).encode()
    digest = hashlib.sha256(raw).hexdigest()
    for offset in range(0, len(raw), 16384):
        params = {"handle": handle, "snapshot": name, "sha256": digest, "codec": CODEC, "size": len(raw), "offset": offset, "bytes": base64.b64encode(raw[offset:offset+16384]).decode()}
        reply = rpc_call("code.snapshot.put", params)
        assert reply["next_offset"] >= offset + min(16384, len(raw)-offset)
    return name


def restore(handle, snapshot):
    data = bytearray()
    while True:
        result = rpc_call("code.snapshot.get", {"handle": handle, "snapshot": snapshot, "offset": len(data)})
        data.extend(base64.b64decode(result["bytes"], validate=True))
        if len(data) == result["size"]:
            assert hashlib.sha256(data).hexdigest() == result["sha256"]
            return json.loads(data)


def retain_sources(handle):
    cursor=0
    sources=[]
    while True:
        result=rpc_call("code.sources", {"handle":handle,"cursor":cursor,"limit":2})
        sources.extend(result["sources"])
        if result["next_cursor"] is None:
            break
        cursor=result["next_cursor"]
    note("private-sources-"+handle,{"handle":handle,"sources":sources})


def serve(frame):
    p = frame["params"]
    handle = p.get("handle")
    try:
        if frame["method"] == "forget.request":
            for source_map in folder.glob("private-sources-*.json"):
                info=json.loads(source_map.read_bytes())
                if set(info["sources"]) & set(p["sources"]):
                    erased.add(info["handle"])
                    for file in folder.glob("private-*"+info["handle"]+"*.json"):
                        file.unlink()
            result = {"key": p["key"], "watermark": p["watermark"], "proof": "fixture private files removed"}
        else:
            if frame["method"] == "code.resume":
                state = restore(handle, p["snapshot"])
                checkpoint = p["checkpoint"]
                children = p["children"]
                note("resumed-" + handle, {"snapshot": p["snapshot"], "checkpoint": checkpoint, "next": state["next"]})
            else:
                assert frame["method"] == "tool.call"
                marker = folder / ("started-" + handle + ".json")
                assert not marker.exists(), "original script must never be started twice"
                note("started-" + handle, {"operation": p["operation"]})
                state = {"next": 0, "steps": p["arguments"]["steps"], "seen": []}
                checkpoint = 0
                children = []
            retain_sources(handle)
            catalog = rpc_call("tool.catalog", {"handle": handle, "limit": 2})
            schema = rpc_call("tool.schema", {"handle": handle, "name": "read_tool_artifact"})
            assert "parameters" in schema and all("parameters" not in i for i in catalog["items"])
            while state["next"] < len(state["steps"]):
                index = state["next"]
                step = state["steps"][index]
                call_id = "engine-" + str(index)
                original = next((c for c in children if c["ordinal"] == index), None)
                args = step["arguments"]
                if step.get("previous_artifact"):
                    artifact = state["seen"][-1]["receipt"]["result"]["artifact"]
                    args = {"handle": artifact["handle"], "operation": "get", "path": ["value", "value"], "offset": 0, "limit": 128}
                if original is not None:
                    child = rpc_call("tool.resume", {"handle": handle, "ordinal": index, "feed_index": original["feed_index"], "engine_call_id": original["engine_call_id"]})
                else:
                    snap = upload(handle, state, "before-" + str(index))
                    child_request = {"handle": handle, "ordinal": index, "feed_index": step.get("feed_index", index), "engine_call_id": call_id, "checkpoint": checkpoint, "snapshot": snap, "name": step["name"], "arguments": args}
                    if step.get("expect_denied"):
                        refused = rpc_call("tool.invoke", child_request, False)
                        assert "error" in refused
                        state["seen"].append({"denied": True})
                        break
                    child = rpc_call("tool.invoke", child_request)
                    repeated = rpc_call("tool.invoke", child_request)
                    assert child == repeated, "same original child must replay exactly"
                if handle not in erased and not child.get("erased"):
                    retain_sources(handle)
                    note("private-child-" + handle + "-" + str(index), child)
                if not child.get("can_feed"):
                    result = {"ok": True, "data": {"status": "partial", "observed": child}}
                    break
                state["seen"].append(child)
                state["next"] += 1
                after = upload(handle, state, "after-" + str(index))
                after_feed = step.get("after_feed", index+1)
                ack = {"handle": handle, "ordinal": index, "feed_index": child["feed_index"], "engine_call_id": child["engine_call_id"], "ack": {"checkpoint": child["checkpoint"], "snapshot": after, "feed_index": after_feed}}
                if child["feed_index"] > 0:
                    regressed = {**ack, "ack": {**ack["ack"], "feed_index": child["feed_index"]-1}}
                    assert "error" in rpc_call("tool.resume", regressed, False)
                fed = rpc_call("tool.resume", ack)
                assert rpc_call("tool.resume", ack) == fed
                changed_ack = {**ack, "ack": {**ack["ack"], "feed_index": after_feed+1}}
                assert "error" in rpc_call("tool.resume", changed_ack, False)
                checkpoint = fed["checkpoint"]
            else:
                result = {"ok": True, "data": {"status": "completed", "children": len(state["seen"])}}
            if "result" not in locals():
                result = {"ok": True, "data": {"status": "denied_child", "seen": state["seen"]}}
            if handle not in erased:
                note("private-final-" + handle, result)
        send({"jsonrpc": "2.0", "id": frame["id"], "result": result})
    except Exception as error:
        note("private-fault", {"type": type(error).__name__, "message": str(error)})
        send({"jsonrpc": "2.0", "id": frame["id"], "error": {"code": -32020, "message": "opaque conformance fixture failed"}})


def probes():
    while True:
        path=folder/'probe.json'
        if path.is_file():
            request=read_json(path)
            path.unlink()
            result=rpc_call(request['method'],request['params'],False)
            note('probe-'+str(request['id']),result)
        time.sleep(0.01)

threading.Thread(target=probes,daemon=True).start()

while raw := read_frame(sys.stdin.buffer):
    frame = json.loads(raw)
    if "method" in frame:
        rpc.receive(frame)
        threading.Thread(target=serve, args=(frame,), daemon=True).start()
    else:
        with lock:
            rpc.receive(frame)
            answer = pending.pop(frame["id"])
            answer.put(frame)
