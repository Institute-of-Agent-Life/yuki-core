"""A standalone stdio peer for testing against an installed SDK."""

import argparse
import json
from pathlib import Path
import sys
from .connection import Binding, Peer, encode, read_frame


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binding", type=Path)
    parser.add_argument("--initiate", action="store_true")
    args = parser.parse_args()
    binding = (
        Binding.from_json(json.loads(args.binding.read_bytes()))
        if args.binding
        else Binding("fixture.plugin", (), "fixture-r1", ())
    )
    peer = Peer(binding)
    output = sys.stdout.buffer
    if args.initiate:
        output.write(encode(peer.request(1, binding.offer())))
        output.flush()
    while (raw := read_frame(sys.stdin.buffer)) is not None:
        msg = peer.receive(raw)
        if msg.reply is not None:
            output.write(encode(msg.reply))
            output.flush()
        elif msg.error is not None:
            raise ValueError("initialize rejected")
    peer.finish()


if __name__ == "__main__":
    main()
