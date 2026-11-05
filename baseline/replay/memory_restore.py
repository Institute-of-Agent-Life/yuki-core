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
 cases=['tests/unit/test_memory_mutation.py::test_merge_metadata_contest_invalidate_and_restore_operations','tests/unit/test_memory_operation_identity.py']
 with tempfile.TemporaryDirectory(prefix='yuki-original-memory-restore-') as temp:
  code=pytest.main([*cases,'-p','no:cacheprovider','--basetemp',temp,'--junitxml',str(out/'junit.xml'),'-q'])
 files=['src/yuki/memory/mutation/service.py','src/yuki/memory/service.py','src/yuki/memory/mutation/models.py']
 proof={'source_commit':BASE,'source_files':{f:hashlib.sha256((source/f).read_bytes()).hexdigest() for f in files},'cases':cases,'pytest_exit':code,'scope':'Original state restoration and immutable operation identity; not a Rust acceptance result'}
 (out/'original-memory-restore.json').write_text(json.dumps(proof,indent=2)+'\n')
 assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
 return code
if __name__=='__main__':raise SystemExit(main())
