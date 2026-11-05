"""Extract original permission metadata and projections from the fixed v4 checkout."""
import argparse,ast,hashlib,json,os,subprocess,sys,tempfile
from pathlib import Path
BASE='329ca7c487414d2f54ddd98ff4727446ed2446cd'
def main():
 p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
 source=a.source.resolve();out=a.out.resolve();out.mkdir(parents=True,exist_ok=False)
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip()==BASE
 assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
 sys.path[:0]=[str(source),str(source/'src')]
 from yuki.admin.permission_catalog import PermissionCatalogService
 from yuki.services import agent_tools
 from yuki.domain.tool_actor import ToolActor
 from yuki.runtime.origin import TurnOrigin
 import pytest
 declaration_file=source/'src/yuki/services/agent_tools.py'
 tree=ast.parse(declaration_file.read_text())
 nodes=[n for n in ast.walk(tree) if isinstance(n,ast.Call) and any(k.arg=='name' and isinstance(k.value,ast.Constant) and k.value.value=='get_my_capabilities' for k in n.keywords)]
 assert len(nodes)==1
 declaration=eval(compile(ast.Expression(nodes[0]),str(declaration_file),'eval'),vars(agent_tools))
 catalog=PermissionCatalogService()
 actor=ToolActor(user_id='fixture',bot_user_id='bot',group_id=None,origin=TurnOrigin.USER_MESSAGE,instruction='',event_id=1,person_id='00000000-0000-4000-8000-000000000037')
 cases=[]
 for admin in [False,True]:
  for args in [{},{'mode':'full'},{'mode':'focused','category':'memory'},{'mode':'focused','category':' MEMORY '},{'mode':'focused','query':'SOUL'},{'mode':'focused','query':'不存在的目录项'},{'mode':'focused','category':'missing'},{'mode':'focused','query':'   '}]:
   mode,category,query=agent_tools.AgentToolService._capability_options(args)
   result=catalog.report_for_actor(actor,category=category,query=query,actor_is_superuser=admin).to_model_dict(mode)
   cases.append({'admin':admin,'args':args,'result':result})
 descriptors=[dict(d.to_dict(),search_terms=list(d.search_terms),minimum_rank=int(d.minimum_level)) for d in catalog._descriptors]
 originals=['src/yuki/admin/permission_catalog.py','src/yuki/admin/config_registry.py','src/yuki/services/commands/help.py','src/yuki/services/agent_tools.py']
 proof={'source_commit':BASE,'source_files':{f:hashlib.sha256((source/f).read_bytes()).hexdigest() for f in originals},'declaration':{'name':declaration.name,'description':declaration.description,'parameters':declaration.parameters},'descriptors':descriptors,'cases':cases}
 os.chdir(source)
 with tempfile.TemporaryDirectory(prefix='yuki-original-permissions-') as tmp:
  code=pytest.main(['tests/unit/test_chat_admin_refactor.py','-k','permission_directory','-p','no:cacheprovider','--basetemp',tmp,'--junitxml',str(out/'junit.xml'),'-q'])
 proof['pytest_exit']=code
 (out/'original-permissions.json').write_text(json.dumps(proof,ensure_ascii=False,indent=2)+'\n')
 assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
 return code
if __name__=='__main__':raise SystemExit(main())
