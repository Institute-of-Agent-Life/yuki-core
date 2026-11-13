"""Holds a real command while tests exercise original-person authorization."""
import json
import os
import sys
import threading
import time
from pathlib import Path

folder = Path(sys.argv[1]); folder.mkdir(parents=True, exist_ok=True)
sys.path.insert(0, sys.argv[2])
from yuki_plugin_sdk.connection import Binding, Peer, encode, read_frame
from yuki_plugin_sdk.host import Calls
lock = threading.RLock()
def send(value):
    with lock:
        sys.stdout.buffer.write(encode(value)); sys.stdout.buffer.flush()
binding = Binding.from_json(json.loads(os.environ['YUKI_PLUGIN_BINDING']))
peer = Peer(binding); send(peer.request(1, binding.offer()))
raw = read_frame(sys.stdin.buffer); initialized = json.loads(raw); peer.receive(raw)
calls = Calls(initialized['result'])
(folder / 'ready').touch()
def inputs():
    while True:
        reply = folder / 'reply.json'
        if reply.is_file():
            try:
                frame = json.loads(reply.read_text())
                reply.unlink()
            except (OSError, ValueError):
                pass
            else:
                send(frame)
        for path in folder.glob('request-*.json'):
            try:
                data = json.loads(path.read_text()); path.unlink()
            except (OSError,ValueError):
                continue
            # Deliberately includes malformed adversarial requests; the core must deny them.
            with lock:
                send(calls.request(data["id"], data["method"], data["params"]))
        time.sleep(0.01)
threading.Thread(target=inputs,daemon=True).start()
while (raw := read_frame(sys.stdin.buffer)) is not None:
    message = json.loads(raw)
    if message.get('method'):
        kind, method, args = calls.receive(message)
        if method == 'command.invoke':
            (folder / 'command.json').write_text(json.dumps({'id': message['id'], 'params': args}))
    else:
        with lock:
            calls.receive(message)
        (folder / ('response-' + str(message['id']) + '.json')).write_text(json.dumps(message))
