"""Run fixed original connector identity cases using isolated fixture databases."""
import argparse,hashlib,json,os,subprocess,sys,tempfile
from pathlib import Path
BASE='329ca7c487414d2f54ddd98ff4727446ed2446cd'

def main():
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
    source=a.source.resolve();out=a.out.resolve();out.mkdir(parents=True,exist_ok=False)
    assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip()==BASE
    assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
    sys.path[:0]=[str(source),str(source/'src')];os.chdir(source)
    import pytest
    cases=['tests/unit/test_connector_identity.py']
    with tempfile.TemporaryDirectory(prefix='yuki-connector-oracle-') as temp:
        code=pytest.main([*cases,'-p','no:cacheprovider','--basetemp',temp,'--junitxml',str(out/'junit.xml'),'-q'])
    files=['src/yuki/identity/credentials.py','src/yuki/external_agents/identity.py','src/yuki/external_agents/repository.py','tests/unit/test_connector_identity.py']
    proof={'source_commit':BASE,'source_files':{f:hashlib.sha256((source/f).read_bytes()).hexdigest() for f in files},'cases':cases,'pytest_exit':code,'scope':'Original purpose, Person, account transfer, merge, revoke and accountless credential behavior; not a Rust or extension acceptance result'}
    (out/'original-connector-identity.json').write_text(json.dumps(proof,indent=2)+'\n')
    assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
    return code
if __name__=='__main__':raise SystemExit(main())
