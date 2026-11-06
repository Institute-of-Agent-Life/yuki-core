"""Install both immutable SDK distributions, then compile an external Rust consumer."""
import hashlib
import json
import os
import subprocess
import sys
import tomllib


def prepare(ctx):
    dist=ctx.output/'sdk-distributions'
    installed=ctx.working/'control-sdk'
    built=subprocess.run([sys.executable,str(ctx.root/'ci/publish-contract.py'),'build','--out',str(dist),'--verify-work',str(installed)],capture_output=True,text=True)
    if built.returncode: ctx.write('sdk-build-failure.json',{'stdout':built.stdout,'stderr':built.stderr})
    ctx.equal('actual Python wheel and Rust crate verify independently',built.returncode,0)
    consumer=ctx.working/'control-rust-consumer';(consumer/'src').mkdir(parents=True)
    crate=next((installed/'rust-install').glob('yuki-plugin-sdk-*'))
    (consumer/'Cargo.toml').write_text('[package]\nname="control-consumer"\nversion="0.0.0"\nedition="2024"\n[dependencies]\nserde_json="=1.0.145"\nyuki-plugin-sdk={path='+json.dumps(str(crate))+'}\n')
    (consumer/'src/main.rs').write_bytes((ctx.root/'tests/peers/control/rust.rs').read_bytes())
    for command in [['cargo','generate-lockfile','--offline'],['cargo','build','--locked','--offline','--target-dir',str(installed/'rust-target')]]:
        process=subprocess.run(command,cwd=consumer,capture_output=True,text=True,env={**os.environ,"RUSTUP_TOOLCHAIN":tomllib.loads((ctx.root/"rust-toolchain.toml").read_text())["toolchain"]["channel"]})
        if process.returncode: ctx.write('rust-control-consumer-failure.json',{'stdout':process.stdout,'stderr':process.stderr})
        ctx.equal('external Rust management consumer compiles against candidate crate',process.returncode,0)
    ctx.control_python_sdk=installed/'python-install'
    ctx.control_rust_sdk=installed/'rust-target/debug'/('control-consumer.exe' if os.name=='nt' else 'control-consumer')
    return {'python_install':'verified wheel','rust_install':'verified crate','rust_consumer_sha256':hashlib.sha256(ctx.control_rust_sdk.read_bytes()).hexdigest(),
            'candidate':json.loads(built.stdout),'external_extension_consumption':False}
