"""Ship a hashed source snapshot to the authorized SSH test machine."""

import argparse
import base64
import json
import subprocess
import sys
import uuid
import zipfile
from pathlib import Path

from source import digest, source_identity

ROOT = Path(__file__).resolve().parents[1]


def remote(host, script, capture=False):
    encoded = base64.b64encode(script.encode()).decode()
    command = 'python -c "import base64;exec(base64.b64decode(\'' + encoded + '\'))"'
    return subprocess.run(["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8",
                           host, command], check=True, capture_output=capture, text=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("card")
    parser.add_argument("--host", default="hjw")
    parser.add_argument("--host-candidate",action="store_true",help="also package the exact verified host or personal candidate")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--fault", choices=["swap", "empty"])
    args = parser.parse_args()
    out = args.out.resolve()
    if out.exists():
        parser.exit(2, "output exists; choose a new attempt\n")
    if args.host_candidate and args.card not in {"CORE-032","CORE-033","CORE-038","CORE-050"}:parser.exit(2,"host candidate requires CORE-032, CORE-033, CORE-038 or CORE-050\n")
    source = source_identity(ROOT)
    run_id = args.card + "-" + source["tree_sha256"][:12] + "-" + uuid.uuid4().hex[:8]
    cache = ROOT / ".cache/windows" / run_id
    cache.mkdir(parents=True, exist_ok=False)
    archive = cache / "source.zip"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as bundle:
        for name in source["files"]:
            bundle.write(ROOT / name, name)
        bundle.writestr(".yuki-source.json", json.dumps(source, ensure_ascii=False))
    incoming = run_id + ".zip"
    subprocess.run(["scp", "-q", str(archive), args.host + ":" + incoming], check=True)
    script = f'''
from pathlib import Path
import hashlib, json, subprocess, sys, zipfile
archive=Path.home()/{incoming!r}
assert hashlib.sha256(archive.read_bytes()).hexdigest()=={digest(archive.read_bytes())!r}
profile_file=Path.home()/'yuki-core-tooling/environment.json'
profile=json.loads(profile_file.read_text(encoding='utf-8')) if profile_file.is_file() else {{}}
root=Path(profile.get('workspace',str(Path.home()/'yuki-core-validation')))/{run_id!r}
root.mkdir(parents=True,exist_ok=False)
with zipfile.ZipFile(archive) as z:
    for member in z.infolist():
        assert (root/member.filename).resolve().is_relative_to(root.resolve())
    z.extractall(root)
sys.path.insert(0,str(root/'ci'))
from source import source_identity
source_identity(root)
command=[sys.executable,'ci/check.py',{args.card!r},'--out','artifacts/run']
if {args.fault!r}: command.extend(['--fault',{args.fault!r}])
import os
env=dict(os.environ)
temporary=root/'.cache'/'test-temp'
temporary.mkdir(parents=True,exist_ok=True)
env.update(TEMP=str(temporary),TMP=str(temporary))
if profile.get('cargo_home'):
    env.update(CARGO_HOME=profile['cargo_home'],RUSTUP_HOME=profile['rustup_home'])
    env['PATH']=str(Path(profile['cargo_home'])/'bin')+os.pathsep+env['PATH']
if profile.get('mingw_bin'):
    env['YUKI_MINGW_BIN']=profile['mingw_bin']
    env['PATH']=profile['mingw_bin']+os.pathsep+env['PATH']
r=subprocess.run(command,cwd=root,env=env)
if r.returncode==0 and {args.host_candidate!r}:
    pack=[sys.executable,'ci/host_candidate.py','--manifest','artifacts/run/manifest.json','--out','artifacts/run/handoff']
    if {source['dirty']!r}: pack.append('--development')
    subprocess.run(pack,cwd=root,env=env,check=True)
with zipfile.ZipFile(Path.home()/{(run_id+'-artifacts.zip')!r},'w',zipfile.ZIP_DEFLATED) as z:
    for file in sorted((root/'artifacts/run').rglob('*')):
        if file.is_file(): z.write(file,file.relative_to(root/'artifacts/run'))
print(json.dumps({{'windows_root':str(root),'exit_code':r.returncode}}))
'''
    remote(args.host, script)
    result_zip = cache / "artifacts.zip"
    subprocess.run(["scp", "-q", args.host + ":" + run_id + "-artifacts.zip",
                    str(result_zip)], check=True)
    out.mkdir(parents=True, exist_ok=False)
    with zipfile.ZipFile(result_zip) as bundle:
        for member in bundle.infolist():
            if not (out / member.filename).resolve().is_relative_to(out):
                raise ValueError("artifact path escapes output")
        bundle.extractall(out)
    manifest = json.loads((out / "manifest.json").read_text(encoding="utf-8"))
    print(json.dumps({"out": str(out), "status": manifest["status"],
                      "source_commit": source["commit"], "source_tree": source["tree_sha256"]}))
    return manifest["exit_code"]


if __name__ == "__main__":
    sys.exit(main())
