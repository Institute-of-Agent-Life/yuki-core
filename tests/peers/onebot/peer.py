"""Small RFC 6455 universal reverse peer used only by the offline test suite."""

import base64
import hashlib
import json
import os
import socket
import struct


class Peer:
    def __init__(self, address, bot, key, *, claimed=None):
        host, port = address.rsplit(":", 1)
        self.socket = socket.create_connection((host, int(port)), timeout=5)
        nonce = base64.b64encode(os.urandom(16)).decode()
        headers = [
            f"GET /onebot/{bot} HTTP/1.1",
            f"Host: {address}",
            "Upgrade: websocket",
            "Connection: Upgrade",
            "Sec-WebSocket-Version: 13",
            "Sec-WebSocket-Key: " + nonce,
            "X-Client-Role: Universal",
            "X-Self-ID: " + (claimed or bot),
            "Authorization: Bearer " + key,
            "",
            "",
        ]
        self.socket.sendall("\r\n".join(headers).encode())
        self.reader = self.socket.makefile("rb")
        status = self.reader.readline().decode()
        received = {}
        while (line := self.reader.readline()) not in (b"", b"\r\n"):
            name, value = line.decode().strip().split(":", 1)
            received[name.lower()] = value.strip()
        expected = base64.b64encode(
            hashlib.sha1(
                (nonce + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()
            ).digest()
        ).decode()
        if " 101 " not in status or received.get("sec-websocket-accept") != expected:
            self.close()
            raise ValueError("websocket handshake rejected: " + status.strip())

    def close(self):
        self.reader.close()
        self.socket.close()

    def frame(self, opcode, data):
        n = len(data)
        length = (
            bytes([128 + n])
            if n < 126
            else b"\xfe" + struct.pack("!H", n)
            if n < 65536
            else b"\xff" + struct.pack("!Q", n)
        )
        mask = os.urandom(4)
        self.socket.sendall(
            bytes([128 + opcode])
            + length
            + mask
            + bytes(x ^ mask[i % 4] for i, x in enumerate(data))
        )

    def send(self, body):
        raw = json.dumps(body).encode()
        self.frame(1, raw)
        return raw

    def receive(self):
        while True:
            start = self.reader.read(2)
            if len(start) != 2:
                return None
            opcode = start[0] & 15
            n = start[1] & 127
            if n == 126:
                n = struct.unpack("!H", self.reader.read(2))[0]
            if n == 127:
                n = struct.unpack("!Q", self.reader.read(8))[0]
            if n > 1048576:
                raise ValueError("oversized test frame")
            data = self.reader.read(n)
            if opcode == 8:
                return None
            if opcode == 9:
                self.frame(10, data)
            if opcode == 1:
                return json.loads(data)
