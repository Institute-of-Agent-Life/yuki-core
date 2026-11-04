"""Black-box checks against the independently installed Python/Rust distributions."""

import ast
import copy
import hashlib
import json
import os
import subprocess
import sys
import tarfile
import zipfile


def verify(ctx, dist):
    doc = json.loads((dist / "candidate.json").read_bytes())
    members = doc["members"]
    work = ctx.working / "installed"
    rust = (
        work
        / "rust-target/debug"
        / ("yuki-plugin-peer.exe" if os.name == "nt" else "yuki-plugin-peer")
    )
    env = {
        **os.environ,
        "PYTHONNOUSERSITE": "1",
        "PYTHONPATH": str(work / "python-install"),
    }
    cmds = {
        "Rust": [str(rust)],
        "Python": [sys.executable, "-P", "-m", "yuki_plugin_sdk.peer"],
        "portable": [sys.executable, str(dist / members["peer"]["name"])],
    }
    transcript = json.loads((dist / members["verification"]["name"]).read_bytes())
    golden = [
        json.loads(line)
        for line in (dist / members["golden"]["name"]).read_bytes().splitlines()
    ]
    ctx.equal(
        "installed Rust and Python each initiate and answer with the same connection-local ID",
        transcript["both_directions"],
        {"rust": golden, "python": golden},
    )
    ctx.equal("preview contains only initialize", doc["capabilities"], ["initialize"])
    ctx.equal(
        "schema SHA matches index",
        doc["schema_sha256"],
        json.loads((dist / members["method-index"]["name"]).read_bytes())[
            "schema_sha256"
        ],
    )
    whl = dist / members["python-sdk"]["name"]
    with zipfile.ZipFile(whl) as package:
        imports = []
        for name in package.namelist():
            if name.endswith(".py"):
                tree = ast.parse(package.read(name).decode())
                for node in ast.walk(tree):
                    if isinstance(node, ast.Import):
                        imports.extend(v.name for v in node.names)
                    if isinstance(node, ast.ImportFrom) and node.module:
                        imports.append(node.module)
        ctx.check(
            "wheel has no core application imports",
            not any(n.split(".")[0] in {"src", "yuki", "yuki_core"} for n in imports),
        )
    with tarfile.open(dist / members["rust-sdk"]["name"]) as archive:
        ctx.check(
            "Rust archive contains no filesystem metadata",
            all(
                not any(p.startswith("._") for p in f.name.split("/")) for f in archive
            ),
        )
    offer = golden[0]

    def rpc(identifier, *, change=None, method="initialize"):
        call = copy.deepcopy(offer)
        call.update(id=identifier, method=method)
        if change:
            call["params"].update(change)
        return call

    sequence = [
        rpc(
            "optional",
            change={
                "optional_features": ["future_optional"],
                "future_field": True,
                "namespace": "other",
                "grants": ["admin"],
            },
        ),
        rpc(
            "wrong-version",
            change={
                "versions": {
                    "min": {"major": 9, "minor": 0},
                    "max": {"major": 9, "minor": 0},
                }
            },
        ),
        rpc("unavailable", change={"required_features": ["gateway.inbound"]}),
        rpc(
            "duplicate",
            change={
                "declarations": [{"kind": "test", "name": "fixture", "revision": 1}] * 2
            },
        ),
        rpc("changed", change={"manifest_revision": "unapproved"}),
        rpc("method", method="unapproved.method"),
        rpc(
            "invalid",
            change={
                "versions": {
                    "min": {"major": True, "minor": 0},
                    "max": {"major": 1, "minor": 0},
                }
            },
        ),
        rpc(1),
    ]
    sequence[-1]["params"].pop("optional_features")
    expected = [None, -32010, -32011, -32013, -32012, -32601, -32602, None]
    observed = {}
    for label, cmd in cmds.items():
        data = b"".join(json.dumps(f).encode() + b"\n" for f in sequence)
        process = subprocess.run(
            cmd, cwd=work, env=env, input=data, capture_output=True, timeout=15
        )
        ctx.equal(
            label + " isolated errors preserve the stdio session", process.returncode, 0
        )
        lines = [json.loads(raw) for raw in process.stdout.splitlines()]
        ctx.equal(
            label + " IDs remain scoped and typed",
            [r["id"] for r in lines],
            [r["id"] for r in sequence],
        )
        ctx.equal(
            label + " rejects exactly the requested fault branches",
            [r.get("error", {}).get("code") for r in lines],
            expected,
        )
        for r in (lines[0], lines[-1]):
            ctx.equal(
                label + " namespace and grants come from approved binding",
                (r["result"]["namespace"], r["result"]["grants"]),
                ("fixture.plugin", []),
            )
        ctx.equal(
            label + " optional negotiation does not claim business methods",
            lines[0]["result"]["extension_points"],
            [],
        )
        # A separate connection reuses the numeric ID. A stray response has no pending owner.
        fresh = subprocess.run(
            cmd,
            cwd=work,
            env=env,
            input=json.dumps(offer).encode() + b"\n",
            capture_output=True,
            check=True,
            timeout=8,
        )
        ctx.equal(
            label + " fresh connection may use ID 1",
            json.loads(fresh.stdout),
            golden[1],
        )
        unknown = subprocess.run(
            cmd,
            cwd=work,
            env=env,
            input=json.dumps(golden[1]).encode() + b"\n",
            capture_output=True,
            timeout=8,
        )
        ctx.check(
            label + " refuses response ID from another connection",
            unknown.returncode != 0,
        )
        oversized = subprocess.run(
            cmd,
            cwd=work,
            env=env,
            input=b"x" * 65537 + b"\n",
            capture_output=True,
            timeout=8,
        )
        ctx.check(
            label + " rejects oversized unterminated frames", oversized.returncode != 0
        )
        declaration = {"name": "fixture.cap", "kind": "synthetic", "revision": 1}
        binding = {
            "namespace": "bound.fixture",
            "grants": ["fixture:read"],
            "manifest_revision": "approved-r2",
            "declarations": [declaration],
        }
        binding_path = work / (label + "-binding.json")
        binding_path.write_text(json.dumps(binding))
        declared = rpc(
            7,
            change={
                "manifest_revision": "approved-r2",
                "declarations": [declaration],
                "namespace": "forged",
            },
        )
        altered = copy.deepcopy(declared)
        altered["params"]["declarations"][0]["revision"] = 2
        wire = b"".join(
            json.dumps(f).encode() + b"\n" for f in (declared, altered, declared)
        )
        bound = subprocess.run(
            [*cmd, "--binding", str(binding_path)],
            cwd=work,
            env=env,
            input=wire,
            capture_output=True,
            check=True,
            timeout=8,
        )
        accepted, rejected, restored = map(json.loads, bound.stdout.splitlines())
        ctx.equal(
            label + " trusted namespace and narrow grant remain bound",
            (accepted["result"]["namespace"], accepted["result"]["grants"]),
            ("bound.fixture", ["fixture:read"]),
        )
        ctx.equal(
            label + " changed declaration cannot replace manifest",
            (rejected["error"]["code"], restored),
            (-32012, accepted),
        )
        observed[label] = lines
    command = [
        sys.executable,
        str(ctx.root / "ci/publish-contract.py"),
        "pin",
        str(dist),
        "--lock",
        str(ctx.working / "consumer.lock.json"),
    ]
    archive_sha = hashlib.sha256(
        (dist / "yuki-handshake-preview.zip").read_bytes()
    ).hexdigest()
    subprocess.run(
        [*command, "--expected-sha256", archive_sha, "--allow-working-tree"],
        check=True,
        capture_output=True,
    )
    lock = ctx.working / "consumer.lock.json"
    pinned = lock.read_bytes()
    ctx.equal(
        "consumer pin anchors expected ZIP",
        json.loads(pinned)["archive_sha256"],
        archive_sha,
    )
    bad = subprocess.run(
        [*command, "--expected-sha256", "0" * 64, "--allow-working-tree"],
        capture_output=True,
    )
    ctx.check(
        "wrong digest preserves existing pin",
        bad.returncode != 0 and lock.read_bytes() == pinned,
    )
    # Isolate the missing-artifact rehearsal from the verified preview delivered by the case.
    broken = ctx.working / "incomplete"
    broken.mkdir()
    (broken / "candidate.json").write_bytes((dist / "candidate.json").read_bytes())
    missing = subprocess.run(
        [
            *command[:3],
            str(broken),
            *command[4:],
            "--expected-sha256",
            archive_sha,
            "--allow-working-tree",
        ],
        capture_output=True,
    )
    ctx.check(
        "missing distribution preserves previous pin",
        missing.returncode != 0 and lock.read_bytes() == pinned,
    )
    return {
        "status": "handshake-preview",
        "source_commit": doc["source_commit"],
        "schema_revision": doc["schema_revision"],
        "sdk_versions": doc["sdk_versions"],
        "archive_sha256": archive_sha,
        "members": members,
        "crosswire": transcript,
        "fault_transcripts": observed,
        "real_plugin_consumers": [],
    }
