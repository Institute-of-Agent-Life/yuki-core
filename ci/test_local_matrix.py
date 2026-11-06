import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from local_matrix import verify_suite
from source import digest


class LocalMatrix(unittest.TestCase):
    def test_mismatched_evidence_is_rejected(self):
        registry = b'{"cards":{"CORE-001":{"artifact":"proof.json"}}}'
        source = dict(commit='merge', dirty=False, tree_sha256='tree', files={'file': 'hash'})
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            folder = root / 'CORE-001/ci'
            folder.mkdir(parents=True)
            (root / 'suite.json').write_text(json.dumps(dict(verified=True, registered=1, failed=[],
                not_finished=[], seconds=1, cards={'CORE-001': {'exit_code': 0}})))
            artifact = folder / 'proof.json'
            artifact.write_bytes(b'{}')
            manifest = dict(card='CORE-001', platform='Linux-aarch64', source=source,
                lock_sha256=digest(registry), exit_code=0, status='verified', failure=None,
                assertions=[{'passed': True}], artifacts={'proof.json': digest(b'{}')})
            (folder / 'manifest.json').write_text(json.dumps(manifest))
            self.assertEqual(verify_suite(root, 'linux', source, registry)['cards'], 1)
            for platform, expected in [('macos', source), ('linux', dict(source, commit='other'))]:
                with self.assertRaises(ValueError):
                    verify_suite(root, platform, expected, registry)
            artifact.write_bytes(b'changed')
            with self.assertRaises(ValueError):
                verify_suite(root, 'linux', source, registry)
