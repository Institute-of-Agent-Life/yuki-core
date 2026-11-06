"""Run the actual Host delegation against an original QQ Work and three selected namespaces."""
import hashlib
import json
import sqlite3
import sys
import uuid
from contextlib import closing
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder

def exercise(ctx,wire,sdk,Plugin):
    o=Owner(ctx,'selected-work',46)
    folder=ctx.working/'selected-work-consumer';folder.mkdir()
    plugin=Plugin(o,folder);key=lambda:str(uuid.uuid4())
    def http(path,device=None,body=None,headers=None,expected=200):
        status,result=wire(o.svc.address,path,device and device['token'],body,selected=headers)
        ctx.equal('selected Work '+path+' status',status,expected)
        return result.get('data',result)
    def pair(person,purpose):
        code=o.op('personal','issue',person,key(),purpose)['code']
        return http(('/client' if purpose=='chat' else '/agents')+'/v1/pair',body={'code':code,'device_label':'selected-work fixture'})
    def sql(statement,*args):
        with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:db.execute(statement,args);db.commit()
    with Recorder() as model:
        args=[str(ctx.root/'tests/peers/personal/consumer.py'),str(folder),str(sdk/'python-install')]
        o.config(model,'\n[[plugins]]\nid="personal-peer"\nnamespace="local-agent"\nprogram='+json.dumps(sys.executable)+'\nargs='+json.dumps(args)+'\ngrants=["personal.auth","personal.work","native.events"]\n')
        with o.svc.running():
            plugin.ready();b=key();o.op('person','create',b)
            a=pair(o.actor,'chat');bdev=pair(b,'chat');connector=pair(b,'connector')
            home_a=http('/client/v1/me',a);home_b=http('/client/v1/me',bdev);bh={'X-Yuki-Entry':home_b['binding_id']}
            acp=o.op('account','open','acp','personal','work-client',1,'--person',b,'--start',1)['assignment']
            bot=o.op('account','open','acp','personal','yuki-acp',1,'--start',1)['assignment']
            conv=o.op('entry','private',acp,bot,'--topic',key())['entry'];ch={'X-Yuki-Conversation':conv}
            o.human('accept this QQ Work');_,q=o.wire(model)
            reply(q,calls=[call('accept','task_control',{'action':'accept','goal':'keep original QQ source'})])
            body,q=o.wire(model)
            work=json.loads(next(m['content'] for m in body['messages'] if m.get('tool_call_id')=='accept'))['result']['work_id']
            url='/client/v1/works/'+work
            original=o.rows('SELECT principal,entry,owner_execution,budget FROM work_nodes WHERE id=?',work)
            o.op('person','merge',o.actor,b)
            def proof(device,purpose,header=None):
                return http('/core/v1/entry/exchange',device,{'purpose':purpose,'plugin':'personal-peer','work':work},header)['proof']
            def auth(ticket):return plugin.rpc('device.auth',{'proof':ticket})['handle']
            handle=auth(proof(a,'chat',bh))
            op=key();request={'request_id':op,'text':'one UUID across private namespaces'}
            chosen=plugin.rpc('work.input',{'handle':handle,'request':request})
            ctx.check('B device replays A-selected B Host input over HTTP',http(url+'/inputs',bdev,request)['replay'])
            pending=proof(a,'chat',bh)
            selections=[(chosen,home_b['conversation_id'],b),(http(url+'/inputs',a,request),home_a['conversation_id'],o.actor),(http(url+'/inputs',a,request,ch),conv,b)]
            for result,entry,person in selections:
                ctx.equal('selected Work mail and event share the authenticated original source',o.rows('SELECT e.entry_id,e.raw_actor FROM work_mail m JOIN work_wait_mail w ON m.id=w.mail JOIN events e ON e.id=w.source_event WHERE m.work=? AND m.id=? AND e.id=?',work,result['input_id'],result['event_id']),[(entry,person)])
            ctx.equal('one UUID has three durable independent Work mailbox effects',len({r['input_id'] for r,_,_ in selections}),3)
            target={'device_id':connector['device_id'],'intent':'prompt','operation_id':'selected-op'}
            native_body={'request_id':key(),'text':'selected target','agent_target':target}
            http(url+'/inputs',a,native_body,bh)
            http(url+'/inputs',a,{**native_body,'request_id':key()},ch,409)
            http(url+'/inputs',a,{**native_body,'request_id':key(),'agent_target':{**target,'intent':'steer'}},ch,422)
            nh=auth(proof(connector,'connector'))
            event={'handle':nh,'operation':'selected-op','event_key':'ack'}
            ack=plugin.rpc('native.notify',{**event,'payload':{'result':'private answer'}})
            ctx.equal('native event uses original selected chat entry',o.rows('SELECT entry_id,raw_actor FROM events WHERE id=?',ack['event']),[(home_b['conversation_id'],b)])
            stage=home_b['binding_id'];at=o.rows('SELECT max(at)+1 FROM assignment_states WHERE stage=?',stage)[0][0]
            o.op('account','enabled',stage,'false',at)
            before=o.rows('SELECT (SELECT count(*) FROM work_mail),(SELECT count(*) FROM personal_receipts),(SELECT count(*) FROM native_receipts),(SELECT count(*) FROM events)')
            plugin.rpc('device.auth',{'proof':pending},False)
            plugin.rpc('work.get',{'handle':handle})
            plugin.rpc('work.input',{'handle':handle,'request':{**request,'request_id':key()}},False)
            plugin.rpc('native.notify',{**event,'event_key':'late','payload':{'result':'after pause'}},False)
            ctx.equal('late selected callbacks leave all four ledgers intact',o.rows('SELECT (SELECT count(*) FROM work_mail),(SELECT count(*) FROM personal_receipts),(SELECT count(*) FROM native_receipts),(SELECT count(*) FROM events)'),before)
            o.op('account','enabled',stage,'true',at+1);auth(pending)
            old=proof(a,'chat')
            sql('UPDATE personal_proofs SET selected_entry=NULL WHERE hash=?',hashlib.sha256(old.encode()).hexdigest())
            handle=auth(old);sql('UPDATE personal_sessions SET selected_entry=NULL WHERE hash=?',hashlib.sha256(handle.encode()).hexdigest())
            restored=plugin.rpc('work.input',{'handle':handle,'request':request})
            ctx.equal('reader 46 NULL proof and session replay only original device input',restored['event_id'],selections[1][0]['event_id'])
            ctx.equal('private inputs retain original QQ Work, manager and budget',o.rows('SELECT principal,entry,owner_execution,budget FROM work_nodes WHERE id=?',work),original)
            ctx.equal('selected inputs did not create App Main events',o.rows('SELECT count(*) FROM main_inputs WHERE event IN (SELECT event FROM personal_receipts WHERE operation=?)',op),[(0,)])
            reply(q,text='received')
    return {'independent_mailboxes':3,'original_work_unchanged':True,'paused_callbacks_new_effects':0,'legacy_null_delegation':'original device entry'}
