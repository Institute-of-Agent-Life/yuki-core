"""An SDK peer must consume one original command after Windows releases its handle."""
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(os.name == "nt", "requires Windows file sharing")
class TerminalMailboxTest(unittest.TestCase):
    def test_original_status_survives_delete_denial(self):
        import _winapi
        sys.path.insert(0, str(ROOT / "sdk/python"))
        try:
            from yuki_plugin_sdk.connection import Binding, Peer, encode
            from yuki_plugin_sdk.host import SCHEMA
        finally:
            sys.path.pop(0)

        binding = {"namespace": "manager", "grants": ["tool.call"],
                   "declarations": [], "manifest_revision": "revision-1"}
        host = Peer(Binding.from_json(binding))
        with tempfile.TemporaryDirectory() as name:
            folder = Path(name)
            command = folder / "cmd-18.json"
            request = {"id": 18, "method": "tool.status", "params": {"handle": "original"}}
            command.write_text(json.dumps(request))
            # The writer permits readers, but Windows will refuse unlink until close.
            handle = _winapi.CreateFile(str(command), 0x80000000, 1, 0, 3, 0, 0)
            with subprocess.Popen(
                [sys.executable, str(ROOT / "tests/peers/terminal/consumer.py"),
                 name, str(ROOT / "sdk/python")],
                env=dict(os.environ, YUKI_PLUGIN_BINDING=json.dumps(binding)),
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            ) as peer:
                frames = queue.Queue()

                def read_wire():
                    for line in peer.stdout:
                        frames.put(line)

                reader = threading.Thread(target=read_wire, daemon=True)
                reader.start()
                try:
                    initialized = host.receive(frames.get(timeout=5)).reply
                    result = initialized["result"]
                    result["method_revisions"].append({"name": "tool.status",
                        "revision": SCHEMA["methods"]["tool.status"]["revision"]})
                    result["host_schema_revision"] = SCHEMA["schema_revision"]
                    peer.stdin.write(encode(initialized))
                    peer.stdin.flush()
                    deadline = time.monotonic() + 5
                    while not (folder / "ready.json").exists():
                        self.assertLess(time.monotonic(), deadline)
                        time.sleep(0.01)
                    with self.assertRaises(queue.Empty):
                        frames.get(timeout=0.25)
                    _winapi.CloseHandle(handle)
                    handle = None
                    frame = json.loads(frames.get(timeout=3))
                    self.assertEqual(frame, dict(request, jsonrpc="2.0"))
                    self.assertFalse(command.exists())
                    with self.assertRaises(queue.Empty):
                        frames.get(timeout=0.1)
                finally:
                    if handle is not None:
                        _winapi.CloseHandle(handle)
                    peer.stdin.close()
                    peer.wait(timeout=5)
                    reader.join(timeout=2)
                    error = peer.stderr.read().decode(errors="replace")
                    self.assertEqual(error, "", error)
