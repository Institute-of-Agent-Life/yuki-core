"""Verify original receipt bytes over a fresh pipe, including native Windows SSH sessions."""

import subprocess
import tempfile
import unittest
from pathlib import Path

from verify_dist import NAMESPACE, verify_signature


class CandidateSignatureTest(unittest.TestCase):
    def test_original_signature_and_modified_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            key = root / "signer"
            data = root / "receipt"
            data.write_bytes(b'{"source":"original"}\n' * 4096)
            subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)],
                           check=True, timeout=10)
            subprocess.run(["ssh-keygen", "-q", "-Y", "sign", "-f", str(key),
                            "-n", NAMESPACE, str(data)], check=True, timeout=10)
            signature = data.with_suffix(".sig").read_bytes()
            public = key.with_suffix(".pub")
            verify_signature(data.read_bytes(), signature, public)
            with self.assertRaisesRegex(ValueError, "signature differs"):
                verify_signature(data.read_bytes() + b"changed", signature, public)
