"""Capture only non-secret identity relationships from the pinned Client9 service."""
import argparse,asyncio,hashlib,json,os,subprocess,sys,tempfile
from datetime import datetime,UTC
from pathlib import Path
BASE='329ca7c487414d2f54ddd98ff4727446ed2446cd'

async def capture(directory):
    from tests.conftest import _seed_database
    from tests.support.person_merge import merge_persons
    from tests.unit.test_acp_sessions import paired
    from yuki.client_api.sessions import ClientSessions
    from yuki.client_api.errors import ClientError
    from yuki.persistence.database import Database
    from yuki.identity.write_settings import IdentityWriteSettings,configure_identity_write_settings,reset_identity_write_settings
    from uuid import uuid4
    reset_identity_write_settings();configure_identity_write_settings(IdentityWriteSettings())
    db=Database('sqlite+aiosqlite:///'+str(directory/'original.db'))
    try:
        await db.create_schema();await _seed_database(db)
        ident,da,a=await paired(db);_,db_device,b=await paired(db,'1002')
        session=await ClientSessions(ident).create(b,str(uuid4()));scope=session['conversation_id']
        cp=await ident.authenticate(db_device['token'],conversation_id=scope)
        labels={a.person_id:'A',b.person_id:'B',a.binding_id:'A.app',b.binding_id:'B.app',a.conversation_id:'A.home',b.conversation_id:'B.home',cp.binding_id:'B.acp',cp.conversation_id:'B.thread',cp.thread_id:'thread',None:None}
        rows=[]
        async def observe(name,**selectors):
            try:
                principal=await ident.authenticate(da['token'],**selectors);view=await ident.me(principal)
                result={k:labels[view[k]] for k in ('person_id','source_person_id','request_person_id','binding_id','conversation_id','request_scope','request_conversation_id','thread_id')};result['platform']=view['platform']
            except ClientError as error:result={'error':str(error)}
            rows.append({'name':name,'result':result})
        await observe('default-before');await observe('foreign-app-before',binding_id=b.binding_id);await observe('foreign-acp-before',conversation_id=scope)
        async with db.immediate_session() as tx:await merge_persons(tx,a.person_id,b.person_id,now=datetime.now(UTC))
        await observe('default-after');await observe('selected-app',binding_id=b.binding_id);await observe('selected-acp',conversation_id=scope);await observe('matched-acp',binding_id=cp.binding_id,conversation_id=scope)
        await observe('mismatched-binding',binding_id=b.binding_id,conversation_id=scope);await observe('app-as-acp',conversation_id=b.conversation_id);await observe('acp-binding-alone',binding_id=cp.binding_id);await observe('missing-binding',binding_id='missing');await observe('empty-binding',binding_id='')
        from yuki.identity.db_models import IdentityBindingModel
        from yuki.client_api.requests import personal_command
        from yuki.conversation.canonical_db_models import ControlCommandReceiptModel
        from sqlalchemy import select
        async with db.immediate_session() as tx:(await tx.get(IdentityBindingModel,a.binding_id)).status='disabled'
        await observe('paused-default-app');await observe('paused-source-selected-app',binding_id=b.binding_id)
        async with db.immediate_session() as tx:(await tx.get(IdentityBindingModel,a.binding_id)).status='active'
        op=str(uuid4());current=await ident.authenticate(da['token']);old=await ident.issue_for_device(current,op)
        async with db.immediate_session() as tx:
            r=await tx.scalar(select(ControlCommandReceiptModel).where(ControlCommandReceiptModel.request_id==op));r.request_person_id=None
        async with db.read_session() as tx:
            nulls={}
            for label,person,conversation in [('original_source',a.person_id,None),('merged_root',b.person_id,None),('acp',b.person_id,scope)]:
                row=await personal_command(tx,person,op,conversation_id=conversation);nulls[label]=row is not None and row.result_resource_id==old['code_id']
        await ident.revoke(b,da['device_id'],str(uuid4()));await observe('revoked',binding_id=b.binding_id)
        return rows,nulls
    finally:await db.close();reset_identity_write_settings()

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--source',type=Path,required=True);parser.add_argument('--out',type=Path,required=True);a=parser.parse_args();source=a.source.resolve();out=a.out.resolve();out.mkdir(parents=True,exist_ok=False)
    assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip()==BASE and not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
    sys.path[:0]=[str(source),str(source/'src')];os.environ['SOUL_FILE']=str(source/'config/SOUL.md')
    with tempfile.TemporaryDirectory(prefix='yuki-client9-oracle-') as temp:
        os.chdir(temp);rows,nulls=asyncio.run(capture(Path(temp)))
    files=['src/yuki/client_api/http.py','src/yuki/client_api/identity.py','src/yuki/client_api/requests.py','src/yuki/client_api/sessions.py','tests/unit/test_acp_sessions.py']
    result={'source_commit':BASE,'source_files':{f:hashlib.sha256((source/f).read_bytes()).hexdigest() for f in files},'scenarios':rows,'legacy_null_receipt':nulls}
    (out/'original-entry-selection.json').write_text(json.dumps(result,indent=2)+'\n');assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source);print('original Client9 scenarios:',len(rows))
if __name__=='__main__':main()
