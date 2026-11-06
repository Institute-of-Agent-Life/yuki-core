"""A failed or omitted card cannot turn a bounded parallel suite green."""
import json
from pathlib import Path
from tempfile import TemporaryDirectory
import threading
import unittest
from unittest.mock import patch

import suite


class Scheduling(unittest.TestCase):
    def test_coverage_failure_and_capacity(self):
        cards = {f'CORE-{n:03}': {} for n in range(1, 8)}
        for failing in (False, True):
            with self.subTest(failing=failing), TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / 'ci').mkdir()
                (root / 'ci/checks.json').write_text(json.dumps({'cards': cards}))
                first = threading.Barrier(2, timeout=5)
                called = []
                def check(args, prepared):
                    self.assertIsNone(prepared)
                    if args.card in ('CORE-001', 'CORE-002'):
                        first.wait()
                    called.append(args.card)
                    return int(failing and args.card == 'CORE-001')
                with patch.object(suite, 'ROOT', root), patch.object(suite, 'run_card', check):
                    code = suite.run(root / 'out')
                summary = json.loads((root / 'out/suite.json').read_text())
                self.assertEqual(len(called), len(set(called)))
                self.assertEqual(summary['verified'], not failing)
                self.assertEqual(set(called) | set(summary['not_finished']), set(cards))
                if failing:
                    self.assertNotEqual(code, 0)
                    self.assertEqual(summary['failed'], ['CORE-001'])
                else:
                    self.assertEqual(code, 0)
                    self.assertEqual(set(called), set(cards))
