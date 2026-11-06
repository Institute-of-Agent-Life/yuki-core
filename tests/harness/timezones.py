"""Pinned, independent IANA calendar fixtures on hosts without a system tzdb."""
import hashlib
import json
from pathlib import Path
from zoneinfo import ZoneInfo as NativeZoneInfo

ROOT = Path(__file__).resolve().parents[1] / "fixtures/timezones"


def ZoneInfo(key):
    provenance = json.loads((ROOT / "provenance.json").read_text())
    member = provenance["members"][key]
    path = ROOT / key
    raw = path.read_bytes()
    if len(raw) != member["bytes"] or hashlib.sha256(raw).hexdigest() != member["sha256"]:
        raise ValueError("calendar fixture differs from pinned IANA source")
    with path.open("rb") as stream:
        return NativeZoneInfo.from_file(stream, key=key)
