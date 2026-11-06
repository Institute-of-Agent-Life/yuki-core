"""Run original recall scenarios against a fixed read-only checkout."""
import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

BASE = '329ca7c487414d2f54ddd98ff4727446ed2446cd'

def main():
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
    source=a.source.resolve();out=a.out.resolve();out.mkdir(parents=True,exist_ok=False)
    assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip()==BASE
    assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
    sys.path[:0]=[str(source),str(source/'src')]
    from yuki.social.service import SocialService
    from yuki.social.tools import social_tool_definitions
    import pytest
    original=SocialService.execute;records=[]
    async def capture(self,name,args,context,*rest,**kw):
        result=await original(self,name,args,context,*rest,**kw)
        if name=='recall_own_message':
            records.append({'args':args,'conversation_id':context.conversation_id,'space_id':context.space_id,'result':result})
        return result
    SocialService.execute=capture
    os.chdir(source)
    with tempfile.TemporaryDirectory(prefix='yuki-original-recall-') as tmp:
        code=pytest.main(['tests/unit/test_social_identity_contracts.py','-k','recall','-p','no:cacheprovider','--basetemp',tmp,'--junitxml',str(out/'junit.xml'),'-q'])
    definitions=[{'name':t.name,'description':t.description,'parameters':t.parameters} for t in social_tool_definitions() if t.name=='recall_own_message']
    (out/'original-recall.json').write_text(json.dumps({'source_commit':BASE,'pytest_exit':code,'declarations':definitions,'original_results':records},ensure_ascii=False,indent=2)+'\n')
    assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
    return code

if __name__=='__main__':raise SystemExit(main())
