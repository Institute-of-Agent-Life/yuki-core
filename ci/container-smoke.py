"""Verify the built Linux image as its non-root user with no external network."""

import argparse
import json
import subprocess
import uuid
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--image-manifest", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    image = json.loads(args.image_manifest.read_text())
    name = "yuki-core-smoke-" + uuid.uuid4().hex[:10]
    observations = []
    result = {"image_id": image["image_id"], "source": image["source"], "published": False,
              "network": "none", "status": "failed", "failure": None}
    errors = (args.out / "server-stderr.txt").open("w")
    server = subprocess.Popen(["docker", "run", "--rm", "--name", name, "--network", "none",
                               "--entrypoint", "/bin/sh", image["image_id"], "-c",
                               "yuki-core --data /tmp/yuki-core-smoke init --memory-mode lexical-only && exec yuki-core --data /tmp/yuki-core-smoke run"],
                              stdout=subprocess.PIPE, stderr=errors, text=True)
    try:
        observations.append(json.loads(server.stdout.readline()))
        observations.append(json.loads(server.stdout.readline()))

        def command(*operation, expected=0, user=None):
            args = ["docker", "exec"]
            if user: args += ["--user", user]
            args += [name, "yuki-core", "--data", "/tmp/yuki-core-smoke", *operation]
            r = subprocess.run(args, capture_output=True, text=True, timeout=20)
            observations.append({"command": args, "exit_code": r.returncode,
                                 "stdout": r.stdout, "stderr": r.stderr})
            if r.returncode != expected: raise RuntimeError("container command outcome: " + " ".join(operation))
            return r

        command("run", expected=1)
        command("migrate", expected=1)
        info = json.loads(command("operator", "database-info").stdout)
        if info["foreign_keys"] != 1 or info["journal_mode"] != "wal": raise RuntimeError("SQLite configuration")
        command("operator", "set-label", "container owner")
        if json.loads(command("operator", "get-label").stdout)["value"] != "container owner": raise RuntimeError("wrong owner result")
        command("operator", "get-label", user="65533:65533", expected=1)
        command("operator", "shutdown")
        if server.wait(timeout=20): raise RuntimeError("container shutdown")
        result.update(status="verified", sqlite=info, other_uid="denied", exit_code=0)
    except Exception as error:
        result.update(failure=str(error), exit_code=1)
    finally:
        if server.poll() is None:
            subprocess.run(["docker", "stop", "--time", "5", name], capture_output=True)
            server.wait(timeout=15)
        errors.close()
        (args.out / "observations.json").write_text(json.dumps(observations, indent=2))
        (args.out / "manifest.json").write_text(json.dumps(result, indent=2))
    print(json.dumps({key: result[key] for key in ["status", "failure", "exit_code", "image_id"]}))
    return result["exit_code"]


if __name__ == "__main__":
    raise SystemExit(main())
