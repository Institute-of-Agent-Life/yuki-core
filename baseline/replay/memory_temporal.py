"""Original temporal parser and real mutation outcomes, in independent data directories."""
import argparse,asyncio,hashlib,json,os,subprocess,sys,tempfile
from datetime import datetime,UTC
from pathlib import Path
BASE='329ca7c487414d2f54ddd98ff4727446ed2446cd'

def vectors():
 from yuki.memory.temporal import MemoryTemporalResolver
 from yuki.memory.enums import MemoryTemporalMode
 values=[None,'','  ','2032-02-29','20320229','2032-W09-7','2032-02-29T12','2032-02-29 12:34','20320229T123456','2032-02-29T12:34:56.123456','2032-02-29T12:34:56,123456','2032-02-29T12:34:56Z','2032-02-29T12:34:56+08:00','2032-02-29T12:34:56-04:30','2032-02-29T12:34:56+08:00:12','2032-02-29T12:34:56.1234567','2031-02-29','2032-02-29T24:00:00','2032-02-29T12:34:60','tomorrow']
 values += ['2032W097','2032-W09','2032W09','2032-02-29☀12:34:56','2032-02-29T1234','2032-02-29T12.5','2032-02-29T12:3456','2032-02-29T1234:56','0000-01-01','0001-01-01T00:00:00+08:00','9999-12-31T23:00:00-08:00','2032-02-29T12:34:56+00:60','2032-02-29T12:34:56+01:00.5']
 pairs=[('Asia/Shanghai',v,None) for v in values]+[('Asia/Shanghai',None,v) for v in values]
 pairs += [('America/New_York',v,None) for v in ['2032-03-14T02:30:00','2032-11-07T01:30:00']]
 pairs += [('Australia/Lord_Howe','2032-10-03T02:15:00',None),('Pacific/Apia','2011-12-30T12:00:00',None),('UTC','2032-03-01','2032-02-29'),('UTC','2032-02-29','2032-02-29')]
 rows=[]
 for zone,start,end in pairs:
  item={'zone':zone,'valid_from':start,'valid_until':end}
  try:
   value=MemoryTemporalResolver().resolve(mode=MemoryTemporalMode.TEMPORARY if end is not None else MemoryTemporalMode.PERSISTENT,valid_from=start,valid_until=end,occurred_at=datetime(2030,1,1,tzinfo=UTC),timezone_name=zone)
   item.update(ok=True,mode=value.mode.value,start=value.valid_from.isoformat() if value.valid_from else None,end=value.valid_until.isoformat() if value.valid_until else None)
  except (ValueError,OverflowError) as exc:item.update(ok=False,error=str(exc))
  rows.append(item)
 return rows

async def mutations(directory):
 os.chdir(directory)
 from tests.conftest import _seed_database
 from tests.unit.test_memory_mutation import _service,_event,_context
 from yuki.persistence.database import Database
 from yuki.memory.mutation.models import MemoryMutationRequest
 from yuki.identity.write_settings import IdentityWriteSettings,configure_identity_write_settings,reset_identity_write_settings
 reset_identity_write_settings();configure_identity_write_settings(IdentityWriteSettings())
 db=Database('sqlite+aiosqlite:///'+str(directory/'original.db'))
 rows=[]
 try:
  await db.create_schema();await _seed_database(db);service,facts,ledger,_=_service(db)
  fact=None
  steps=[('create',{'valid_from':'2032-01-01','valid_until':'2032-12-31'}),('update_metadata',{'category':'updated'}),('update_metadata',{'valid_from':'2033-01-01'}),('update_metadata',{'valid_until':'2034-12-31'}),('correct',{'new_content':'corrected tea'}),('correct',{'new_content':'dated tea','valid_from':'2035-01-01','valid_until':'2035-12-31'}),('invalidate',{}),('restore',{})]
  for index,(op,fields) in enumerate(steps):
   event=await _event(ledger,message_id='temporal-'+str(index),sender_user_id='1001',content='Please remember and update my tea dates')
   args={'operation':op,**fields}
   if fact is None:args.update(target={'subject_ref':'current_speaker','scope_type':'person'},memory_key='tea',new_content='tea dates',category='drink')
   else:args['fact_id']=fact
   result=await service.mutate(MemoryMutationRequest(**args),_context(event));assert result.ok,result
   fact=result.new_fact_id or fact;value=await facts.get_fact(fact)
   rows.append({'operation':op,'fields':fields,'start':value.valid_from.isoformat() if value.valid_from else None,'end':value.valid_until.isoformat() if value.valid_until else None,'status':value.status.value})
 finally:await db.close();reset_identity_write_settings()
 return rows

def main():
 p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args();source=a.source.resolve();out=a.out.resolve();out.mkdir(parents=True,exist_ok=False)
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip()==BASE
 assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
 sys.path[:0]=[str(source),str(source/'src')];os.environ['SOUL_FILE']=str(source/'config/SOUL.md')
 with tempfile.TemporaryDirectory(prefix='yuki-time-oracle-') as temp:
  os.chdir(temp);cases=vectors();operations=asyncio.run(mutations(Path(temp)))
 files=['src/yuki/memory/temporal.py','src/yuki/memory/mutation/service.py','src/yuki/memory/mutation/models.py']
 proof={'source_commit':BASE,'source_files':{f:hashlib.sha256((source/f).read_bytes()).hexdigest() for f in files},'vectors':cases,'operations':operations,'scope':'Original resolver and real mutation results; not Rust acceptance'}
 (out/'original-memory-temporal.json').write_text(json.dumps(proof,ensure_ascii=False,indent=2)+'\n')
 assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
 print(json.dumps({'vectors':len(cases),'operations':len(operations),'out':str(out)}))
if __name__=='__main__':main()
