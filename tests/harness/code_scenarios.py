"""Composition failure boundaries through native Main, SDK subprocesses and real HTTP effects."""

import json
import subprocess
import threading
from tests.harness.compact import Owner, call, reply
from tests.harness.json_file import read_json
from tests.peers.chat.recorder import Recorder


def response(owner, model, event, steps, timeout=12):
    _, q = owner.wire(model)
    reply(q, calls=[call('accept', 'task_control', {'action':'accept', 'goal':'original composition'})])
    _, q = owner.wire(model)
    reply(q, calls=[call('compose', 'code_a', {'steps':steps})])
    body, q = owner.wire(model, timeout=timeout)
    result = json.loads(next(m['content'] for m in body['messages'] if m.get('tool_call_id')=='compose'))
    reply(q, 'done')
    owner.finished(event)
    return result


def budgets(ctx, sdk, install):
    proofs = []
    for kind in ('readback', 'last-slot', 'no-work'):
        o = Owner(ctx, 'code-budget-'+kind, 29)
        folder = ctx.working/('budget-peer-'+kind)
        folder.mkdir()
        with Recorder() as model:
            install(ctx,o,folder,sdk,model,limit=2)
            if kind=='readback':
                config=o.svc.data/'yuki.toml'
                config.write_text(config.read_text().replace('[main]','[main]\nresult_page_budget=32000'),encoding='utf-8')
            with o.svc.running():
                event = o.human('apply original business tool accounting')
                if kind=='no-work':
                    _,q=o.wire(model)
                    reply(q,calls=[call('compose','code_a',{'steps':[]})])
                    body,q=o.wire(model)
                    result=json.loads(next(m['content'] for m in body['messages'] if m.get('tool_call_id')=='compose'))
                    ctx.equal('Code requires an accepted Work',result['state'],'failed')
                    ctx.equal('refused Code creates no VM parent',o.rows('SELECT count(*) FROM code_parents'),[(0,)])
                    reply(q,'done')
                    o.finished(event)
                else:
                    steps = ([{'name':'put','arguments':{'target':'large','value':'🦀'*15000}}, {'name':'get','arguments':{'target':'large'}}, {'name':'read_tool_artifact','arguments':{},'previous_artifact':True}] if kind=='readback' else [{'name':'send','arguments':{'text':'same'}} for _ in range(3)])
                    result=response(o,model,event,steps)
                    if result['state']!='confirmed':
                        ctx.write('code-budget-failure-'+kind+'.json',{'result':result,'children':o.rows('SELECT ordinal,operation,receipt FROM code_children'),'faults':{p.name:read_json(p) for p in folder.glob('private*fault*.json')}})
                    ctx.equal('native composition settled '+kind,result['state'],'confirmed')
                    ctx.equal('business root admits exactly two '+kind,o.rows('SELECT used_calls FROM root_tool_budgets'),[(2,)])
                    root,slots,units=o.rows('SELECT root,max_requests,max_units FROM root_budgets')[0]
                    o.op('model','root',root,slots,units)
                    usage=o.op('model','usage',root)
                    ctx.equal('original model root reconciliation preserves tool limit',(usage['business_tool_calls'],usage['business_tool_limit']),(2,2))
                    if kind=='readback':
                        row=o.rows("SELECT c.entered_at,c.receipt,b.charged FROM code_children c JOIN operations t ON t.namespace=c.execution AND t.operation_id=c.operation JOIN tool_charges b ON b.execution=c.execution AND b.operation=c.operation WHERE t.target='read_tool_artifact'")[0]
                        receipt=json.loads(row[1])
                        ctx.check('artifact child has real dispatch and original typed receipt',row[0] is not None and receipt['state']=='confirmed')
                        ctx.equal('artifact child reserves zero business slots',row[2],0)
                        ctx.equal('artifact child reads exact original text',receipt['result']['value'],'🦀'*128)
                    else:
                        ctx.equal('last slot prevents third physical effect',len(model.deliveries),2)
                        ctx.equal('third child has a definite un-dispatched refusal',o.rows("SELECT o.state,c.accepted,c.charged FROM code_children v JOIN operations o ON o.namespace=v.execution AND o.operation_id=v.operation JOIN tool_charges c ON c.execution=v.execution AND c.operation=v.operation WHERE v.ordinal=2"),[('failed',0,0)])
                proofs.append({'case':kind,'result':result,'charges':o.rows('SELECT operation,charged,accepted FROM tool_charges ORDER BY rowid')})
    return proofs


def ancestry(ctx,sdk,install):
    proofs=[]
    for kind,depth in [('cycle',2),('deep',8)]:
        o=Owner(ctx,'code-ancestors-'+kind,29)
        with Recorder() as model:
            for index in range(depth):
                name='code_'+chr(97+index)
                folder=ctx.working/(kind+'-'+name)
                folder.mkdir()
                install(ctx,o,folder,sdk,model,name=name,configure_model=index==0)
            steps=[{'name':'code_a','arguments':{'steps':[]},'expect_denied':True}] if kind=='cycle' else [{'name':'time_get_current','arguments':{}}]
            for index in reversed(range(1,depth)):
                steps=[{'name':'code_'+chr(97+index),'arguments':{'steps':steps}}]
            with o.svc.running():
                event=o.human('preserve the actual ancestor chain')
                result=response(o,model,event,steps)
                ctx.equal('nested composition completes '+kind,result['state'],'confirmed')
                if kind=='cycle':
                    ctx.equal('A to B to A creates no recursive A child',o.rows('SELECT count(*) FROM code_children'),[(1,)])
                    ctx.equal('cycle denial does not spend business slot',o.rows('SELECT used_calls FROM root_tool_budgets'),[(0,)])
                else:
                    ctx.equal('eight actual plugin parents survive without a depth gate',o.rows('SELECT count(*) FROM code_parents'),[(8,)])
                    ctx.equal('only final native child is business charged',o.rows('SELECT used_calls FROM root_tool_budgets'),[(1,)])
                proofs.append({'case':kind,'depth':depth,'parents':o.rows('SELECT handle,operation,closed FROM code_parents'),'children':o.rows('SELECT handle,ordinal,operation,fed_snapshot FROM code_children')})
    return proofs


def unknown(ctx,sdk,install):
    o=Owner(ctx,'code-physical-unknown',29)
    folder=ctx.working/'unknown-peer'
    folder.mkdir()
    with o.svc.running():
        event=o.human('preserve unknown physical dispatch')
    seen=threading.Event()
    release=threading.Event()
    with Recorder() as model:
        def hold(_):
            seen.set()
            release.wait(timeout=15)
        model.before_binding_reply=hold
        install(ctx,o,folder,sdk,model)
        process=subprocess.Popen([*o.svc.argv,'run'],stdout=subprocess.PIPE,text=True)
        try:
            ctx.equal('physical-cut native owner starts',json.loads(process.stdout.readline())['status'],'ready')
            _,q=o.wire(model)
            reply(q,calls=[call('accept','task_control',{'action':'accept','goal':'unknown child'})])
            _,q=o.wire(model)
            reply(q,calls=[call('compose','code_a',{'steps':[{'name':'send','arguments':{'text':'one received unknown'}} for _ in range(2)]})])
            ctx.check('receiver saw actual request before process termination',seen.wait(timeout=10))
            before=o.rows('SELECT operation FROM code_children')
            process.kill()
            ctx.check('physical boundary terminated real owner',process.wait(timeout=8)!=0)
        finally:
            release.set()
            if process.poll() is None:
                process.kill()
                process.wait(timeout=8)
            process.stdout.close()
        model.before_binding_reply=None
        with o.svc.running():
            body,q=o.wire(model)
            result=json.loads(next(m['content'] for m in body['messages'] if m.get('tool_call_id')=='compose'))
            ctx.equal('known parent returns a partial composition',result['result']['composition']['status'],'partial')
            ctx.equal('unknown child stays original and is never fed',o.rows('SELECT c.operation,o.state,c.fed_snapshot FROM code_children c JOIN operations o ON o.namespace=c.execution AND o.operation_id=c.operation'),[(before[0][0],'unknown',None)])
            ctx.equal('unknown receiver saw exactly one physical effect',len(model.deliveries),1)
            ctx.equal('unknown physical business slot remains reserved',o.rows('SELECT used_calls FROM root_tool_budgets'),[(1,)])
            reply(q,'done')
            o.finished(event)
        return {'parent':result,'physical_effects':model.deliveries,'original_child':before}


def erase(ctx,sdk,install):
    o=Owner(ctx,'code-erasure',29)
    folder=ctx.working/'erasure-peer'
    folder.mkdir()
    with Recorder() as model:
        install(ctx,o,folder,sdk,model)
        with o.svc.running():
            first=o.human('complete the unrelated earlier Work')
            response(o,model,first,[{'name':'time_get_current','arguments':{}}])
            other=o.rows('SELECT handle FROM code_parents')[0][0]
            old_bytes=o.rows('SELECT sum(length(bytes)) FROM code_snapshots WHERE handle=?',other)[0][0]
            canary='CODE_VM_PRIVATE_CANARY_534'
            event=o.human('erase this source '+canary)
            _,q=o.wire(model)
            reply(q,calls=[call('accept','task_control',{'action':'accept','goal':'private Code Work'})])
            _,q=o.wire(model)
            seen=threading.Event()
            release=threading.Event()
            def hold(_):
                seen.set()
                release.wait(timeout=15)
            model.before_binding_reply=hold
            reply(q,calls=[call('compose','code_a',{'steps':[{'name':'send','arguments':{'text':canary}}]})])
            ctx.check('private child crosses real effect boundary',seen.wait(timeout=10))
            try:
                o.op('privacy','erase','erase-code',o.actor,json.dumps([event]))
                ctx.equal('source erasure removes every affected VM byte',o.rows('SELECT count(*) FROM code_snapshots s JOIN code_parents p ON p.handle=s.handle JOIN executions e ON e.id=p.execution WHERE e.source=? AND s.bytes IS NOT NULL',event),[(0,)])
                ctx.equal('unrelated completed VM snapshot survives source erasure',o.rows('SELECT sum(length(bytes)) FROM code_snapshots WHERE handle=?',other),[(old_bytes,)])
            finally:
                release.set()
            o.until(lambda:o.rows("SELECT id FROM executions WHERE source=? AND state='held'",event))
            ctx.equal('late child returns no erased VM receipt body',o.rows('SELECT count(*) FROM code_children c JOIN executions e ON e.id=c.execution WHERE e.source=? AND c.receipt IS NOT NULL',event),[(0,)])
            ctx.equal('erasure does not clear unrelated original receipt',o.rows('SELECT count(*) FROM code_children WHERE handle=? AND receipt IS NOT NULL',other),[(1,)])
            ctx.check('no new model purchase confirms erased result',model.wires.empty())
            return {'erased_event':event,'unrelated_parent':other,'retained_bytes':old_bytes,'physical_effects':len(model.deliveries)}


def cancel(ctx,sdk,install):
    import uuid
    from urllib.request import Request,urlopen
    o=Owner(ctx,'code-cancel',29)
    folder=ctx.working/'cancel-peer'
    folder.mkdir()
    with Recorder() as model:
        install(ctx,o,folder,sdk,model)
        seen=threading.Event()
        release=threading.Event()
        def hold(_):
            seen.set()
            release.wait(timeout=15)
        model.before_binding_reply=hold
        with o.svc.running():
            event=o.human('cancel this original Code Work')
            _,q=o.wire(model)
            reply(q,calls=[call('accept','task_control',{'action':'accept','goal':'cancel original program'})])
            _,q=o.wire(model)
            reply(q,calls=[call('compose','code_a',{'steps':[{'name':'send','arguments':{'text':'original first effect'}} for _ in range(2)]})])
            ctx.check('cancellable program reaches actual recipient',seen.wait(timeout=10))
            def http(path,body=None,token=None):
                headers={'Content-Type':'application/json'}
                if token:headers['Authorization']='Bearer '+token
                with urlopen(Request('http://'+o.svc.address+path,data=json.dumps(body).encode() if body is not None else None,headers=headers),timeout=8) as r:return json.load(r)['data']
            try:
                code=o.op('personal','issue',o.actor,str(uuid.uuid4()),'chat')['code']
                token=http('/client/v1/pair',{'code':code,'device_label':'Code cancellation'})['token']
                work=o.rows('SELECT id FROM work_nodes')[0][0]
                state=http('/client/v1/works/'+work,token=token)
                http('/client/v1/works/'+work+'/cancel',{'request_id':str(uuid.uuid4()),'expected_revision':state['revision']},token)
            finally:
                release.set()
            o.until(lambda:o.rows("SELECT state FROM host_calls WHERE state IN('confirmed','failed','unknown')"))
            ctx.equal('cancellation stops second Code child',o.rows('SELECT count(*) FROM code_children'),[(1,)])
            ctx.equal('cancelled program retains one real effect',len(model.deliveries),1)
            ctx.equal('cancelled program retains its original business charge',o.rows('SELECT used_calls FROM root_tool_budgets'),[(1,)])
            ctx.equal('cancelled program creates no new Main purchase',o.rows('SELECT reserved_requests FROM root_budgets'),[(2,)])
            ctx.equal('work cancellation stays terminal',o.rows('SELECT status FROM work_nodes'),[('cancelled',)])
            return {'work':work,'event':event,'physical_effects':model.deliveries}


def lifecycle(ctx,sdk,install):
    o=Owner(ctx,'code-lifecycle',29)
    folder=ctx.working/'lifecycle-peer'
    folder.mkdir()
    with Recorder() as model:
        install(ctx,o,folder,sdk,model,limit=1)
        with o.svc.running():
            event=o.human('finish and still deliver the original program tail')
            _,q=o.wire(model)
            reply(q,calls=[call('accept','task_control',{'action':'accept','goal':'Code completion tail'})])
            _,q=o.wire(model)
            reply(q,calls=[call('compose','code_a',{'steps':[{'name':'task_control','arguments':{'action':'complete','result':'retained original result'}},{'name':'send','arguments':{'text':'legitimate completion tail'}}]})])
            o.finished(event)
            ctx.equal('Code completion proposal leaves legitimate tail executable',len(model.deliveries),1)
            ctx.equal('Code completion and parent consume zero business slots',o.rows('SELECT used_calls FROM root_tool_budgets'),[(1,)])
            ctx.equal('completed program does not buy another model confirmation',o.rows('SELECT reserved_requests FROM root_budgets'),[(2,)])
            ctx.equal('child completion proposal settles original Work',o.rows('SELECT status,summary FROM work_nodes'),[('completed','retained original result')])
            return {'children':o.rows('SELECT ordinal,operation,fed_snapshot FROM code_children ORDER BY ordinal'),'physical_effects':model.deliveries}


def boundaries(ctx,sdk,install):
    import base64
    o=Owner(ctx,'code-grant-boundaries',29)
    folders={name:ctx.working/('bound-'+name) for name in ('code_a','code_b')}
    for folder in folders.values():folder.mkdir()
    with Recorder() as model:
        install(ctx,o,folders['code_a'],sdk,model)
        install(ctx,o,folders['code_b'],sdk,model,name='code_b',configure_model=False)
        seen=threading.Event()
        release=threading.Event()
        def hold(_):
            seen.set()
            release.wait(timeout=20)
        model.before_binding_reply=hold
        with o.svc.running():
            event=o.human('verify original scoped Code boundaries')
            _,q=o.wire(model)
            reply(q,calls=[call('accept','task_control',{'action':'accept','goal':'scope assertions'})])
            _,q=o.wire(model)
            steps=[{'name':'code_b','arguments':{'steps':[{'name':'send','arguments':{'text':'bound original effect'}}]}}]
            reply(q,calls=[call('compose','code_a',{'steps':steps})])
            ctx.check('both original Code parents are active',seen.wait(timeout=10))
            a=o.rows("SELECT handle FROM host_calls WHERE plugin='code_a'")[0][0]
            b=o.rows("SELECT handle FROM host_calls WHERE plugin='code_b'")[0][0]
            count=0
            observations=[]
            def probe(plugin,method,params,ok=False):
                nonlocal count
                count+=1
                path=folders[plugin]/'probe.tmp'
                path.write_text(json.dumps({'id':count,'method':method,'params':params}),encoding='utf-8')
                path.replace(folders[plugin]/'probe.json')
                out=folders[plugin]/('probe-'+str(count)+'.json')
                try:
                    o.until(out.is_file)
                    result=read_json(out)
                except Exception:
                    ctx.write('code-boundary-probe-failure.json',{
                        'method':method,'params':params,
                        'calls':o.rows('SELECT handle,plugin,state,result FROM host_calls'),
                        'plugins':o.rows('SELECT plugin,enabled,online FROM host_installations'),
                        'peer_faults':{name:{p.name:read_json(p) for p in folder.glob('private-*fault.json')}
                            for name,folder in folders.items()},
                    })
                    raise
                ctx.equal('bound callback '+method+(' accepted' if ok else ' denied'),'result' in result,ok)
                observations.append({'plugin':plugin,'method':method,'accepted':ok})
                return result.get('result')
            try:
                probe('code_b','tool.schema',{'handle':a,'name':'send'})
                probe('code_b','code.snapshot.get',{'handle':a,'snapshot':'before-0','offset':0})
                probe('code_b','code.sources',{'handle':a})
                probe('code_a','tool.schema',{'handle':a,'name':'send','actor':'forged-admin'})
                original={'handle':a,'ordinal':0,'feed_index':0,'engine_call_id':'engine-0','checkpoint':0,'snapshot':'before-0','name':'code_b','arguments':{'steps':[]}}
                probe('code_a','tool.invoke',original)
                probe('code_a','tool.resume',{'handle':a,'ordinal':0,'feed_index':99,'engine_call_id':'other-engine-call'})
                digest,size=o.rows('SELECT sha256,size FROM code_snapshots WHERE handle=? AND id=?',a,'before-0')[0]
                probe('code_a','code.snapshot.put',{'handle':a,'snapshot':'before-0','sha256':digest,'size':size,'codec':'2'*64,'offset':0,'bytes':base64.b64encode(b'changed').decode()})
                probe('code_a','code.snapshot.get',{'handle':a,'snapshot':'before-0','offset':size+1})
                probe('code_a','code.snapshot.put',{'handle':a,'snapshot':'wrong-codec','sha256':'0'*64,'size':4,'codec':'4'*64,'offset':0,'bytes':base64.b64encode(b'blob').decode()})
                probe('code_a','code.snapshot.put',{'handle':a,'snapshot':'bad-hash','sha256':'0'*64,'size':4,'codec':'2'*64,'offset':0,'bytes':base64.b64encode(b'blob').decode()},True)
                probe('code_a','tool.invoke',dict(original,ordinal=1,feed_index=1,engine_call_id='engine-1',checkpoint=1,snapshot='bad-hash',name='time_get_current',arguments={}))
                ctx.equal('rejected callback attempts create no child or extra charge',(o.rows('SELECT count(*) FROM code_children'),o.rows('SELECT used_calls FROM root_tool_budgets')), ([(2,)],[(1,)]))
            finally:
                release.set()
            body,q=o.wire(model)
            reply(q,'done')
            o.finished(event)
            settlement=o.rows("SELECT plugin,state,result FROM host_calls WHERE method='tool.call' ORDER BY plugin")
            ctx.write('code-boundaries-settlement.json',{'calls':settlement})
            ctx.equal('both original boundary parents settle before retained probes',
                [(plugin,state,json.loads(result or '{}').get('data',{}).get('status')) for plugin,state,result in settlement],
                [('code_a','confirmed','completed'),('code_b','confirmed','completed')])
            # A paired parent only exposes retained receipts. It cannot accept another child.
            probe('code_a','tool.invoke',dict(original,ordinal=1,feed_index=1,engine_call_id='engine-1',checkpoint=2,name='send',arguments={'text':'forbidden tail'}))
            ctx.equal('paired original parent produces one actual effect',len(model.deliveries),1)
            return {'handles':[a,b],'probes':observations,'physical_effects':model.deliveries}


def long_program(ctx,sdk,install):
    o=Owner(ctx,'code-long-program',29)
    folder=ctx.working/'long-program-peer'
    folder.mkdir()
    with Recorder() as model:
        install(ctx,o,folder,sdk,model,limit=40)
        with o.svc.running():
            event=o.human('continue an actual long original program')
            # The installed Code watchdog permits 30 seconds for this multi-boundary call.
            result=response(o,model,event,[{'name':'time_get_current','arguments':{}} for _ in range(32)],timeout=35)
            ctx.equal('long program settles through ordinary Main',result['state'],'confirmed')
            ctx.equal('32 original VM feed points retained',o.rows('SELECT count(*),sum(fed_snapshot IS NOT NULL) FROM code_children'),[(32,32)])
            ctx.equal('32 child reservations and zero parent reservation',o.rows('SELECT used_calls FROM root_tool_budgets'),[(32,)])
            body=json.loads(o.rows("SELECT result FROM host_calls WHERE method='tool.call'")[0][0])
            ctx.equal('parent summary reports original full cardinality',body['composition']['child_count'],32)
            ctx.check('parent summary has explicit paging boundary',body['composition']['has_more'] and len(body['composition']['children'])==16)
            ctx.equal('superseded VM bodies are reclaimed while original keys remain',o.rows('SELECT count(*),sum(bytes IS NOT NULL) FROM code_snapshots'),[(64,1)])
            return {'parent_summary':body['composition'],'snapshots':64,'retained_vm_bodies':1}


def parity(ctx,sdk,install):
    import uuid
    o=Owner(ctx,'code-direct-parity',29)
    folder=ctx.working/'parity-peer'
    folder.mkdir()
    with Recorder() as model:
        install(ctx,o,folder,sdk,model)
        config=o.svc.data/'yuki.toml'
        config.write_text(config.read_text().replace('code_tool="code_a"\n',''),encoding='utf-8')
        with o.svc.running():
            foreign=str(uuid.uuid4())
            o.op('person','create',foreign,'--name','FOREIGN_PRIVATE_DIRECTORY_CANARY')
            event=o.human('apply identical authority on direct and Code reads')
            _,q=o.wire(model)
            reply(q,calls=[call('accept','task_control',{'action':'accept','goal':'same original caller'})])
            _,q=o.wire(model)
            args={'kind':'person','target_id':foreign}
            reply(q,calls=[call('compose','code_a',{'steps':[{'name':'find_contacts','arguments':args}]}),call('direct','find_contacts',args)])
            body,q=o.wire(model)
            direct=json.loads(next(m['content'] for m in body['messages'] if m.get('tool_call_id')=='direct'))
            child=json.loads(o.rows('SELECT receipt FROM code_children')[0][0])
            ctx.equal('direct and Code use identical current caller and source intersection',(direct['state'],direct['result']),(child['state'],child['result']))
            ctx.equal('foreign person remains outside both result sets',child['result']['items'],[])
            ctx.check('private foreign name is never emitted','FOREIGN_PRIVATE_DIRECTORY_CANARY' not in json.dumps(body))
            reply(q,'done')
            o.finished(event)
            return {'direct':direct,'child':child}
