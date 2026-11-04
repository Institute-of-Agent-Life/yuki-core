"""Isolated case process: environment is provided by ci/check.py."""

import argparse
import importlib.util
import json
import sys
import tempfile
import traceback
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))

from tests.harness import CaseFailure, Context
from tests.harness.network import install


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("card")
    parser.add_argument("output", type=Path)
    parser.add_argument("--fault")
    args = parser.parse_args()
    lock = json.loads((ROOT / "ci/checks.json").read_text(encoding="utf-8"))
    ctx = Context(ROOT, args.output, lock, args.fault)
    result = {"assertions": ctx.assertions, "failure": None}
    install()
    try:
        with tempfile.TemporaryDirectory(prefix="yuki-card-") as working:
            ctx.working = Path(working)
            spec = importlib.util.spec_from_file_location("card_case", ROOT / lock["cards"][args.card]["case"])
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            artifact = module.run(ctx)
            if not ctx.assertions:
                raise CaseFailure("empty case")
            ctx.write(lock["cards"][args.card]["artifact"], artifact)
    except Exception as error:
        result["failure"] = str(error)
        (args.output / "failure.txt").write_text(traceback.format_exc(), encoding="utf-8")
    ctx.write("case-result.json", result)
    return 1 if result["failure"] else 0


if __name__ == "__main__":
    sys.exit(main())
