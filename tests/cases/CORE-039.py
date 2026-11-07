"""Build and install the release candidate on this native host without the producer checkout."""

import importlib.util
import json
import os
import subprocess
import sys
import urllib.request
import zipfile

from tests.harness.native_only_search import exercise as native_search


def run(ctx):
    sys.path.insert(0, str(ctx.root / "ci"))
    from dist import archive
    from source import digest, source_identity
    from verify_dist import verify, native_target
    from tests.harness.portable_cleanup import check_workspace
    from tests.harness.json_file import take_command

    check_workspace(ctx)
    mailbox = ctx.working / "original-command.json"
    original = {"method": "gateway.inbound", "params": {"source": "original"}}
    mailbox.write_text(json.dumps(original), encoding="utf8")
    if os.name == "nt":
        with mailbox.open("rb"):
            ctx.equal(
                "locked mailbox is deferred before dispatch",
                take_command(mailbox),
                None,
            )
            ctx.equal(
                "deferred mailbox retains its original bytes",
                json.loads(mailbox.read_bytes()),
                original,
            )
    ctx.equal(
        "released mailbox returns the original command", take_command(mailbox), original
    )
    ctx.equal("claimed mailbox cannot dispatch twice", take_command(mailbox), None)

    spec = importlib.util.spec_from_file_location(
        "host_case", ctx.root / "tests/cases/CORE-032.py"
    )
    case = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(case)
    native_search(ctx, "release")
    host_receipt = case.run(ctx, "release")
    ctx.write("host-delegation.json", host_receipt)
    private = ctx.working / "test-signer"
    subprocess.run(
        ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(private)],
        check=True,
        timeout=20,
    )
    pub = private.with_suffix(".pub")
    # This key lives only in the disposable conformance case, independently of the package.
    local = ctx.working / "distribution"
    source = source_identity(ctx.root)
    package, manifest = archive(
        local,
        lambda name: (ctx.output / name).read_bytes(),
        private,
        source,
        development=True,
    )
    sha = digest(package.read_bytes())
    fixtures = digest((ctx.root / "fixtures/host/boundaries.json").read_bytes())
    args = (pub, source["commit"], sha, fixtures)
    install = ctx.working / "empty-install"
    actual = verify(package, *args, destination=install)
    ctx.equal(
        "certified native byte identity",
        actual["members"][
            "bin/" + ("yuki-core.exe" if os.name == "nt" else "yuki-core")
        ]["sha256"],
        host_receipt["binary_sha256"],
    )
    exe = install / "bin" / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
    data = ctx.working / "first-launch"
    descriptor = json.loads(
        subprocess.check_output([exe, "plugin-surface"], timeout=15)
    )
    profile = json.loads(
        (ctx.root / "protocol/plugin/stable/1.0/profile.json").read_bytes()
    )
    ctx.equal(
        "default release describes the original witnessed base",
        descriptor["surface_sha256"],
        profile["surface_sha256"],
    )
    ctx.equal(
        "default release method inventory remains exact",
        descriptor["methods"],
        sorted(["initialize", *profile["surface"]["methods"]]),
    )
    ctx.equal(
        "ordinary development executable retains explicit base selection",
        descriptor["selected_by_default"],
        False,
    )
    ctx.check("surface reflection does not initialize an owner", not data.exists())
    ctx.write("plugin-surface.json", descriptor)
    env = (
        {
            "PATH": os.path.join(os.environ["SystemRoot"], "System32"),
            "SystemRoot": os.environ["SystemRoot"],
        }
        if os.name == "nt"
        else {"PATH": "/usr/bin:/bin"}
    )
    subprocess.run(
        [exe, "--data", data, "init", "--memory-mode", "lexical-only"],
        cwd=install,
        env=env,
        check=True,
        capture_output=True,
        timeout=20,
    )
    proc = subprocess.Popen(
        [exe, "--data", data, "run"],
        cwd=install,
        env=env,
        stdout=subprocess.PIPE,
        text=True,
    )
    try:
        boot = json.loads(proc.stdout.readline())
        with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(
            "http://" + boot["listen"] + "/readyz", timeout=5
        ) as response:
            ctx.check(
                "native installed owner listening on loopback",
                json.load(response)["ready"],
            )
        subprocess.run(
            [exe, "--data", data, "operator", "shutdown"],
            cwd=install,
            env=env,
            check=True,
            capture_output=True,
            timeout=20,
        )
        ctx.equal("isolated installed owner shut down", proc.wait(timeout=20), 0)
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=10)
        proc.stdout.close()
    rejected = []

    def reject(label, fn):
        try:
            fn()
        except (ValueError, KeyError):
            rejected.append(label)
        ctx.check(label, label in rejected)

    system, arch = native_target()
    reject(
        "reject swapped architecture",
        lambda: verify(
            package, *args, target=(system, "arm64" if arch == "x86_64" else "x86_64")
        ),
    )
    reject(
        "reject wrong fixture pin",
        lambda: verify(package, pub, source["commit"], sha, "0" * 64),
    )
    reject(
        "reject adjacent source", lambda: verify(package, pub, "0" * 40, sha, fixtures)
    )
    other = ctx.working / "untrusted"
    subprocess.run(
        ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(other)], check=True
    )
    reject(
        "reject unrelated signing key",
        lambda: verify(
            package, other.with_suffix(".pub"), source["commit"], sha, fixtures
        ),
    )
    corrupt = ctx.working / "changed.zip"
    with zipfile.ZipFile(package) as z, zipfile.ZipFile(corrupt, "x") as out:
        for info in z.infolist():
            contents = z.read(info)
            out.writestr(
                info,
                (
                    b"substituted"
                    if info.filename == "fixtures/host-boundaries.json"
                    else contents
                ),
            )
    reject(
        "reject changed member under valid signed manifest",
        lambda: verify(
            corrupt, pub, source["commit"], digest(corrupt.read_bytes()), fixtures
        ),
    )
    for name, mode in [
        ("../escape", 0o100644),
        ("symlink", 0o120777),
        ("unexpected", 0o100644),
    ]:
        bad = ctx.working / (name.split("/")[-1] + ".zip")
        with zipfile.ZipFile(package) as z, zipfile.ZipFile(bad, "x") as out:
            for info in z.infolist():
                out.writestr(info, z.read(info))
            info = zipfile.ZipInfo(name)
            info.create_system = 3
            info.external_attr = mode << 16
            out.writestr(info, b"outside candidate")
        absent = ctx.working / ("rejected-" + name.split("/")[-1])
        reject(
            "reject unsafe or extra member " + name,
            lambda: verify(
                bad,
                pub,
                source["commit"],
                digest(bad.read_bytes()),
                fixtures,
                destination=absent,
            ),
        )
        ctx.check("rejection happens before extraction", not absent.exists())
    ctx.check(
        "actual target graph and licenses accompany release",
        len(json.loads((install / "sbom.json").read_bytes())["packages"]) > 10
        and any((install / "licenses/cargo").iterdir()),
    )
    ctx.check(
        "source and SDK wire revisions pinned",
        manifest["host_schema_sha256"] == host_receipt["host_idl_sha256"]
        and all(
            manifest["members"][n]["sha256"] == digest((install / n).read_bytes())
            for n in manifest["members"]
            if n.startswith("sdk/")
        ),
    )
    return {
        "source_commit": source["commit"],
        "source_tree_sha256": source["tree_sha256"],
        "dirty": source["dirty"],
        "platform": [system, arch],
        "archive_sha256": sha,
        "fixtures_sha256": fixtures,
        "binary_sha256": host_receipt["binary_sha256"],
        "signature": "ephemeral conformance signer",
        "sdk_versions": manifest["sdk_versions"],
        "methods": manifest["implemented_methods"],
        "rejected": rejected,
        "oci": "separately validated by candidate workflow",
        "published": False,
    }
