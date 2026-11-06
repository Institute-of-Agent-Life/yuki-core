"""Run the original v4 automatic compaction scenarios in the isolated oracle."""

import json
import os
from pathlib import Path

import pytest
from tests.integration import test_compact_delivery_e2e as original

pytest_plugins = ["tests.conftest"]


@pytest.mark.parametrize("scenario", ["finish", "p_refused"])
async def test_automatic_continuation(database, tmp_path, monkeypatch, scenario):
    out = Path(os.environ["YUKI_SLICE_OUT"]) / scenario
    out.mkdir()
    evidence = {"requests": []}
    try:
        await original.test_real_ordinary_send_keeps_receipt_and_replaces_public_prefix(
            database, tmp_path, monkeypatch, (out, evidence), "unknown", scenario
        )
    finally:
        (out / "original.json").write_text(
            json.dumps(evidence, default=str, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
