"""Exercise a visible Windows response file while a non-sharing handle is open."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(os.name == "nt", "requires Windows file sharing")
class PersonalReplyTest(unittest.TestCase):
    def test_locked_reply_does_not_resend_command(self):
        import _winapi

        sys.path.insert(0, str(ROOT))
        try:
            spec = importlib.util.spec_from_file_location("personal_case", ROOT / "tests/cases/CORE-033.py")
            case = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(case)
        finally:
            sys.path.remove(str(ROOT))

        class Owner:
            def __init__(owner, directory):
                owner.folder = directory
                owner.ctx = owner
                owner.handle = None

            def equal(owner, name, actual, expected):
                self.assertEqual(actual, expected, name)

            def until(owner, check):
                # The peer has received exactly one published request and produced its reply.
                commands = list(owner.folder.glob("cmd-*.json"))
                self.assertEqual(len(commands), 1)
                request = json.loads(commands[0].read_bytes())
                path = owner.folder / f"out-{request['id']}.json"
                path.write_text(json.dumps({"id": request["id"], "result": "original"}))
                owner.handle = _winapi.CreateFile(str(path), 0x80000000, 0, 0, 3, 0, 0)
                first = check()
                if first:
                    return first
                _winapi.CloseHandle(owner.handle)
                owner.handle = None
                return check()

        with tempfile.TemporaryDirectory() as name:
            owner = Owner(Path(name))
            try:
                self.assertEqual(case.Plugin(owner, owner.folder).rpc("device.check", {}), "original")
                self.assertEqual([f.name for f in owner.folder.iterdir()], ["cmd-11.json"])
            finally:
                if owner.handle is not None:
                    _winapi.CloseHandle(owner.handle)
