import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from unittest.mock import patch

import local_matrix
from local_matrix import verify_suite
from source import digest


class LocalMatrix(unittest.TestCase):
    def test_stacked_pr_is_bound_to_the_current_base_and_merge(self):
        repo = {'full_name': local_matrix.REPO}
        pr = {'state': 'open', 'head': {'repo': repo, 'sha': 'head'},
              'base': {'repo': repo, 'ref': 'main', 'sha': 'previous-base'},
              'merge_commit_sha': 'merge'}
        def check():
            local_matrix.verify_pr(96, ['current-base', 'head'], 'merge', True)
        live = {'object': {'sha': 'current-base'}}
        with patch.object(local_matrix, 'api', side_effect=[pr, live]) as api:
            check()
            self.assertEqual([c.args[0] for c in api.call_args_list],
                             ['pulls/96', 'git/ref/heads/main'])
        for bad, ref in [
            (pr, {'object': {'sha': 'newer-base'}}),
            ({**pr, 'head': {**pr['head'], 'sha': 'new-head'}}, live),
            ({**pr, 'merge_commit_sha': 'another-merge'}, live),
            ({**pr, 'state': 'closed'}, live),
            ({**pr, 'base': {**pr['base'], 'ref': 'other'}}, live),
            ({**pr, 'base': {**pr['base'], 'repo': {'full_name': 'elsewhere'}}}, live),
            ({**pr, 'head': {**pr['head'], 'repo': {'full_name': 'elsewhere'}}}, live),
        ]:
            with self.subTest(pr=bad, ref=ref), patch.object(local_matrix, 'api',
                    side_effect=[bad, ref]), self.assertRaises(ValueError):
                check()

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
