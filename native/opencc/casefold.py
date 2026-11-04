"""Emit the full Unicode 15.0 case folding used by the pinned Python 3.12 oracle."""

import sys
import unicodedata
from pathlib import Path

if sys.version_info[:2] != (3, 12) or unicodedata.unidata_version != "15.0.0":
    raise RuntimeError("requires the oracle's Python 3.12 and Unicode 15.0.0")
out = Path(__file__).resolve().parents[2] / "src/history/casefold.txt"
out.write_text(
    "".join(
        f"{n:x} " + ",".join(format(ord(ch), "x") for ch in chr(n).casefold()) + "\n"
        for n in range(0x110000)
        if chr(n).casefold() != chr(n)
    ),
    encoding="utf-8",
)
