"""Build the pinned Linux runtime locally, check its isolated owner, export and verify its OCI graph."""

import argparse
import json
import subprocess
import sys
from pathlib import Path
from source import digest, source_identity
from verify_dist import NAMESPACE
from verify_oci import hash_file, verify_layout, verify_oci

ROOT = Path(__file__).resolve().parents[1]
SMOKE = r"""
set -eu
test "$(id -u)" = 65532
test ! -S /var/run/docker.sock
for tool in python python3 ffmpeg docker cargo; do ! command -v "$tool"; done
yuki-core --data /data/owner init >/dev/null
yuki-core --data /data/owner run >/data/ready.log &
pid=$!
trap 'kill "$pid" 2>/dev/null || true' EXIT
for i in $(seq 1 100); do test ! -s /data/ready.log || break; sleep .1; done
port=$(sed -n 's/.*"listen":"127.0.0.1:\([0-9]*\)".*/\1/p' /data/ready.log)
test -n "$port"
exec 3<>/dev/tcp/127.0.0.1/"$port"
printf 'GET /readyz HTTP/1.0\r\nHost: localhost\r\n\r\n' >&3
answer=$(cat <&3)
[[ "$answer" == *'{"ready":true}'* ]]
yuki-core --data /data/owner operator shutdown >/dev/null
wait "$pid"
"""


def build(out, signing_key, development=False):
    source = source_identity(ROOT)
    if source["dirty"] and not development:
        raise ValueError("clean source required")
    out.mkdir(parents=True, exist_ok=False)
    stage = out / "builder"
    subprocess.run(
        [sys.executable, str(ROOT / "ci/docker-build.py"), "--out", str(stage)],
        check=True,
    )
    b = json.loads((stage / "manifest.json").read_bytes())
    if b["source"] != source:
        raise ValueError("build source changed")
    image = b["image_id"]
    inspection = json.loads(
        subprocess.check_output(["docker", "image", "inspect", image])
    )[0]
    if (
        inspection["Os"] != "linux"
        or inspection["Config"]["User"] != "65532:65532"
        or inspection["Config"]["Entrypoint"] != ["yuki-core"]
    ):
        raise ValueError("wrong OCI runtime defaults")
    run = subprocess.run(
        [
            "docker",
            "run",
            "--rm",
            "--network",
            "none",
            "--read-only",
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges",
            "--tmpfs",
            "/data:rw,nosuid,nodev,mode=1777",
            "--entrypoint",
            "/bin/bash",
            image,
            "-ec",
            SMOKE,
        ],
        capture_output=True,
        timeout=60,
    )
    (out / "runtime.log").write_bytes(run.stdout + run.stderr)
    if run.returncode:
        raise RuntimeError("real isolated OCI smoke failed; see runtime.log")
    target = (
        out
        / f"yuki-core-{source['commit'][:12]}-linux-{inspection['Architecture']}.oci.tar"
    )
    subprocess.run(
        ["docker", "image", "save", "--output", str(target), b["tag"]], check=True
    )
    graph = verify_layout(target)
    m = {
        "source_commit": source["commit"],
        "tree_sha256": source["tree_sha256"],
        "dirty": source["dirty"],
        "platform": "linux/" + inspection["Architecture"],
        "image_id": image,
        "archive": target.name,
        "sha256": hash_file(target),
        "oci": graph,
        "host_idl_sha256": digest(
            (ROOT / "protocol/plugin/gateway/host-preview.json").read_bytes()
        ),
        "nonroot_owner": True,
        "readyz": "verified",
        "python_ffmpeg_docker_socket": "absent",
        "published": False,
    }
    manifest = out / "oci-candidate.json"
    manifest.write_text(
        json.dumps(m, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    subprocess.run(
        [
            "ssh-keygen",
            "-Y",
            "sign",
            "-q",
            "-f",
            str(signing_key),
            "-n",
            NAMESPACE,
            str(manifest),
        ],
        check=True,
    )
    public = out / "signer.pub"
    public.write_bytes(
        subprocess.check_output(["ssh-keygen", "-y", "-f", str(signing_key)]).strip()
        + b"\n"
    )
    verify_oci(target, manifest, public, source["commit"], m["sha256"], m["platform"])
    print(json.dumps(m))


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--signing-key", type=Path, required=True)
    p.add_argument("--development", action="store_true")
    a = p.parse_args()
    build(a.out, a.signing_key, a.development)
