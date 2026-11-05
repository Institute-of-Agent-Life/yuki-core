"""Original child identities, shared dispatcher and opaque VM recovery across real processes."""

import json
import hashlib
from concurrent.futures import ThreadPoolExecutor
import subprocess
import sys
from pathlib import Path
from tests.harness.compact import Owner, call, reply
from tests.harness.crash import kill_writer
from tests.peers.chat.recorder import Recorder


def installed(ctx, owner, folder, sdk, model, *, limit=8, name="code_a", configure_model=True):
    text = f'\n[main]\nendpoint="{model.url}"\nbinding_endpoint="{model.url.removesuffix("/v1/chat/completions")}/binding"\nmodel="fixture"\nname="Yuki"\nmax_output=512\nroot_slots=8\nroot_tool_calls={limit}\nmax_input_units=4000000\nmerge_ms=1\ncode_tool="{name}"\n'
    if not configure_model:
        text = ""
    text += f'\n[[plugins]]\nid="{name}"\nnamespace="{name}"\nprogram=' + json.dumps(sys.executable)
    text += '\nargs=' + json.dumps([str(ctx.root/'tests/peers/code/peer.py'), str(folder), str(sdk/'python-install')])
    text += '\ngrants=["tool.call","composition.read","composition.execute","forget.owner"]\n'
    text += f'[plugins.composition]\ntool="{name}"\nabi="opaque-conformance-v1"\nengine_sha256="'+ '1'*64 + '"\ncodec_sha256="' + '2'*64 + '"\nlauncher_sha256="' + '3'*64 + '"\n'
    text += f'[[plugins.tools]]\nname="{name}"\ndescription="Opaque SDK composition fixture"\nparallel_read=false\ncacheable=false\ncode_direct=true\nschema={{type="object",properties={{steps={{type="array",items={{type="object"}},maxItems=100}}}},required=["steps"],additionalProperties=false}}\n'
    with (owner.svc.data/'yuki.toml').open('a',encoding='utf-8') as f:
        f.write(text)


def cuts(ctx, sdk):
    results=[]
    phases=("code-T1-committed", "code-executor-entered", "code-T3-committed", "code-feed-committed")
    for index,phase in enumerate(phases):
        owner=Owner(ctx,"code-cut-"+str(index),29)
        folder=ctx.working/("cut-peer-"+str(index))
        folder.mkdir()
        with owner.svc.running():
            event=owner.human("continue original VM after process termination")
        with Recorder() as model:
            installed(ctx,owner,folder,sdk,model)
            with ThreadPoolExecutor(max_workers=1) as pool:
                death=pool.submit(kill_writer,ctx,owner.svc.data,"work_delivery_crash_child",phase,{})
                _,q=owner.wire(model)
                reply(q,calls=[call("accept","task_control",{"action":"accept","goal":"original VM recovery"})])
                _,q=owner.wire(model)
                reply(q,calls=[call("compose","code_a",{"steps":[{"name":"send","arguments":{"text":"same original arguments"}} for _ in range(2)]})])
                death.result()
            prior=owner.rows("SELECT ordinal,operation,feed_index,engine_call_id,snapshot,fed_snapshot FROM code_children ORDER BY ordinal")
            ctx.equal("original first child persisted at "+phase,len(prior),1)
            before=len(model.deliveries)
            with owner.svc.running():
                body,q=owner.wire(model)
                if (folder/'private-fault.json').exists():
                    ctx.write('peer-fault-'+str(index)+'.json',json.loads((folder/'private-fault.json').read_bytes()))
                result=json.loads(next(m['content'] for m in body['messages'] if m.get('tool_call_id')=='compose'))
                ctx.equal("cold original Code parent completes at "+phase,result['state'],'confirmed')
                rows=owner.rows("SELECT ordinal,operation,feed_index,engine_call_id,snapshot,fed_snapshot FROM code_children ORDER BY ordinal")
                ctx.equal("retained child key and VM position at "+phase,rows[0][:5],prior[0][:5])
                ctx.equal("two physical effects at "+phase,len(model.deliveries),2)
                ctx.equal("root tool count survives at "+phase,owner.rows("SELECT used_calls FROM root_tool_budgets"),[(2,)])
                ctx.equal("one restored worker per original parent",len(list(folder.glob('resumed-*.json'))),1)
                ctx.equal("original script initialized once",len(list(folder.glob('started-*.json'))),1)
                reply(q,'done')
                owner.finished(event)
                ctx.equal("three Main purchases cover entire cold workflow",owner.rows("SELECT reserved_requests FROM root_budgets"),[(3,)])
                results.append({'cut':phase,'before':prior,'after':rows,'physical_before':before,'physical_after':len(model.deliveries),'result':result})
    return results


def contract_change(ctx,sdk):
    o=Owner(ctx,'code-contract-change',29)
    folder=ctx.working/'contract-peer'
    folder.mkdir()
    with o.svc.running():
        event=o.human('retain the original VM codec contract')
    with Recorder() as model:
        installed(ctx,o,folder,sdk,model)
        with ThreadPoolExecutor(max_workers=1) as pool:
            death=pool.submit(kill_writer,ctx,o.svc.data,'work_delivery_crash_child','code-T1-committed',{},sample='changed-codec')
            _,q=o.wire(model)
            reply(q,calls=[call('accept','task_control',{'action':'accept','goal':'preserve original codec'})])
            _,q=o.wire(model)
            reply(q,calls=[call('compose','code_a',{'steps':[{'name':'send','arguments':{'text':'original'}}]})])
            death.result()
        before=o.rows('SELECT handle,operation,snapshot FROM code_parents')
        config=o.svc.data/'yuki.toml'
        config.write_text(config.read_text().replace('engine_sha256="'+'1'*64+'"','engine_sha256="'+'4'*64+'"'),encoding='utf-8')
        with o.svc.running():
            o.until(lambda:o.rows("SELECT id FROM executions WHERE source=? AND state='held'",event))
            ctx.equal('incompatible runtime has an explicit original diagnostic',o.rows('SELECT diagnostic FROM main_turns'),[('code_original_runtime_unavailable',)])
            ctx.equal('changed runtime preserves original checkpoint',o.rows('SELECT handle,operation,snapshot FROM code_parents'),before)
            ctx.equal('changed runtime starts no replacement script',len(list(folder.glob('started-*.json'))),1)
            ctx.equal('changed runtime never dispatches child',len(model.deliveries),0)
            ctx.equal('changed runtime buys no confirmation',o.rows('SELECT reserved_requests FROM root_budgets'),[(2,)])
        return {'original':before,'diagnostic':'code_original_runtime_unavailable'}


def run(ctx):
    sdk = ctx.working/'sdk'
    process = subprocess.run([sys.executable,str(ctx.root/'ci/publish-contract.py'),'build','--out',str(ctx.output/'sdk-distributions'),'--verify-work',str(sdk)],capture_output=True,text=True)
    if process.returncode:
        ctx.write('sdk-failure.json',{'stderr':process.stderr})
    ctx.equal('both packaged SDKs negotiate Code methods',process.returncode,0)
    owner = Owner(ctx,'composition',29)
    folder = ctx.working/'code-a'
    folder.mkdir()
    with Recorder() as model:
        installed(ctx,owner,folder,sdk,model)
        with owner.svc.running():
            event = owner.human('perform the two original equal operations')
            _,q = owner.wire(model)
            reply(q,calls=[call('accept','task_control',{'action':'accept','goal':'original Code Work'})])
            _,q = owner.wire(model)
            steps = [{'name':'send','arguments':{'text':'same original arguments'}} for _ in range(2)]
            reply(q,calls=[call('compose','code_a',{'steps':steps})])
            body,q = owner.wire(model)
            ctx.write('code-first-next-wire.json',body)
            if (folder/'private-fault.json').exists():
                ctx.write('code-peer-fault.json',json.loads((folder/'private-fault.json').read_bytes()))
            receipt = json.loads(next(m['content'] for m in body['messages'] if m.get('tool_call_id')=='compose'))
            ctx.equal('composition succeeds through ordinary Main',receipt['state'],'confirmed')
            children = owner.rows('SELECT ordinal,operation,feed_index,engine_call_id,fed_snapshot FROM code_children ORDER BY ordinal')
            ctx.equal('same arguments have two original business children',len(children),2)
            ctx.check('child keys differ by ordinal',children[0][1]!=children[1][1])
            ctx.equal('two real physical effects',len(model.deliveries),2)
            ctx.equal('original root charges children and excludes parent',owner.rows('SELECT used_calls FROM root_tool_budgets'),[(2,)])
            ctx.equal('code parent and lifecycle are zero charged',owner.rows("SELECT charged FROM tool_charges c JOIN operations o ON o.namespace=c.execution AND o.operation_id=c.operation WHERE o.target IN('task_control','code_a') ORDER BY c.rowid"),[(0,),(0,)])
            reply(q,'done')
            owner.finished(event)
            proof={'same_arguments':children,'physical_effects':model.deliveries,'parent_receipt':receipt,'methods':json.loads((folder/'ready.json').read_bytes())['methods'],'snapshot_provider':'opaque SDK conformance fixture','real_vm_consumer':'pending EXT-023 and EXT-024'}
    proof['cuts']=cuts(ctx,sdk)
    from tests.harness import code_scenarios
    proof['budgets']=code_scenarios.budgets(ctx,sdk,installed)
    proof['ancestry']=code_scenarios.ancestry(ctx,sdk,installed)
    proof['unknown']=code_scenarios.unknown(ctx,sdk,installed)
    proof['erasure']=code_scenarios.erase(ctx,sdk,installed)
    proof['cancel']=code_scenarios.cancel(ctx,sdk,installed)
    proof['lifecycle']=code_scenarios.lifecycle(ctx,sdk,installed)
    proof['boundaries']=code_scenarios.boundaries(ctx,sdk,installed)
    proof['long_program']=code_scenarios.long_program(ctx,sdk,installed)
    proof['contract_change']=contract_change(ctx,sdk)
    proof['parity']=code_scenarios.parity(ctx,sdk,installed)
    methods=(ctx.root/'protocol/plugin/composition/methods.json').read_bytes()
    proof['binary_sha256']=hashlib.sha256(owner.svc.binary.read_bytes()).hexdigest()
    proof['host_idl_sha256']=hashlib.sha256((ctx.root/'sdk/rust/src/host-preview.json').read_bytes()).hexdigest()
    proof['composition_schema_sha256']=hashlib.sha256(methods).hexdigest()
    proof['negotiated_composition_methods']=sorted(set(proof['methods']) & set(json.loads(methods)['methods']))
    ctx.equal('all composition methods exercised by the installed SDK offer',proof['negotiated_composition_methods'],sorted(json.loads(methods)['methods']))
    ctx.write('vm-recovery-cuts.json',{'cases':{'H13':proof}})
    return proof
