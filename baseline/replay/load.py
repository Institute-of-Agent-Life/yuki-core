"""Run using the isolated v4 interpreter. Refuse a reused artifact directory."""
import argparse
import json
import os
import platform
import sys
import time
from pathlib import Path

from record import SPEC, git, offline
import pytest

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--smoke", action="store_true")
    args = parser.parse_args()
    source, out = args.source.resolve(), args.out.resolve()
    assert git(source, "rev-parse", "HEAD") == SPEC["source_commit"]
    assert git(source, "rev-parse", "HEAD^{tree}") == SPEC["source_tree"]
    assert not git(source, "diff", "HEAD", "--name-only")
    out.mkdir(parents=True, exist_ok=False)
    os.environ.update(YUKI_ORACLE_OUT=str(out), YUKI_ORACLE_SMOKE=str(int(args.smoke)))
    os.chdir(source)
    sys.path[:0] = [str(source), str(source / "src"), str(source / "packages/yuki-agent-connector/src")]
    offline()
    started, cpu = time.perf_counter(), time.process_time()
    code = pytest.main(["-q", "--maxfail=1", "-c", str(source / "pyproject.toml"),
                        "--junitxml", str(out / "junit.xml"), str(Path(__file__).with_name("load_cases.py"))])
    (out / "run.json").write_text(json.dumps({"line": "A", "source_commit": SPEC["source_commit"],
        "platform": platform.platform(), "smoke": args.smoke, "exit_code": int(code),
        "elapsed_seconds": time.perf_counter() - started, "process_cpu_seconds": time.process_time() - cpu,
        "concurrency": 1, "seed": SPEC["seed"], "paid_calls": 0, "model": "v4 FakeLLMProvider"}, indent=2))
    return code

if __name__ == "__main__":
    sys.exit(main())
