"""Kill the compiled Rust test worker after it signals an actual SQLite commit barrier."""

import os
import subprocess
import time


def kill_writer(ctx, data, child_test, phase, variables):
    executable = (ctx.output / "rust-test-executable.txt").read_text().strip()
    marker = ctx.working / (child_test + "-" + phase + ".ready")
    env = {
        **os.environ,
        **variables,
        "YUKI_WRITER_TEST_DATA": str(data),
        "YUKI_WRITER_TEST_PHASE": phase,
        "YUKI_WRITER_TEST_MARKER": str(marker),
    }
    with (ctx.output / (child_test + "-" + phase + ".log")).open("w") as log:
        process = subprocess.Popen(
            [
                executable,
                "--exact",
                "--ignored",
                "--nocapture",
                "store::tests::" + child_test,
            ],
            env=env,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        try:
            deadline = time.monotonic() + 20
            while (
                time.monotonic() < deadline
                and not marker.exists()
                and process.poll() is None
            ):
                time.sleep(0.01)
            ctx.check(
                phase + " reached SQLite commit barrier",
                marker.exists() and process.poll() is None,
            )
            process.kill()
            ctx.check("abnormal process termination", process.wait(timeout=8) != 0)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=8)
