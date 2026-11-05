"""Controlled wire4 daemon and adapter. Only the core exchanges credentials and commits events."""

import base64
from contextlib import closing
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import socket
import struct
import threading
from tests.peers.onebot.peer import Peer


class Daemon(Peer):
    def __init__(self, address, token):
        self.socket = socket.create_connection(address, timeout=8)
        nonce = base64.b64encode(os.urandom(16)).decode()
        req = (
            f"GET /agents/v1/connect HTTP/1.1\r\nHost: {address[0]}:{address[1]}\r\n"
            f"Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {nonce}\r\nAuthorization: Bearer {token}\r\n\r\n"
        )
        self.socket.sendall(req.encode())
        self.reader = self.socket.makefile("rb")
        self.status = int(self.reader.readline().split()[1])
        while self.reader.readline() not in (b"\r\n", b""):
            pass


def exact(reader, n):
    b = reader.read(n)
    if len(b) != n:
        raise EOFError
    return b


def frame(reader):
    h = exact(reader, 2)
    if h[0] == 0x88:
        raise EOFError
    if h[0] != 0x81 or not h[1] & 128:
        raise ValueError("masked text frame required")
    n = h[1] & 127
    if n == 126:
        n = struct.unpack("!H", exact(reader, 2))[0]
    elif n == 127:
        n = struct.unpack("!Q", exact(reader, 8))[0]
    if n > 65536:
        raise ValueError("oversized fixture frame")
    mask = exact(reader, 4)
    data = exact(reader, n)
    return json.loads(bytes(x ^ mask[i % 4] for i, x in enumerate(data)))


def exercise(ctx, wire, owner, plugin, work, connector, chat):
    golden = json.loads(
        (ctx.root / "protocol/baseline-v4/wire4-frames.json").read_bytes()
    )
    hello = dict(golden[0]["body"], device_id=connector["device_id"])
    event = {
        **golden[1]["body"],
        "event_id": "fixture-wire4",
        "payload": {"text": "private wire4 progress"},
    }
    observed = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            status, issued = wire(
                owner.svc.address,
                "/core/v1/entry/exchange",
                self.headers["Authorization"].removeprefix("Bearer "),
                {"purpose": "connector", "plugin": "personal-peer", "work": work},
            )
            if status != 200:
                self.send_error(status)
                return
            h = plugin.rpc("device.auth", {"proof": issued["data"]["proof"]})["handle"]
            self.send_response(101)
            self.send_header("Connection", "Upgrade")
            self.send_header("Upgrade", "websocket")
            self.send_header(
                "Sec-WebSocket-Accept",
                base64.b64encode(
                    hashlib.sha1(
                        (
                            self.headers["Sec-WebSocket-Key"]
                            + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
                        ).encode()
                    ).digest()
                ).decode(),
            )
            self.end_headers()
            try:
                first = frame(self.rfile)
                if first != hello:
                    return
                msg = frame(self.rfile)
                if msg["type"] != "event":
                    return
                accepted = plugin.rpc(
                    "native.notify",
                    {
                        "handle": h,
                        "operation": "original-op",
                        "event_key": msg["event_id"],
                        "payload": msg["payload"],
                    },
                )
                observed.append(
                    {
                        "accepted": accepted["accepted"],
                        "event": accepted["event"],
                        "watermark": accepted["watermark"],
                        "replay": accepted["replay"],
                    }
                )
                if len(observed) == 1:
                    return  # lose ACK after the original transaction
                ack = json.dumps({"type": "ack", "event_id": msg["event_id"]}).encode()
                self.wfile.write(bytes([0x81, len(ack)]) + ack)
                self.wfile.flush()
            except (EOFError, ConnectionError):
                return

    with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
        t = threading.Thread(target=server.serve_forever, daemon=True)
        t.start()
        try:
            with closing(Daemon(server.server_address, chat["token"])) as denied:
                ctx.equal(
                    "chat bearer rejected before wire4 upgrade", denied.status, 401
                )
            for replay in (False, True):
                with closing(Daemon(server.server_address, connector["token"])) as peer:
                    ctx.equal(
                        "original connector exchanges credential before wire4 upgrade",
                        peer.status,
                        101,
                    )
                    peer.send(hello)
                    peer.send(event)
                    ack = peer.receive()
                    ctx.equal(
                        "actual wire4 ACK after commit or injected loss",
                        ack,
                        (
                            {"type": "ack", "event_id": event["event_id"]}
                            if replay
                            else None
                        ),
                    )
        finally:
            server.shutdown()
            t.join()
    ctx.check(
        "two physical connector sessions recover one committed event",
        len(observed) == 2
        and observed[0]["event"] == observed[1]["event"]
        and observed[1]["replay"],
    )
    return observed
