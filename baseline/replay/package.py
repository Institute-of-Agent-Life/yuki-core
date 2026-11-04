"""Build an immutable offline oracle handoff from the actual checked-out core tree."""
import json
import zipfile
from pathlib import Path

from ci.source import source_identity
from baseline.replay.freeze import digest


def make_candidate(root, output, index):
    source = source_identity(root)
    lock = json.loads((root / "baseline/lock.json").read_bytes())
    members = set(lock["files"]) | {"baseline/lock.json", "baseline/replay/scenarios.json"}
    members.update("docs/taskbook/v3.2/audit/results/" + name for name in lock["historical_evidence"])
    members.update(("LICENSE", "NOTICE"))
    metadata = {"kind": "v4-oracle", "status": "unpublished", "source_commit": source["commit"],
                "source_dirty": source["dirty"], "baseline_commit": index["source_commit"],
                "schema_version": lock["schema"], "sdk_version": None, "client_revision": 9,
                "connector_wire": 4, "connector_distribution": "0.4.3",
                "capabilities": ["legacy-client-admin-openapi", "legacy-wire4-reconnect",
                                 "partial-delivery", "app-qq-work", "next-normal-main", "A-load"],
                "verification": index["evidence"], "consumer_evidence": [],
                "files": {name: digest((root / name).read_bytes()) for name in sorted(members)}}
    content = {name: (root / name).read_bytes() for name in members}
    content["candidate.json"] = (json.dumps(metadata, sort_keys=True, indent=2) + "\n").encode()
    archive = output / "v4-oracle-candidate.zip"
    with zipfile.ZipFile(archive, "x", compression=zipfile.ZIP_STORED) as bundle:
        for name, data in sorted(content.items()):
            entry = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
            entry.create_system = 3
            entry.external_attr = 0o100644 << 16
            bundle.writestr(entry, data)
    with zipfile.ZipFile(archive) as bundle:
        assert bundle.testzip() is None
        manifest = json.loads(bundle.read("candidate.json"))
        for name, expected in manifest["files"].items():
            assert digest(bundle.read(name)) == expected
    return {"filename": archive.name, "sha256": digest(archive.read_bytes()),
            "source_commit": source["commit"], "source_dirty": source["dirty"], "status": "unpublished"}
