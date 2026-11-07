"""Prepare and qualify immutable additive surface packages in a local registry."""

from contextlib import contextmanager
import copy
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import tarfile
import tempfile
import tomllib
import zipfile

from contract import deterministic_zip


def encoded(value):
    return (
        json.dumps(value, sort_keys=True, ensure_ascii=False, indent=2) + "\n"
    ).encode()


def sha(data):
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read(path):
    return json.loads(path.read_bytes())


def version(value):
    require(
        re.fullmatch(r"1\.(0|[1-9][0-9]*)\.0", value), "expected additive 1.n.0 version"
    )
    return int(value.split(".")[1])


@contextmanager
def locked(registry):
    with (registry / ".lock").open("a+b") as stream:
        if os.name == "nt":
            import msvcrt

            if stream.tell() == 0:
                stream.write(b"0")
                stream.flush()
            stream.seek(0)
            msvcrt.locking(stream.fileno(), msvcrt.LK_LOCK, 1)
        else:
            import fcntl

            fcntl.flock(stream.fileno(), fcntl.LOCK_EX)
        try:
            yield
        finally:
            if os.name == "nt":
                stream.seek(0)
                msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(stream.fileno(), fcntl.LOCK_UN)


def initialize(registry, base, expected_digest, *, synthetic=False):
    raw = base.read_bytes()
    require(sha(raw) == expected_digest, "previous stable digest differs")
    manifest = json.loads(raw)
    require(
        version(manifest["version"]) == 0, "registry must start from the witnessed base"
    )
    require(manifest["surface"]["methods"] and manifest["consumers"], "empty base")
    require(
        version(manifest["min_reader"]) <= version(manifest["version"]),
        "invalid minimum reader",
    )
    registry.mkdir(parents=True, exist_ok=False)
    (registry / "registry.json").write_bytes(encoded({"synthetic": synthetic}))
    (registry / "base.json").write_bytes(raw)
    (registry / "versions").mkdir()


def latest(registry):
    current = read(registry / "base.json")
    digest = sha((registry / "base.json").read_bytes())
    directories = sorted(
        (p for p in (registry / "versions").iterdir() if not p.name.startswith(".")),
        key=lambda p: version(p.name),
    )
    for directory in directories:
        manifest = read(directory / "manifest.json")
        require(
            manifest["parent_stable_digest"] == digest
            and version(manifest["version"]) == version(current["version"]) + 1
            and directory.name == manifest["version"],
            "registry chain differs",
        )
        require(
            sha((directory / "package.zip").read_bytes()) == manifest["package_sha256"],
            "qualified package differs",
        )
        current = manifest
        digest = sha((directory / "manifest.json").read_bytes())
    return current, digest


def canonical(root):
    paths = [root / "protocol/plugin/gateway/host-preview.json"]
    paths += sorted((root / "protocol/plugin").glob("*/methods.json"))
    result = read(paths[0])
    for path in paths[1:]:
        for name, spec in read(path)["methods"].items():
            require(name not in result["methods"], "duplicate canonical method")
            result["methods"][name] = spec
    handshake_path = root / "protocol/plugin/handshake.schema.json"
    handshake = read(handshake_path)
    for name in ("status", "sdk_versions"):
        handshake["x-yuki"].pop(name, None)
    result["handshake_sha256"] = sha(encoded(handshake))
    paths.append(handshake_path)
    return result, {p.relative_to(root).as_posix(): sha(p.read_bytes()) for p in paths}


def additive(previous, current):
    """Only optional object properties can extend an existing accepted schema."""
    if previous == current:
        return
    require(
        isinstance(previous, dict) and isinstance(current, dict),
        "changed existing schema value",
    )
    require(previous.keys() == current.keys(), "changed existing schema structure")
    for key, value in previous.items():
        if key == "properties":
            require(value.keys() <= current[key].keys(), "removed accepted field")
            added = current[key].keys() - value.keys()
            require(
                not added.intersection(current.get("required", [])),
                "new required field",
            )
            for name, field in value.items():
                additive(field, current[key][name])
        else:
            additive(value, current[key])


def project(previous, idl, selection):
    result = copy.deepcopy(previous)
    old_methods, methods = previous["methods"], idl["methods"]
    require(old_methods.keys() <= methods.keys(), "removed stable method")
    for name, spec in old_methods.items():
        additive(spec, methods[name])
    for name, choice in selection.items():
        require(name in methods, "selected method is absent from canonical IDL")
        if choice == {"new_method": True}:
            require(name not in old_methods, "method already stable")
            require(methods[name].get("grant"), "new method requires an explicit grant")
            result["methods"][name] = copy.deepcopy(methods[name])
            continue
        require(
            name in old_methods and choice.keys() == {"fields"},
            "invalid surface selection",
        )
        require(choice["fields"], "empty optional field selection")
        for pointer in choice["fields"]:
            # JSON schema paths use arrays of original keys, without pointer unescaping.
            require(
                isinstance(pointer, list)
                and len(pointer) >= 3
                and len(pointer) % 2 == 1
                and pointer[0] in {"params", "result"}
                and all(key == "properties" for key in pointer[1::2]),
                "expected an optional property path",
            )
            old, new = result["methods"][name], methods[name]
            for key in pointer[:-2]:
                require(
                    isinstance(key, str) and key in old and key in new,
                    "unknown schema path",
                )
                old, new = old[key], new[key]
            field = pointer[-1]
            require(
                field in new["properties"] and field not in old["properties"],
                "field already stable or absent",
            )
            require(
                field not in new.get("required", [])
                and "default" not in new["properties"][field],
                "new field changes required/default semantics",
            )
            old["properties"][field] = copy.deepcopy(new["properties"][field])
    return result


def plan(registry, request, idl, idl_files):
    previous, digest = latest(registry)
    require(
        previous["handshake_sha256"] == idl["handshake_sha256"],
        "initialize contract changed",
    )
    require(
        re.fullmatch(r"[a-f0-9]{64}", request["parent_stable_digest"]),
        "invalid previous stable digest",
    )
    require(request["selection"], "empty surface promotion")
    require(
        re.fullmatch(r"[a-f0-9]{40}", request["candidate_source"]),
        "invalid candidate commit",
    )
    require(request["consumers"], "real capability consumers must be named")
    require(
        isinstance(request["consumers"], list)
        and all(
            isinstance(name, str)
            and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", name)
            for name in request["consumers"]
        ),
        "invalid consumer identifiers",
    )
    surface = project(previous["surface"], idl, request["selection"])
    return {
        "format": 1,
        "version": f"1.{version(previous['version']) + 1}.0",
        "parent_stable_digest": digest,
        "requested_parent_digest": request["parent_stable_digest"],
        "rebased": digest != request["parent_stable_digest"],
        "candidate_source": request["candidate_source"],
        "selection": request["selection"],
        "surface": surface,
        "surface_sha256": sha(encoded(surface)),
        "handshake_sha256": idl["handshake_sha256"],
        "idl_files": idl_files,
        "consumers": sorted(set(previous["consumers"]) | set(request["consumers"])),
        "new_consumers": sorted(set(request["consumers"])),
        "min_reader": previous["min_reader"],
        "published": False,
    }


def artifact(folder, record):
    name = record["path"]
    path = PurePosixPath(name)
    require(
        not path.is_absolute()
        and path.parts
        and ".." not in path.parts
        and "\\" not in name,
        "evidence path escapes package",
    )
    source = folder / name
    require(
        source.is_file()
        and not source.is_symlink()
        and source.resolve().is_relative_to(folder.resolve()),
        "evidence must be an original local file",
    )
    data = source.read_bytes()
    require(sha(data) == record["sha256"], "evidence bytes differ: " + name)
    return data


def sdk_surface(data, kind, expected_version):
    if kind == "python-sdk":
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            metadata = [
                n for n in archive.namelist() if n.endswith(".dist-info/METADATA")
            ]
            require(
                len(metadata) == 1
                and f"Version: {expected_version}\n"
                in archive.read(metadata[0]).decode(),
                "Python SDK version differs from planned minor",
            )
            return json.loads(archive.read("yuki_plugin_sdk/host-preview.json"))
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        members = [
            m
            for m in archive.getmembers()
            if m.name.endswith("/src/host-preview.json") and m.isfile()
        ]
        require(len(members) == 1, "Rust SDK projection is missing or ambiguous")
        prefix = members[0].name.removesuffix("/src/host-preview.json")
        manifest = tomllib.loads(
            archive.extractfile(prefix + "/Cargo.toml").read().decode()
        )
        require(
            manifest["package"]["version"] == expected_version,
            "Rust SDK version differs from planned minor",
        )
        return json.load(archive.extractfile(members[0]))


def fields_witnessed(name, choice, frames):
    requests = [row["frame"] for row in frames if row["frame"].get("method") == name]
    if choice == {"new_method": True}:
        return bool(requests)
    request_ids = {frame.get("id") for frame in requests}
    responses = [
        row["frame"]
        for row in frames
        if "method" not in row["frame"] and row["frame"].get("id") in request_ids
    ]
    for pointer in choice["fields"]:
        candidates = requests if pointer[0] == "params" else responses
        wire_path = [key for i, key in enumerate(pointer) if i % 2 == 0]
        present = False
        for frame in candidates:
            value = frame
            for key in wire_path:
                if not isinstance(value, dict) or key not in value:
                    break
                value = value[key]
            else:
                present = True
        if not present:
            return False
    return True


def qualify(registry, planned, folder):
    proof = read(folder / "evidence.json")
    synthetic = read(registry / "registry.json")["synthetic"]
    require(
        proof["synthetic"] == synthetic,
        "synthetic evidence cannot qualify a real release",
    )
    require(
        proof["plan_sha256"] == sha(encoded(planned)),
        "qualification belongs to another plan; rebuild and replay",
    )
    require(
        proof["candidate_source"] == planned["candidate_source"],
        "candidate source differs",
    )
    assets = proof["assets"]
    required = {"host-macos", "host-linux", "host-windows", "rust-sdk", "python-sdk"}
    require(assets.keys() == required, "three native hosts and both SDKs required")
    content = {
        "surface.json": encoded(planned["surface"]),
        "plan.json": encoded(planned),
        "evidence/evidence.json": encoded(proof),
    }

    def capture(record):
        data = artifact(folder, record)
        member = "evidence/" + record["path"]
        require(
            member not in content or content[member] == data,
            "conflicting package member",
        )
        content[member] = data
        return data

    require(
        len({item["path"] for item in assets.values()}) == len(assets),
        "artifact roles require distinct files",
    )
    for role, item in assets.items():
        data = capture(item)
        if role.endswith("sdk"):
            require(
                sdk_surface(data, role, planned["version"]) == planned["surface"],
                "SDK contains unwitnessed or missing methods",
            )
    require(
        proof["platforms"].keys() == {"macos", "linux", "windows"},
        "missing native qualification",
    )
    for platform, platform_proof in proof["platforms"].items():
        expected = {
            key: assets[key]["sha256"]
            for key in ("host-" + platform, "rust-sdk", "python-sdk")
        }
        require(
            platform_proof["artifacts"] == expected,
            "host/SDK bytes differ between witnesses",
        )
        require(
            platform_proof["consumers"].keys() == set(planned["consumers"]),
            "previous stable consumer was omitted",
        )
        seen = set()
        for consumer, witness in platform_proof["consumers"].items():
            require(
                witness["source_commit"] and witness["package"]["sha256"],
                "consumer provenance required",
            )
            capture(witness["package"])
            require(
                witness["cases"].keys() >= {"normal", "denied", "restart"},
                "business failure/restart evidence missing",
            )
            for case, record in witness["cases"].items():
                report = json.loads(capture(record))
                require(
                    report["status"] == "PASS"
                    and report["assertions"]
                    and all(value is True for value in report["assertions"].values()),
                    "consumer business check failed",
                )
                require(
                    report["plan_sha256"] == proof["plan_sha256"]
                    and report["artifacts"] == expected
                    and report["consumer_sha256"] == witness["package"]["sha256"],
                    "business replay is not bound to this exact package",
                )
                require(
                    report["advertised_methods"]
                    == sorted(["initialize", *planned["surface"]["methods"]]),
                    "host exposes an unwitnessed or missing method",
                )
                frames = [
                    json.loads(line)
                    for line in capture(report["transcript"]).splitlines()
                ]
                require(frames, "empty wire transcript")
                observed = {
                    r["frame"]["method"] for r in frames if "method" in r["frame"]
                }
                require(
                    set(report["methods"]) <= observed,
                    "method witness absent from original wire",
                )
                if consumer in planned["new_consumers"] and case == "normal":
                    seen.update(
                        name
                        for name in report["methods"]
                        if name in planned["selection"]
                        and fields_witnessed(name, planned["selection"][name], frames)
                    )
        require(
            set(planned["selection"]) <= seen, "new method lacks real consumer witness"
        )
    return proof, content


def validate_plan(registry, planned, idl, idl_files):
    request = {
        "candidate_source": planned["candidate_source"],
        "selection": planned["selection"],
        "consumers": planned["new_consumers"],
        "parent_stable_digest": planned["requested_parent_digest"],
    }
    require(
        plan(registry, request, idl, idl_files) == planned,
        "plan changed or stable advanced; rebase, rebuild and replay required",
    )


def promote(registry, planned, folder, idl, idl_files):
    validate_plan(registry, planned, idl, idl_files)
    proof, content = qualify(registry, planned, folder)
    # Staging is invisible to readers. The locked rename publishes one complete version.
    with tempfile.TemporaryDirectory(prefix=".prepare-", dir=registry) as temporary:
        stage = Path(temporary) / "version"
        stage.mkdir()
        deterministic_zip(stage / "package.zip", content)
        manifest = {
            **planned,
            "package_sha256": sha((stage / "package.zip").read_bytes()),
            "evidence_sha256": sha(encoded(proof)),
            "qualification": (
                "synthetic-control" if proof["synthetic"] else "release-ready"
            ),
            "assets": {
                role: {**item, "path": "evidence/" + item["path"]}
                for role, item in proof["assets"].items()
            },
            "evidence_directory": "evidence",
            "compatibility_matrix": {
                platform: {
                    consumer: {
                        "status": "verified",
                        "host_version": planned["version"],
                        "host_sha256": entry["artifacts"]["host-" + platform],
                        "sdk_version": planned["version"],
                        "consumer_source": witness["source_commit"],
                        "consumer_sha256": witness["package"]["sha256"],
                        "cases": sorted(witness["cases"]),
                    }
                    for consumer, witness in entry["consumers"].items()
                }
                for platform, entry in proof["platforms"].items()
            },
        }
        (stage / "manifest.json").write_bytes(encoded(manifest))
        with locked(registry):
            _, digest = latest(registry)
            require(
                digest == planned["parent_stable_digest"],
                "stable advanced; rebase, rebuild and replay required",
            )
            destination = registry / "versions" / planned["version"]
            require(not destination.exists(), "minor is already occupied")
            stage.rename(destination)
        return manifest
