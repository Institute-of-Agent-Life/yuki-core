"""Record a test installation's real child; optionally omit one negotiated test method."""

import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time


def main():
    program, log_name, workspace, fault = sys.argv[1:]
    log = Path(log_name)
    environment = dict(os.environ)
    if fault in {"unsupported-version", "missing-required-field"}:
        environment["YUKI_TELEGRAM_TOKEN"] = "101:" + fault
    child = subprocess.Popen(
        [program], stdin=subprocess.PIPE, stdout=subprocess.PIPE, cwd=workspace,
        env=environment,
    )
    lock = threading.Lock()
    killed_for_test = threading.Event()

    def record(row):
        with lock, log.open("a", encoding="utf-8") as target:
            print(json.dumps(row), file=target)

    def copy(stream, sink, direction):
        try:
            for line in stream:
                frame = json.loads(line)
                result = frame.get("result", {})
                if (
                    fault == "unsupported-version"
                    and direction == "from_plugin"
                    and frame.get("method") == "initialize"
                ):
                    record({"direction": "before_fault", "frame": frame})
                    frame["params"]["versions"] = {
                        "min": {"major": 0, "minor": 0},
                        "max": {"major": 0, "minor": 0},
                    }
                    line = (json.dumps(frame) + "\n").encode()
                if (
                    fault == "missing-required-field"
                    and direction == "from_host"
                    and isinstance(result, dict)
                    and "namespace" in result
                ):
                    record({"direction": "before_fault", "frame": frame})
                    del result["namespace"]
                    line = (json.dumps(frame) + "\n").encode()
                if (
                    fault == "missing-method"
                    and direction == "from_host"
                    and isinstance(result, dict)
                    and any(
                        m["name"] == "gateway.dispatch_part"
                        for m in result.get("method_revisions", [])
                    )
                ):
                    record({"direction": "before_fault", "frame": frame})
                    result["method_revisions"] = [
                        m
                        for m in result["method_revisions"]
                        if m["name"] != "gateway.dispatch_part"
                    ]
                    line = (json.dumps(frame) + "\n").encode()
                record({"direction": direction, "frame": frame})
                sink.write(line)
                sink.flush()
        except (BrokenPipeError, OSError):
            pass

    def feed():
        copy(sys.stdin.buffer, child.stdin, "from_host")
        child.stdin.close()

    def kill_on_request():
        request = log.with_suffix(".kill")
        while child.poll() is None:
            if request.exists():
                request.unlink()
                killed_for_test.set()
                child.kill()
                return
            time.sleep(0.04)

    threading.Thread(target=feed, daemon=True).start()
    threading.Thread(target=kill_on_request, daemon=True).start()
    try:
        copy(child.stdout, sys.stdout.buffer, "from_plugin")
    finally:
        if child.poll() is None:
            child.kill()
        code = child.wait()
        if killed_for_test.is_set():
            assert code != 0
            tmp = log.with_suffix(".pid-tmp")
            tmp.write_text(str(child.pid), encoding="ascii")
            tmp.replace(log.with_suffix(".killed"))


if __name__ == "__main__":
    main()
