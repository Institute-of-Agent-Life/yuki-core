"""Exercise the installed SDK and ask a running base Host for a preview-only RPC."""

import json
import os
from pathlib import Path
import sys

installation, output, requirement = sys.argv[1:4]
sys.path.insert(0, installation)
from yuki_plugin_sdk.connection import Binding, Peer, encode, read_frame

binding = Binding.from_json(json.loads(os.environ["YUKI_PLUGIN_BINDING"]))
offer = binding.offer()
if requirement != "initialize":
    offer.required_features.append(requirement)
peer = Peer(binding)
request = peer.request(1, offer)
sys.stdout.buffer.write(encode(request))
sys.stdout.buffer.flush()
response = json.loads(read_frame(sys.stdin.buffer))
record = {"initialize": response}
if "result" in response:
    sys.stdout.buffer.write(
        encode({"jsonrpc": "2.0", "id": 2, "method": "gateway.status", "params": {}})
    )
    sys.stdout.buffer.flush()
    record["preview"] = json.loads(read_frame(sys.stdin.buffer))
    if sys.argv[4:] == ["original-inbound"]:
        sys.stdout.buffer.write(
            encode(
                {
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "gateway.inbound",
                    "params": {
                        "source": "original-profile-event",
                        "native_id": "message-1",
                        "sender": "human-1",
                        "space": "group-1",
                        "topic": "topic-1",
                        "occurred": 1,
                        "kind": "data",
                        "segments": [],
                        "accept": False,
                    },
                }
            )
        )
        sys.stdout.buffer.flush()
        record["inbound"] = json.loads(read_frame(sys.stdin.buffer))
path = Path(output)
tmp = path.with_suffix(".tmp")
tmp.write_text(json.dumps(record), encoding="utf-8")
tmp.replace(path)
if "result" in response:
    sys.stdin.buffer.read()
