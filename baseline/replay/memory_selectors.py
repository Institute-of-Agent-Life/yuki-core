"""Run exact original selector operations in isolated databases, without editing the legacy tree."""
import argparse,asyncio,hashlib,json,os,subprocess,sys,tempfile
from pathlib import Path
BASE='329ca7c487414d2f54ddd98ff4727446ed2446cd'

async def cases(directory):
 os.chdir(directory)
 from tests.conftest import _seed_database
 from tests.unit.test_memory_mutation import _service,_event,_context
 from yuki.persistence.database import Database
 from yuki.memory.mutation.models import MemoryMutationRequest
 from yuki.identity.write_settings import IdentityWriteSettings,configure_identity_write_settings,reset_identity_write_settings
 specs=[
  ('exact_key',[('tea','likes tea','drink','1001'),('coffee','likes coffee','drink','1001')],{'memory_key':'tea'}),
  ('normalized_key',[('tea key','likes tea','drink','1001')],{'memory_key':' tea\x00\tkey '}),
  ('exact_content',[('tea','Likes Tea','drink','1001')],{'old_content':'likes tea'}),
  ('original_case',[('tea','Likes Tea','drink','1001')],{'old_content':'Likes Tea'}),
  ('category',[('tea','tea one','first','1001'),('tea','tea two','second','1001')],{'memory_key':'tea','category':'second'}),
  ('ambiguous',[('tea','tea '+str(i),'drink','1001') for i in range(5)],{'memory_key':'tea'}),
  ('partial_content',[('internal','preferred jasmine tea','drink','1001')],{'old_content':'jasmine'}),
  ('literal_wildcards',[('x%_y','literal','drink','1001'),('xABCzy','other','drink','1001')],{'memory_key':'%_'}),
  ('foreign',[('private:cat','private cat','profile','2002')],{'memory_key':'private:cat'}),
 ]
 results=[]
 for index,(name,rows,selector) in enumerate(specs):
  reset_identity_write_settings();configure_identity_write_settings(IdentityWriteSettings())
  db=Database('sqlite+aiosqlite:///'+str(directory/(str(index)+'.db')))
  try:
   await db.create_schema();await _seed_database(db);service,facts,ledger,_=_service(db);created=[]
   for i,(key,content,category,person) in enumerate(rows):
    event=await _event(ledger,message_id='seed-'+str(i),sender_user_id=person,content=content)
    result=await service.mutate(MemoryMutationRequest(operation='create',target={'subject_ref':'current_speaker','scope_type':'person'},memory_key=key,new_content=content,category=category),_context(event))
    assert result.ok,result;created.append(result.new_fact_id)
   original_times=[(await facts.get_fact(id)).updated_at.isoformat() for id in created]
   event=await _event(ledger,message_id='selector',sender_user_id='1001',content='withdraw the exact original fact')
   result=await service.mutate(MemoryMutationRequest(operation='invalidate',selector=selector,target={'subject_ref':'current_speaker','scope_type':'person'}),_context(event))
   results.append({'name':name,'facts':[dict(memory_key=r[0],content=r[1],category=r[2],person=r[3],updated_at=original_times[i]) for i,r in enumerate(rows)],'selector':selector,'ok':result.ok,'reason_code':result.reason_code,'candidate_indices':[created.index(c.fact_id) for c in result.candidates],'candidate_rows':result.tool_payload()['candidates'],'states':[(await facts.get_fact(id)).status.value for id in created]})
  finally:await db.close();reset_identity_write_settings()
 return results

def main():
 p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args();source=a.source.resolve();out=a.out.resolve();out.mkdir(parents=True,exist_ok=False)
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip()==BASE
 assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
 sys.path[:0]=[str(source),str(source/'src')];os.environ['SOUL_FILE']=str(source/'config/SOUL.md');os.chdir(source)
 with tempfile.TemporaryDirectory(prefix='yuki-selector-oracle-') as temp:results=asyncio.run(cases(Path(temp)))
 files=['src/yuki/memory/mutation/service.py','src/yuki/memory/mutation/models.py','src/yuki/memory/repository.py','src/yuki/memory/validation.py']
 proof={'source_commit':BASE,'source_files':{f:hashlib.sha256((source/f).read_bytes()).hexdigest() for f in files},'cases':results,'scope':'Actual original mutation selector outcomes in independent data directories; not Rust acceptance'}
 (out/'original-memory-selectors.json').write_text(json.dumps(proof,indent=2,ensure_ascii=False)+'\n')
 assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
 print(json.dumps({'cases':len(results),'out':str(out)}))
if __name__=='__main__':main()
