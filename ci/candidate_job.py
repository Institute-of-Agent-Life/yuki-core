"""Sign a clean verified candidate with a disposable CI key or an operator-supplied key."""

import argparse
import pathlib
import shutil
import subprocess
import tempfile
from dist import signed_native
from oci_candidate import build


def main():
    p = argparse.ArgumentParser()
    p.add_argument("target", choices=["native", "oci"])
    p.add_argument("--manifest", type=pathlib.Path)
    p.add_argument("--out", type=pathlib.Path, required=True)
    p.add_argument("--key", type=pathlib.Path)
    a = p.parse_args()
    with tempfile.TemporaryDirectory() as tmp:
        key = a.key or pathlib.Path(tmp) / "ephemeral-candidate"
        if not a.key:
            subprocess.run(
                ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)],
                check=True,
            )
        if a.target == "oci":
            build(a.out, key)
        else:
            if a.manifest is None:
                p.error("native requires the CORE-039 verification manifest")
            signed_native(a.manifest, a.out, key)
        pub = (
            subprocess.check_output(["ssh-keygen", "-y", "-f", str(key)]).strip()
            + b"\n"
        )
        (a.out / "signer.pub").write_bytes(pub)
        # Keep the CLI verifier separately available before any untrusted archive is opened.
        if a.target == "oci":
            shutil.copyfile(
                pathlib.Path(__file__).with_name("verify_oci.py"),
                a.out / "verify_oci.py",
            )
        shutil.copyfile(
            pathlib.Path(__file__).with_name("verify_dist.py"), a.out / "verify_dist.py"
        )


if __name__ == "__main__":
    main()
