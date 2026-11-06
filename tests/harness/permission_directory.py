"""Native Main reads current original roles, matching fixed v4 metadata projections."""
import hashlib
import json
from tests.harness.compact import Owner,call,reply
from tests.peers.chat.recorder import Recorder


def run(ctx):
    fixture_path=ctx.root/'fixtures/control/v4-permissions.json'
    fixture=json.loads(fixture_path.read_bytes())
    o=Owner(ctx,'permission-directory',43)
    observations=[]
    with Recorder() as model:
        o.config(model)
        cfg=o.svc.data/'yuki.toml'
        cfg.write_text(cfg.read_text().replace('root_slots=8','root_slots=32').replace('max_output=128','max_output=512').replace('[main]','[main]\nresult_page_budget=32000')+'\n')
        with o.svc.running():
            event=o.human('Read my own authority metadata')
            wire,q=o.wire(model)
            ctx.equal('native Main has the exact original permission declaration',next(t['function'] for t in wire['tools'] if t['function']['name']=='get_my_capabilities'),fixture['declaration'])
            counter=0
            def invoke(args,name='get_my_capabilities'):
                nonlocal q,counter
                counter+=1;key='permissions-'+str(counter)
                reply(q,calls=[call(key,name,args)])
                wire,q=o.wire(model)
                result=json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')==key))
                observations.append({'tool':name,'args':args,'result':result})
                return result
            for case in fixture['cases']:
                o.op('person','set-admin',o.actor,str(case['admin']).lower())
                result=invoke(case['args'])
                ctx.equal('actual permission projection is confirmed',result['state'],'confirmed')
                data=result['result']
                if data.get('truncated'):
                    handle=data['artifact'];raw=o.rows('SELECT body FROM tool_artifacts WHERE handle=?',handle['handle'])[0][0]
                    ctx.equal('large permission directory has the original immutable digest',hashlib.sha256(raw.encode()).hexdigest(),handle['version'])
                    data=json.loads(raw)['result']
                    page=invoke({'handle':handle['handle'],'operation':'get','path':['counts']},'read_tool_artifact')
                    ctx.equal('Main can read the actual directory artifact',page['result']['value'],case['result']['counts'])
                ctx.equal('actual permission projection equals original v4',data,case['result'])
            o.op('person','set-admin',o.actor,'true')
            admin=invoke({})
            o.op('person','set-admin',o.actor,'false')
            ordinary=invoke({})
            ctx.equal('identical query observes live role revocation',(admin['result']['permission_level'],ordinary['result']['permission_level']),('superuser','user'))
            ctx.equal('ordinary projection contains no protected configuration',ordinary['result']['counts']['mutable_configurations'],0)
            invalid=invoke({'mode':'focused'})
            ctx.equal('focused mode requires a real selector',invalid['state'],'failed')
            ctx.equal('metadata lookup creates no management receipts',o.rows("SELECT count(*) FROM core_meta WHERE key LIKE 'control-receipt:%'"),[(0,)])
            reply(q);o.finished(event)
        with o.svc.running():
            event=o.human('Recheck my role after cold start')
            _,q=o.wire(model);reply(q,calls=[call('permissions-cold','get_my_capabilities',{})])
            wire,q=o.wire(model);cold=json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')=='permissions-cold'))
            ctx.equal('restart does not reuse old admin metadata',cold['result'],fixture['cases'][0]['result'])
            reply(q);o.finished(event)
    return {'original_source_commit':fixture['source_commit'],'permission_oracle_sha256':hashlib.sha256(fixture_path.read_bytes()).hexdigest(),'original_projection_cases':len(fixture['cases']),'native_calls':observations,'cold':cold,'metadata_only':True,'private_values_returned':False}
