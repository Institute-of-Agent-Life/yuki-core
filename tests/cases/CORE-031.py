"""Install both distributed SDKs, cross their peers, then attack negotiation and the pin."""

import subprocess
import sys
from tests.harness.handshake import verify


def run(ctx):
    dist = ctx.output / "handshake-preview"
    command = [
        sys.executable,
        str(ctx.root / "ci/publish-contract.py"),
        "build",
        "--out",
        str(dist),
        "--verify-work",
        str(ctx.working / "installed"),
    ]
    process = subprocess.run(command, capture_output=True, text=True)
    if process.returncode:
        ctx.write(
            "package-error.json", {"stderr": process.stderr, "stdout": process.stdout}
        )
    ctx.equal("assemble real immutable distributions", process.returncode, 0)
    return verify(ctx, dist)
