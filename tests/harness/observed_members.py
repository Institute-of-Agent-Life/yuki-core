"""Original member directory through normal Main with no gateway roster request."""
import hashlib
import subprocess
import sys
import json
import os
import uuid
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer as QQ


def run(ctx):
    owner=Owner(ctx,'observed-members',42)
    space=json.dumps(['qq','compact','700'],separators=(',',':'))
    foreign=json.dumps(['qq','compact','701'],separators=(',',':'))
    bob=str(uuid.uuid4());stranger=str(uuid.uuid4())
    with owner.svc.running():
        owner.op('person','create',bob);owner.op('person','create',stranger)
        second=owner.op('account','open','qq','compact','201',1,'--person',owner.actor,'--start',1)
        peer=owner.op('account','open','qq','compact','202',1,'--person',bob,'--start',1)
        hidden=owner.op('account','open','qq','compact','203',1,'--person',stranger,'--start',1)
        def record(account,key,group,card,when=10):
            return owner.op('ingress','record',json.dumps({'source':key,'account':account['account'],'bot_stage':owner.bot['assignment'],'reply_bot':'100','kind':'message','space':group,'occurred':when,'observed':when,'sender':{'nickname':'nick-'+key,'card':card},'segments':[{'type':'text','text':'observed '+key}]}))['event']
        record(owner.user,'alice',space,'Alice')
        record(second,'second-account',space,'Second account')
        record(second,'card-cleared',space,'',when=30)
        record(second,'late-old-card',space,'Outdated card',when=20)
        bob_source=record(peer,'bob',space,'Bob card')
        record(hidden,'private-group',foreign,'Hidden')
    with Recorder() as model:
        owner.config(model)
        cfg=owner.svc.data/'yuki.toml';text=cfg.read_text();text=text.replace('root_slots=8','root_slots=20')
        cfg.write_text(text+f'''\n[qq]\nnamespace="compact"\ndesignated_ingress="100"\n[qq.groups]\n"700"="observe"\n[[qq.bots]]\nself_id="100"\nstage={json.dumps(owner.bot['assignment'])}\ntoken_env="MEMBERS_QQ"\n''')
        environment={**os.environ,'MEMBERS_QQ':'offline-members'}
        with owner.svc.running(environment=environment):
            socket=QQ(owner.svc.address,'100','offline-members')
            try:
                owner.until(lambda:owner.op('qq','status','100')['online'])
                socket.send({'post_type':'message','message_type':'group','self_id':100,'user_id':200,'group_id':700,'message_id':13001,'time':100,'sender':{'user_id':200,'nickname':'Alice live'},'message':[{'type':'at','data':{'qq':'777','name':'unassigned mention'}},{'type':'text','data':{'text':' observed account only'}}]})
                owner.until(lambda:owner.rows("SELECT id FROM accounts WHERE namespace='compact' AND native='777'"))
                unknown=owner.rows("SELECT id FROM accounts WHERE namespace='compact' AND native='777'")[0][0]
                owner.until(lambda:owner.rows("SELECT id FROM events WHERE EXISTS(SELECT 1 FROM json_each(events.mentions) m WHERE json_extract(m.value,'$.account')=?)",unknown))
            finally:socket.close()
            owner.until(lambda:not owner.op('qq','status','100')['online'])
            ctx.equal('observing an unknown mention does not fabricate identity',owner.rows('SELECT count(*) FROM assignments WHERE account_id=?',unknown),[(0,)])
        cfg.write_text(cfg.read_text().replace('\"700\"=\"observe\"','\"700\"=\"paused\"'))
        with owner.svc.running(environment=environment):
            event=owner.human('Read the observed member directory',group=space)
            wire,q=owner.wire(model)
            declaration=json.loads((ctx.root/'fixtures/social/v4-members.json').read_text(encoding='utf-8'))['declarations'][0]
            ctx.equal('normal Main retains original group directory declaration',next(t['function'] for t in wire['tools'] if t['function']['name']=='get_group_members'),declaration)
            snapshots=[]
            def invoke(args,accepted=True):
                nonlocal q
                key=str(uuid.uuid4());reply(q,calls=[call(key,'get_group_members',args)])
                wire,q=owner.wire(model)
                receipt=json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')==key))
                ctx.equal('member result is paired with its original model call',receipt['state'],'confirmed' if accepted else 'failed')
                snapshots.append({'args':args,'receipt':receipt})
                ctx.write('observed-members-progress.json',{'calls':snapshots,'bindings':owner.rows('SELECT * FROM contact_space_bindings'),'observed':owner.rows('SELECT * FROM observed_space_bindings')})
                return receipt['result']
            page=invoke({'limit':1});binding=page['binding_id'];items=page['items']
            while page['next_cursor']:
                page=invoke({'limit':1,'cursor':page['next_cursor']});items+=page['items']
            expected=sorted([owner.user['account'],second['account'],peer['account'],unknown])
            ctx.equal('keyset pages preserve every observed account exactly once',[item['account_id'] for item in items],expected)
            by_id={item['account_id']:item for item in items}
            ctx.equal('same person keeps two separate observed accounts',[by_id[x]['person_id'] for x in [owner.user['account'],second['account']]],[owner.actor,owner.actor])
            ctx.equal('unknown mention remains unassigned',[by_id[unknown]['assignment_id'],by_id[unknown]['person_id'],by_id[unknown]['presence_id']],[None,None,None])
            ctx.equal('later empty card clears previous name and ignores late old input',by_id[second['account']]['display_name'],'nick-card-cleared')
            ctx.equal('directory returns actual group card',by_id[peer['account']]['display_name'],'Bob card')
            ctx.check('opaque account references keep original account IDs',all(item['reference']=='@{account:'+item['account_id']+'}' for item in items))
            full=invoke({'binding_id':binding,'limit':100})
            ctx.equal('offline and send-paused directory remains observed',full['coverage'],'observed')
            ctx.equal('explicit original binding keeps the same members',full['items'],items)
            transferred=owner.op('account','transfer',second['assignment'],200000,200001,'--person',stranger)
            current=invoke({'binding_id':binding,'limit':100})
            account=next(item for item in current['items'] if item['account_id']==second['account'])
            ctx.equal('directory reports the current stage without reattributing old sources',[account['assignment_id'],account['person_id']],[transferred['assignment'],stranger])
            ctx.check('new account owner does not grant their unrelated group membership',hidden['account'] not in [item['account_id'] for item in current['items']])
            invoke({'cursor':'bad-cursor'},False)
            invoke({'target_id':str(uuid.uuid4())},False)
            missing=invoke({'binding_id':str(uuid.uuid4())})
            ctx.equal('unavailable original binding is not silently replaced',missing['error'],'space_entry_unavailable')
            ctx.equal('group directory performs no social sends',owner.rows('SELECT count(*) FROM social_outbound'),[(0,)])
            execution=owner.rows('SELECT id FROM executions WHERE source=?',event)[0][0]
            ctx.check('read result records actual member evidence',bool(owner.rows('SELECT event FROM main_dependencies WHERE execution=? AND event=?',execution,bob_source)))
            reply(q);owner.finished(event)
            owner.op('privacy','forget','member-bob-erasure',bob)
        with owner.svc.running(environment=environment):
            fresh=owner.human('Read current observed directory after restart',group=space)
            _,q=owner.wire(model);reply(q,calls=[call('after-erasure','get_group_members',{})])
            wire,q=owner.wire(model)
            result=json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')=='after-erasure'))
            ctx.equal('fresh original directory call succeeds after restart',result['state'],'confirmed')
            ctx.check('erased membership evidence does not reappear',peer['account'] not in [item['account_id'] for item in result['result']['items']])
            ctx.equal('no platform roster fallback after erasure',owner.rows('SELECT count(*) FROM social_outbound'),[(0,)])
            reply(q);owner.finished(fresh)
    built=subprocess.run([sys.executable,str(ctx.root/'ci/publish-contract.py'),'build','--out',str(ctx.output/'sdk-distributions'),'--verify-work',str(ctx.working/'members-sdk')],capture_output=True,text=True,timeout=120)
    ctx.write('members-sdk-build.json',{'exit_code':built.returncode,'stderr':built.stderr})
    ctx.equal('members candidate contains independently verified SDK packages',built.returncode,0)
    binary=ctx.root/'target/debug'/('yuki-core.exe' if os.name=='nt' else 'yuki-core')
    return {'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
            'host_idl_sha256':hashlib.sha256((ctx.root/'sdk/rust/src/host-preview.json').read_bytes()).hexdigest(),
            'members_oracle_sha256':hashlib.sha256((ctx.root/'fixtures/social/v4-members.json').read_bytes()).hexdigest(),
            'cases':snapshots,'after_erasure':result,'original_oracle':'329ca7c487414d2f54ddd98ff4727446ed2446cd','coverage':'observed','directory_executed_offline':True}
