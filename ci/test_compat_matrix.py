"""Partial or changed public assets never become an accepted compatibility input."""

import hashlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from compat_matrix import asset_path, prepare_asset


class ConsumerInputs(unittest.TestCase):
    def test_verified_cache_and_partial_download(self):
        data = b"fixed original package"
        asset = {
            "name": "package.zip",
            "size": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
            "url": "https://github.com/Institute-of-Agent-Life/yuki-core/releases/download/v1/package.zip",
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = asset_path(root, asset)
            with patch("compat_matrix.urlopen", return_value=io.BytesIO(data[:-1])):
                with self.assertRaises(ValueError):
                    prepare_asset(root, asset)
            self.assertFalse(path.exists())
            self.assertEqual(list(path.parent.iterdir()), [])
            with patch("compat_matrix.urlopen", return_value=io.BytesIO(data)):
                self.assertEqual(prepare_asset(root, asset), path)
            with patch(
                "compat_matrix.urlopen",
                side_effect=AssertionError("cache redownloaded"),
            ):
                self.assertEqual(prepare_asset(root, asset), path)
                path.write_bytes(b"changed")
                with self.assertRaises(ValueError):
                    prepare_asset(root, asset)

    def test_mutable_branch_and_untrusted_downloads_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            asset = {"name": "package.zip", "size": 1, "sha256": "0" * 64}
            for url in [
                "http://github.com/Institute-of-Agent-Life/yuki-core/releases/download/v1/package.zip",
                "https://raw.githubusercontent.com/Institute-of-Agent-Life/yuki-extensions/main/package.zip",
                "https://example.test/package.zip",
            ]:
                with self.subTest(url=url), self.assertRaises(ValueError):
                    prepare_asset(Path(temporary), {**asset, "url": url})


if __name__ == "__main__":
    unittest.main()
