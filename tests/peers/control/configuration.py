"""Independent installed plugin: its files record real starts, never core DB access."""
import hashlib
import json
import os
import sys
import time
from pathlib import Path

folder = Path(sys.argv[1])
folder.mkdir(parents=True, exist_ok=True)
with (folder / 'starts.jsonl').open('a', encoding='utf-8') as f:
    f.write(json.dumps({'pid': os.getpid()}) + '\n')
if (folder / 'fail-handshake').exists():
    sys.exit(5)
sys.path.insert(0, sys.argv[2])
from yuki_plugin_sdk.connection import Binding, Peer, encode, read_frame
from yuki_plugin_sdk.host import Calls

def send(value):
    sys.stdout.buffer.write(encode(value))
    sys.stdout.buffer.flush()

def frame():
    value = read_frame(sys.stdin.buffer)
    return json.loads(value) if value else None

binding = Binding.from_json(json.loads(os.environ['YUKI_PLUGIN_BINDING']))
peer = Peer(binding)
send(peer.request(1, binding.offer()))
initialized = frame()
peer.receive(json.dumps(initialized).encode())
(folder / 'negotiated.json').write_text(json.dumps(initialized['result']), encoding='utf-8')
calls = Calls(initialized['result'])
send(calls.request('endpoint', 'plugin.config.get', {'key': 'endpoint'}))
endpoint = None
while (message := frame()) is not None:
    kind, method, value = calls.receive(message)
    if kind == 'response':
        if method == 'plugin.config.get':
            endpoint = value.get('result', {}).get('value')
            if not endpoint or endpoint == 'fail-start':
                sys.exit(4)
            send(calls.request('secret', 'plugin.secrets.get', {'key': 'api_key'}))
        else:
            secret = value.get('result', {}).get('value')
            (folder / 'ready.json').write_text(json.dumps({'endpoint': endpoint, 'secret_sha256': hashlib.sha256((secret or '').encode()).hexdigest()}), encoding='utf-8')
    elif method == 'plugin.observe':
        (folder / 'observing').write_text('entered', encoding='utf-8')
        while (folder / 'hold').exists():
            time.sleep(0.01)
        send({'jsonrpc': '2.0', 'id': message['id'], 'result': {'endpoint': endpoint, 'pid': os.getpid()}})
