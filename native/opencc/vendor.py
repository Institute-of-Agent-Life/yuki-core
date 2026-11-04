"""Rebuild the t2s-only native dependency from the two distributions pinned in the v4 uv.lock."""

import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import tarfile
import zipfile

SOURCE_SHA = "47977905f131d7d9cfcec29fba5d841154907e1da73103105a4a68744e0f4f1a"
WHEEL_SHA = "7025dc276b2a60b30ed3aefb99f1ceb8616076fd3eb3310c0f8f2046e79e76b1"
RESOURCES = (
    "CJK_Compatibility_Ideographs.ocd2",
    "TSPhrases.ocd2",
    "TSCharactersExt.ocd2",
    "TSCharacters.ocd2",
    "t2s.json",
)


def package(sdist, wheel, out):
    if (
        hashlib.sha256(sdist.read_bytes()).hexdigest() != SOURCE_SHA
        or hashlib.sha256(wheel.read_bytes()).hexdigest() != WHEEL_SHA
    ):
        raise ValueError("unexpected OpenCC distribution")
    files = {}
    with tarfile.open(sdist) as source:
        for m in source:
            p = PurePosixPath(m.name).relative_to("opencc-1.4.2")
            name = str(p)
            is_header = p.suffix in {".h", ".hpp"}
            regional = name.startswith(
                (
                    "src/plugin/",
                    "deps/marisa-0.3.1/include/",
                    "deps/marisa-0.3.1/lib/",
                    "deps/darts-clone-0.32h/include/",
                    "deps/rapidjson-1.1.0/rapidjson/",
                )
            )
            license_file = p.name.lower().startswith(
                ("copyright", "license", "copying", "authors", "notice")
            ) and (
                len(p.parts) == 1
                or name.startswith(("deps/marisa-", "deps/darts-", "deps/rapidjson-"))
            )
            core = (
                p.parent == PurePosixPath("src")
                and (is_header or p.suffix in (".cpp", ".inc"))
                and "Test" not in p.stem
                and p.stem != "py_opencc"
            )
            if m.isfile() and (
                core or regional and (is_header or p.suffix == ".cc") or license_file
            ):
                files[name] = source.extractfile(m).read()
    with zipfile.ZipFile(wheel) as source:
        for name in RESOURCES:
            files["resources/" + name] = source.read("opencc/clib/share/opencc/" + name)
    notices = [
        (n, d)
        for n, d in sorted(files.items())
        if Path(n)
        .name.lower()
        .startswith(("copyright", "license", "copying", "notice"))
    ]
    (out / "LICENSES.txt").write_text(
        "\n\n".join(n + "\n" + d.decode("utf-8") for n, d in notices).rstrip("\n") + "\n"
    )
    archive = out / "opencc-1.4.2-t2s.tar.gz"
    with (
        archive.open("wb") as writer,
        gzip.GzipFile(filename="", fileobj=writer, mode="wb", mtime=0) as compressed,
        tarfile.open(
            fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT
        ) as bundle,
    ):
        for name, data in sorted(files.items()):
            info = tarfile.TarInfo(name)
            info.mode = 0o644
            info.size = len(data)
            bundle.addfile(info, io.BytesIO(data))
    (out / "archive.sha256").write_text(
        hashlib.sha256(archive.read_bytes()).hexdigest() + "\n"
    )
    (out / "provenance.json").write_text(
        json.dumps(
            {
                "version": "1.4.2",
                "source_url": "https://files.pythonhosted.org/packages/e0/12/09e62f051af1de7ca84be1d69154bd0514416ef72237e2081584cc268bc9/opencc-1.4.2.tar.gz",
                "source_sha256": SOURCE_SHA,
                "resource_wheel": "opencc-1.4.2-cp312-cp312-win_amd64.whl",
                "resource_wheel_sha256": WHEEL_SHA,
                "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
                "resource_hashes": {
                    n: hashlib.sha256(files["resources/" + n]).hexdigest()
                    for n in RESOURCES
                },
                "baseline_commit": "329ca7c487414d2f54ddd98ff4727446ed2446cd",
            },
            indent=2,
        )
        + "\n"
    )


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("sdist", type=Path)
    p.add_argument("wheel", type=Path)
    args = p.parse_args()
    package(args.sdist, args.wheel, Path(__file__).resolve().parent)
