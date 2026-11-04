"""Import verified synthetic observations (old source mounted read-only)."""
import argparse
import gzip
import hashlib
import json
import shutil
import subprocess
from pathlib import Path
from xml.etree import ElementTree

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path("fixtures/oracle/v4")
PROTOCOL = Path("protocol/baseline-v4")
SPEC = json.loads((ROOT / "baseline/replay/scenarios.json").read_bytes())


def digest(data):
    return hashlib.sha256(data).hexdigest()


def dump(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes((json.dumps(data, indent=2, ensure_ascii=False, sort_keys=True) + "\n").encode())


def junit(file, expected):
    suite = ElementTree.parse(file).find(".//testsuite")
    assert int(suite.get("tests")) == expected
    assert all(int(suite.get(key)) == 0 for key in ("errors", "failures", "skipped"))
    # Keep the original at the capture location. Exclude the machine's hostname.
    return {"original_sha256": digest(file.read_bytes()),
            "cases": [element.attrib for element in suite.findall("testcase")]}


def freeze(source, pair, workload, destination):
    spec = SPEC
    if destination.exists():
        raise FileExistsError(destination)
    fixtures, protocol = destination / FIXTURES, destination / PROTOCOL
    fixtures.mkdir(parents=True)
    protocol.mkdir(parents=True)
    git = lambda *args: subprocess.check_output(["git", "-C", str(source), *args])
    assert git("rev-parse", "HEAD").decode().strip() == spec["source_commit"]
    assert git("rev-parse", "HEAD^{tree}").decode().strip() == spec["source_tree"]
    tracked = git("ls-tree", "--name-only", "-r", "-z", "HEAD").decode().split("\0")
    sources = {"uv.lock", "pyproject.toml", "config/SOUL.md", "src/yuki/event_prompt.py",
               "src/yuki/services/main_agent_contract.py", "migrations/versions/0156_active_initiative_reconcile.py",
               "packages/yuki-agent-connector/pyproject.toml", "tests/support/compact_delivery_evidence.py"}
    sources.update(file.split("::")[0] for file in spec["cases"].values())
    sources.update(name for name in tracked if name.endswith("/plugin.toml") or (
        name.startswith(("src/yuki/prompting/", "src/yuki/conversation/", "src/yuki/services/",
                         "src/yuki/plugin_host/", "src/yuki/client_api/", "src/yuki/admin_api/"))
        and ("/prompt" in name or "manifest" in name or "openapi" in name)))
    materials = {}
    for name in sorted(sources):
        data = git("show", "HEAD:" + name)
        materials[name] = {"sha256": digest(data), "git_blob": git("rev-parse", "HEAD:" + name).decode().strip()}
        if name.endswith(("plugin.toml", "SOUL.md")):
            path = fixtures / "material" / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    for side in ("first", "second"):
        source_run = pair / side
        report = json.loads((source_run / "run.json").read_bytes())
        assert report["status"] == "verified" and report["source_commit"] == spec["source_commit"]
        assert report["uv_lock_sha256"] == materials["uv.lock"]["sha256"]
        raw = (source_run / "transcript.json.gz").read_bytes()
        assert digest(gzip.decompress(raw)) == report["raw_sha256"]
        case = fixtures / side
        case.mkdir()
        (case / "transcript.json.gz").write_bytes(raw)
        dump(case / "run.json", {**report, "junit": junit(source_run / "junit.xml", len(spec["cases"]))})
    assert (fixtures / "first/run.json").read_bytes() != (fixtures / "second/run.json").read_bytes()
    for name in ("client-openapi.json", "admin-openapi.json"):
        raw = (pair / "first" / name).read_bytes()
        assert raw == (pair / "second" / name).read_bytes()
        (protocol / name).write_bytes(raw)
    frames = json.loads(gzip.decompress((fixtures / "first/transcript.json.gz").read_bytes()))["wire4"]["frames"]
    dump(protocol / "wire4-frames.json", frames)
    performance = json.loads((workload / "run.json").read_bytes())
    assert performance["exit_code"] == 0 and performance["smoke"] is False
    for name in ("arrivals", "long-works"):
        (fixtures / (name + ".jsonl.gz")).write_bytes(gzip.compress((workload / (name + ".jsonl")).read_bytes(), mtime=0))
    dump(fixtures / "load.json", {**performance, "junit": junit(workload / "junit.xml", 1 + spec["load"]["long_works"])})
    audit = ROOT / "docs/taskbook/v3.2/audit/results"
    lock = {"format": 1, "source_commit": spec["source_commit"], "source_tree": spec["source_tree"],
            "schema": "0156", "client_revision": 9, "connector_distribution": "0.4.3", "connector_wire": 4,
            "source_files": materials, "capture_tools": {"time-machine": "2.19.0", "python-dateutil": "2.9.0.post0", "six": "1.17.0"},
            "historical_evidence": {file: digest((audit / file).read_bytes())
                                    for file in ("ci-source-proof.json", "r2-source-proof.json")},
            "files": {p.relative_to(destination).as_posix(): digest(p.read_bytes()) for p in destination.rglob("*")
                      if p.is_file() and not p.name.startswith("._")}}
    dump(destination / "baseline/lock.json", lock)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    for name in ("source", "pair", "workload", "destination"):
        parser.add_argument("--" + name, required=True, type=Path)
    args = parser.parse_args()
    freeze(args.source, args.pair, args.workload, args.destination)
