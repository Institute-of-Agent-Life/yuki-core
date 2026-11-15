"""Replay the original 1.2 consumers against the selected Windows Host."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory
from zipfile import ZipFile

from participation_replay import sha
from selected_participation import RELEASES, download

FIXTURE_SHA256 = "a4ce8bc82336a47258839da215a62e084db537a007b95bc8090d37f2864cd89e"
CASE_SOURCE = Path(__file__).resolve().parent / "fixtures/core-053-windows-previous-source.zip"


def extract(archive, directory, names):
    directory.mkdir(parents=True, exist_ok=True)
    for name in names:
        path = (directory / name).resolve()
        if not path.is_relative_to(directory.resolve()):
            raise ValueError("Artifact member escapes the replay directory")
        path.parent.mkdir(parents=True, exist_ok=True)
        with archive.open(name) as source, path.open("xb") as output:
            shutil.copyfileobj(source, output)


def original_consumers(package, inputs):
    root = "evidence/platforms/windows/"
    consumers = {
        "feishu/yuki-feishu-0.1.0-windows.zip": "feishu.zip",
        "memory-maintenance/windows-quality-consumer.zip": "quality.zip",
    }
    for name, output in consumers.items():
        (inputs / output).write_bytes(package.read(root + name))
    previous = json.loads((inputs / "previous.json").read_bytes())
    for unit in ("telegram", "web-fetch"):
        directory = inputs / "base" / unit
        archive = inputs / (unit + ".zip")
        archive.write_bytes(package.read(root + f"{unit}/{unit}-original-1.0.zip"))
        with ZipFile(archive) as original:
            extract(original, directory, previous["pin"]["packages"][unit])


def assemble(release, inputs):
    if sha(CASE_SOURCE) != FIXTURE_SHA256 or sha(release) != RELEASES["1.2.0"][1]:
        raise ValueError("Previous consumer inputs differ from their pins")
    with ZipFile(CASE_SOURCE) as fixture, ZipFile(release) as package:
        extract(fixture, inputs, fixture.namelist())
        original_consumers(package, inputs)
    return json.loads((inputs / "bindings.json").read_bytes())


def fetch_wheels(inputs, bindings):
    wheels = inputs / "wheels"
    requirements = inputs / "wheel-requirements.txt"
    with requirements.open("x", encoding="utf-8") as manifest:
        for requirement, wheel in (
            ("protobuf==7.36.2", "protobuf-7.36.2-py3-none-any.whl"),
            ("websockets==16.1.1", "websockets-16.1.1-py3-none-any.whl"),
        ):
            manifest.write(f"{requirement} --hash=sha256:{bindings['files']['wheels/' + wheel]}\n")
    subprocess.run([sys.executable, "-m", "pip", "download", "--disable-pip-version-check",
                    "--require-hashes", "--no-deps", "--only-binary=:all:", "--dest", wheels,
                    "-r", requirements], check=True, timeout=180)


def replay(candidate, expected_sha256, core_revision, output):
    if sha(candidate) != expected_sha256:
        raise ValueError("Selected candidate archive differs")
    with TemporaryDirectory(prefix="ys-") as temporary:
        work = Path(temporary).resolve()
        release = work / "previous.zip"
        download("1.2.0", "yuki-plugin-base-1.2.0.zip", release, RELEASES["1.2.0"][1])
        inputs = work / "inputs"
        bindings = assemble(release, inputs)
        fetch_wheels(inputs, bindings)
        current = work / "candidate"
        with ZipFile(candidate) as archive:
            files = [name for name in archive.namelist()
                     if name.startswith(("sdk/", "windows/")) and not name.endswith("/")]
            extract(archive, current, files)
        build = current / "windows/host-build.json"
        data = json.loads(build.read_bytes())
        if data["source"]["commit"] != core_revision or data["published"] or data["synthetic"]:
            raise ValueError("Wrong selected candidate source")
        if data["profile"]["version"] not in {"1.3.0", "1.4.0"}:
            raise ValueError("Expected a selected participation version")
        driver = inputs / "replay-selected-maintenance.py"
        code = driver.read_text(encoding="utf-8")
        original = "        root = args.inputs / 'ext'\n"
        if code.count(original) != 1:
            raise ValueError("Unexpected prior consumer harness")
        isolated = (
            '        root = scratch / "replay-harness"\n'
            '        shutil.copytree(args.inputs / "ext", root)\n'
            '        peers = root / "tests/peers"\n'
            '        shutil.copyfile(peers / "observe_quality_stdio.py", peers / "observe_stdio.py")\n'
        )
        runner = inputs / "observed_driver.py"
        runner.write_text(code.replace(original, isolated), encoding="utf-8")
        output.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run([sys.executable, "-B", runner, "--system", "windows", "--host-build", build,
                        "--kit", current / "sdk", "--inputs", inputs, "--tmp", work, "--out", output],
                       env=dict(os.environ, PYTHONUTF8="1", UV_OFFLINE="true"), check=True, timeout=420)
    result = json.loads((output / "result.json").read_bytes())
    if result["status"] != "PASS" or result["paid_calls"] != 0:
        raise ValueError("Previous stable consumers did not pass")
    print(result["candidate_source"], result["artifacts"]["host"], sha(output / "result.json"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--candidate-sha256", required=True)
    parser.add_argument("--core-revision", required=True)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    replay(args.candidate.resolve(), args.candidate_sha256, args.core_revision, args.out.resolve())


if __name__ == "__main__":
    main()
