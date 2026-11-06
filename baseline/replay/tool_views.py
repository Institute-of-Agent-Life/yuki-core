"""Freeze real v4 declarations and discovery functions without mutating the checkout."""
import argparse, hashlib, json, os, subprocess, sys, tempfile
from pathlib import Path
BASE='329ca7c487414d2f54ddd98ff4727446ed2446cd'
def main():
 p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
 source=a.source.resolve();out=a.out.resolve();out.mkdir(parents=True,exist_ok=False)
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip()==BASE
 assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
 sys.path[:0]=[str(source),str(source/'src')]
 from yuki.codemode.tool_visibility import DIRECT_TOOL_NAMES,LOOKUP_TOOLS,model_definitions,lookup_tools
 from yuki.codemode.api_projection import project
 from yuki.workspace.short_state import STATE_TOOL
 from yuki.automation.tools import AutomationToolService
 from yuki.domain.messages import ChatTool
 import pytest
 clock=next(t for t in AutomationToolService(None).definitions() if t.name=='time_get_current')
 wire=lambda t:dict(name=t.name,description=t.description,parameters=t.parameters)
 tools=(LOOKUP_TOOLS,STATE_TOOL,clock,ChatTool(name='get_group_members',description='Observed Straße group metadata '+'雪'*260,parameters={'type':'object'}),ChatTool(name='hidden_read',description='A hidden directory Straße GROUP',parameters={'type':'object','properties':{'k':{'type':'string'}}}),ChatTool(name='ext/工具',description='A non-identifier tool',parameters={'type':'object'}),ChatTool(name='execute_code',description='composition syntax',parameters={'type':'object'}))
 api=project(tools,'fixed-oracle-manifest')
 queries=[{}, {'limit':2},{'offset':2,'limit':2},{'offset':500},{'query':'GROUP STRASSE'}, {'query':'group\u001cstrasse'}, {'query':'group\u001fstrasse'}, {'query':'metadata   Straße'}, {'query':'no-match'}, {'name':'get_group_members'}, {'name':'hidden_read'}, {'name':'ext/工具'}, {'name':'execute_code'}, {'name':'lookup_tools'}, {'name':'absent'}, {'name':'hidden_read','offset':0}, {'limit':True}, {'limit':0},{'limit':11},{'offset':-1},{'query':'x'*129},{'name':None}, {'extra':1}, [], {'query':None}]
 cases=[]
 for enabled in (False,True):
  declared=frozenset(t.name for t in model_definitions(tools,enabled=enabled))
  for args in queries:cases.append({'code':enabled,'args':args,'result':json.loads(lookup_tools(api,json.dumps(args),declared_names=declared))})
 files=['src/yuki/codemode/tool_visibility.py','src/yuki/codemode/api_projection.py','src/yuki/workspace/short_state.py','src/yuki/automation/tools.py']
 proof={'source_commit':BASE,'source_files':{f:hashlib.sha256((source/f).read_bytes()).hexdigest() for f in files},'declarations':[wire(t) for t in (LOOKUP_TOOLS,STATE_TOOL,clock)],'direct_names':sorted(DIRECT_TOOL_NAMES),'fixture_tools':[wire(t) for t in tools],'cases':cases}
 os.chdir(source)
 with tempfile.TemporaryDirectory(prefix='yuki-original-tool-views-') as temp:
  code=pytest.main(['tests/integration/test_automation_tool_provider_contract.py','-p','no:cacheprovider','--basetemp',temp,'--junitxml',str(out/'junit.xml'),'-q'])
 proof['pytest_exit']=code
 (out/'original-tool-views.json').write_text(json.dumps(proof,ensure_ascii=False,indent=2)+'\n')
 assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
 return code
if __name__=='__main__':raise SystemExit(main())
