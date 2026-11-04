"""Build and verify the unpublished handshake preview using only immutable local inputs."""

import base64
import csv
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import queue
import re
import subprocess
import sys
import tarfile
import tempfile
import threading
import tomllib
import zipfile

from source import digest, source_identity

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "protocol/plugin/handshake.schema.json"
ROLES = {
    "rust-sdk",
    "python-sdk",
    "schema",
    "method-index",
    "golden",
    "peer",
    "verification",
}


def write_json(path, value):
    path.write_bytes(
        (
            json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
        ).encode()
    )


def deterministic_zip(path, content):
    with zipfile.ZipFile(path, "x", compression=zipfile.ZIP_STORED) as archive:
        for name, data in sorted(content.items()):
            entry = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
            entry.create_system = 3
            entry.external_attr = 0o100644 << 16
            archive.writestr(entry, data)


def wheel(path, meta):
    version = meta["sdk_versions"]["python"]
    pyproject = tomllib.loads(
        (ROOT / "sdk/python/pyproject.toml").read_text(encoding="utf-8")
    )
    if pyproject["project"]["version"] != version:
        raise ValueError("Python distribution version differs from IDL")
    name = "yuki_plugin_sdk"
    dist = f"{name}-{version}.dist-info"
    package = ROOT / "sdk/python" / name
    content = {
        name + "/" + f.name: f.read_bytes()
        for f in package.iterdir()
        if f.is_file() and not f.name.startswith("._") and f.suffix in {".py", ".json"}
    }
    python_files = content.copy()
    content[dist + "/WHEEL"] = (
        b"Wheel-Version: 1.0\nGenerator: yuki-core\nRoot-Is-Purelib: true\nTag: py3-none-any\n"
    )
    content[dist + "/METADATA"] = (
        f"Metadata-Version: 2.4\nName: yuki-plugin-sdk\nVersion: {version}\n"
        "Summary: Unpublished plugin handshake preview\nRequires-Python: >=3.12\nLicense-Expression: AGPL-3.0-only\n"
        "License-File: LICENSE\nLicense-File: NOTICE\n"
    ).encode()
    content[dist + "/entry_points.txt"] = (
        b"[console_scripts]\nyuki-plugin-peer = yuki_plugin_sdk.peer:main\n"
    )
    for file in ("LICENSE", "NOTICE"):
        content[dist + "/licenses/" + file] = (ROOT / file).read_bytes()
    record = io.StringIO(newline="")
    rows = csv.writer(record, lineterminator="\n")
    for member, data in sorted(content.items()):
        code = (
            base64.urlsafe_b64encode(hashlib.sha256(data).digest()).decode().rstrip("=")
        )
        rows.writerow([member, "sha256=" + code, len(data)])
    rows.writerow([dist + "/RECORD", "", ""])
    content[dist + "/RECORD"] = record.getvalue().encode()
    filename = f"{name}-{version}-py3-none-any.whl"
    deterministic_zip(path / filename, content)
    python_files["__main__.py"] = b"from yuki_plugin_sdk.peer import main\nmain()\n"
    python_files.update({n: (ROOT / n).read_bytes() for n in ("LICENSE", "NOTICE")})
    deterministic_zip(path / "yuki-conformance-peer.pyz", python_files)
    return filename


def rust_env():
    env = dict(os.environ)
    env["RUSTUP_TOOLCHAIN"] = tomllib.loads(
        (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8")
    )["toolchain"]["channel"]
    if os.name == "nt" and (bin_dir := env.get("YUKI_MINGW_BIN")):
        bin_dir = Path(bin_dir)
        env.update(
            CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=str(bin_dir / "gcc.exe"),
            CARGO_ENCODED_RUSTFLAGS="\x1f".join(
                [
                    "-C",
                    "link-self-contained=no",
                    "-C",
                    "dlltool=" + str(bin_dir / "dlltool.exe"),
                ]
            ),
        )
    return env


def crate(path, work, meta, source):
    version = meta["sdk_versions"]["rust"]
    package = ROOT / "sdk/rust/Cargo.toml"
    if (
        tomllib.loads(package.read_text(encoding="utf-8"))["package"]["version"]
        != version
    ):
        raise ValueError("Rust distribution version differs from IDL")
    for name in ("LICENSE", "NOTICE"):
        if (package.parent / name).read_bytes() != (ROOT / name).read_bytes():
            raise ValueError("Rust notice differs from core")
    target = work / "cargo-package"
    result = subprocess.run(
        [
            "cargo",
            "package",
            "--locked",
            "--offline",
            "--allow-dirty",
            "--no-verify",
            "--manifest-path",
            str(package),
            "--target-dir",
            str(target),
        ],
        env=rust_env(),
        capture_output=True,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.decode(errors="replace"))
    filename = f"yuki-plugin-sdk-{version}.crate"
    prefix = filename.removesuffix(".crate")
    with tarfile.open(target / "package" / filename, "r:gz") as archive:
        members = {
            m.name: archive.extractfile(m).read()
            for m in archive.getmembers()
            if m.isfile()
            and not any(part.startswith("._") for part in PurePosixPath(m.name).parts)
        }
    # Cargo's VCS note is optional in a verified Windows source snapshot. Bind both to the same producer.
    members[prefix + "/.cargo_vcs_info.json"] = (
        json.dumps(
            {
                "git": {"sha1": source["commit"], "dirty": source["dirty"]},
                "path_in_vcs": "sdk/rust",
            },
            sort_keys=True,
        )
        + "\n"
    ).encode()
    with (
        (path / filename).open("xb") as output,
        gzip.GzipFile(filename="", fileobj=output, mode="wb", mtime=0) as compressed,
    ):
        with tarfile.open(
            fileobj=compressed, mode="w", format=tarfile.GNU_FORMAT
        ) as archive:
            for name, data in sorted(members.items()):
                pure = PurePosixPath(name)
                if pure.is_absolute() or pure.parts[0] != prefix or ".." in pure.parts:
                    raise ValueError("unsafe Cargo member")
                item = tarfile.TarInfo(name)
                item.size = len(data)
                item.mode = 0o644
                archive.addfile(item, io.BytesIO(data))
    return filename


def install(dist, work, whl, rust):
    py = work / "python-install"
    p = subprocess.run(
        [
            sys.executable,
            "-m",
            "pip",
            "--disable-pip-version-check",
            "install",
            "--no-index",
            "--no-deps",
            "--no-compile",
            "--target",
            str(py),
            str(dist / whl),
        ],
        capture_output=True,
    )
    if p.returncode:
        raise RuntimeError(p.stderr.decode(errors="replace"))
    rust_src = work / "rust-install"
    with tarfile.open(dist / rust, "r:gz") as archive:
        for member in archive.getmembers():
            name = PurePosixPath(member.name)
            if (
                not member.isfile()
                or name.is_absolute()
                or ".." in name.parts
                or name.parts[0] != rust.removesuffix(".crate")
            ):
                raise ValueError("unsafe crate member")
        archive.extractall(rust_src, filter="data")
    cargo = rust_src / rust.removesuffix(".crate") / "Cargo.toml"
    target = work / "rust-target"
    command = [
        "cargo",
        "build",
        "--locked",
        "--offline",
        "--manifest-path",
        str(cargo),
        "--target-dir",
        str(target),
        "--bin",
        "yuki-plugin-peer",
    ]
    quality = [
        ["cargo", "fmt", "--manifest-path", str(cargo), "--", "--check"],
        [
            "cargo",
            "clippy",
            "--locked",
            "--offline",
            "--manifest-path",
            str(cargo),
            "--target-dir",
            str(target),
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    ]
    for stage in quality:
        checked = subprocess.run(stage, cwd=work, env=rust_env(), capture_output=True)
        if checked.returncode:
            raise RuntimeError(
                checked.stdout.decode(errors="replace")
                + checked.stderr.decode(errors="replace")
            )
    p = subprocess.run(command, cwd=work, env=rust_env(), capture_output=True)
    if p.returncode:
        raise RuntimeError(p.stderr.decode(errors="replace"))
    return (
        [
            str(
                target
                / "debug"
                / ("yuki-plugin-peer.exe" if os.name == "nt" else "yuki-plugin-peer")
            )
        ],
        [sys.executable, "-P", "-m", "yuki_plugin_sdk.peer"],
        {
            **os.environ,
            "PYTHONPATH": str(py),
            "PYTHONNOUSERSITE": "1",
            "PYTHONUTF8": "1",
        },
    )


def crosswire(commands, work):
    rust, python, env = commands
    peers = []

    def reader(stream, channel):
        for line in stream:
            channel.put(line)
        channel.put(None)

    channels = []
    transcript = {"rust": [], "python": []}
    try:
        for cmd in (rust, python):
            p = subprocess.Popen(
                [*cmd, "--initiate"],
                cwd=work,
                env=env,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            peers.append(p)
            channel = queue.Queue()
            threading.Thread(
                target=reader, args=(p.stdout, channel), daemon=True
            ).start()
            channels.append(channel)
        for key in ("method", "result"):
            pair = [channel.get(timeout=20) for channel in channels]
            if any(line is None for line in pair):
                raise RuntimeError("early EOF during crosswire")
            for index, raw in enumerate(pair):
                frame = json.loads(raw)
                if key not in frame or frame["id"] != 1:
                    raise ValueError("unexpected crosswire frame")
                transcript[("rust", "python")[index]].append(frame)
                peers[1 - index].stdin.write(raw)
                peers[1 - index].stdin.flush()
        for peer in peers:
            peer.stdin.close()
        for peer in peers:
            if peer.wait(timeout=15) != 0:
                raise RuntimeError(peer.stderr.read().decode(errors="replace"))
        return transcript
    finally:
        for peer in peers:
            if peer.poll() is None:
                peer.kill()
                peer.wait(timeout=10)
            for stream in (peer.stdin, peer.stdout, peer.stderr):
                stream.close()


def build(out, work):
    out.mkdir(parents=True, exist_ok=False)
    work.mkdir(parents=True, exist_ok=False)
    subprocess.run(
        [sys.executable, str(ROOT / "protocol/plugin/generate.py"), "--check"],
        check=True,
    )
    source = source_identity(ROOT)
    schema = json.loads(SCHEMA.read_bytes())
    meta = schema["x-yuki"]
    names = {
        "python-sdk": wheel(out, meta),
        "rust-sdk": crate(out, work, meta, source),
        "peer": "yuki-conformance-peer.pyz",
    }
    for role, name in [
        ("schema", "handshake.schema.json"),
        ("method-index", "method-index.json"),
        ("golden", "handshake.ndjson"),
    ]:
        (out / name).write_bytes((ROOT / "protocol/plugin" / name).read_bytes())
        names[role] = name
    executables = install(out, work, names["python-sdk"], names["rust-sdk"])
    transcript = crosswire(executables, work)
    golden = [
        json.loads(line) for line in (out / names["golden"]).read_bytes().splitlines()
    ]
    if not all(frames == golden for frames in transcript.values()):
        raise ValueError("installed peers differ from IDL golden")
    peer = subprocess.run(
        [sys.executable, str(out / names["peer"])],
        cwd=work,
        input=(out / names["golden"]).read_bytes().splitlines()[0] + b"\n",
        capture_output=True,
        timeout=10,
        check=True,
    )
    if json.loads(peer.stdout) != golden[1]:
        raise ValueError("reference peer differs from installed SDK")
    names["verification"] = "sdk-verification.json"
    write_json(
        out / names["verification"],
        {
            "status": "verified",
            "both_directions": transcript,
            "portable_peer_verified": True,
        },
    )
    manifest = {
        "kind": "handshake-preview",
        "status": "unpublished",
        "source_commit": source["commit"],
        "source_dirty": source["dirty"],
        "schema_revision": meta["schema_revision"],
        "schema_sha256": digest(SCHEMA.read_bytes()),
        "sdk_versions": meta["sdk_versions"],
        "wire_versions": meta["versions"],
        "capabilities": meta["features"],
        "consumer_receipts": [],
        "verification": "verified",
        "members": {
            role: {
                "name": name,
                "sha256": digest((out / name).read_bytes()),
                "bytes": (out / name).stat().st_size,
            }
            for role, name in names.items()
        },
    }
    write_json(out / "candidate.json", manifest)
    members = {
        f["name"]: (out / f["name"]).read_bytes() for f in manifest["members"].values()
    }
    members["candidate.json"] = (out / "candidate.json").read_bytes()
    deterministic_zip(out / "yuki-handshake-preview.zip", members)
    verify(out)
    return manifest


def verify(path):
    raw = (path / "candidate.json").read_bytes()
    m = json.loads(raw)
    if (
        m["kind"] != "handshake-preview"
        or m["status"] != "unpublished"
        or m["verification"] != "verified"
        or not re.fullmatch("[0-9a-f]{40}", m["source_commit"])
        or set(m["members"]) != ROLES
    ):
        raise ValueError("incomplete handshake preview")
    for item in m["members"].values():
        name = item["name"]
        if Path(name).name != name or "\\" in name or name in (".", ".."):
            raise ValueError("unsafe member name")
        artifact = path / name
        if artifact.is_symlink() or not artifact.is_file():
            raise ValueError("missing regular artifact: " + name)
        data = artifact.read_bytes()
        if digest(data) != item["sha256"] or len(data) != item["bytes"]:
            raise ValueError("artifact digest mismatch: " + name)
    with zipfile.ZipFile(path / "yuki-handshake-preview.zip") as archive:
        expected = {"candidate.json", *(item["name"] for item in m["members"].values())}
        if (
            len(archive.infolist()) != len(expected)
            or set(archive.namelist()) != expected
            or archive.read("candidate.json") != raw
        ):
            raise ValueError("handoff archive manifest differs")
        for item in m["members"].values():
            if digest(archive.read(item["name"])) != item["sha256"]:
                raise ValueError("handoff member differs")
    return {
        **m,
        "candidate_manifest_sha256": digest(raw),
        "archive_sha256": digest((path / "yuki-handshake-preview.zip").read_bytes()),
    }


def pin(path, lock, expected_sha256, *, allow_working_tree=False):
    value = verify(path)
    if value["archive_sha256"] != expected_sha256:
        raise ValueError("supplied archive hash differs from handoff")
    if value["source_dirty"] and not allow_working_tree:
        raise ValueError("development-only dirty candidate cannot be pinned")
    lock.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=lock.parent, delete=False) as tmp:
        pending = Path(tmp.name)
    try:
        write_json(pending, value)
        os.replace(pending, lock)
    finally:
        pending.unlink(missing_ok=True)
