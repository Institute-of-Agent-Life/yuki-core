"""Management CAS and resume of the actual paid execution, never an operator-owned run."""
import json
import os
import subprocess
import uuid
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def run(ctx, wire):
    o = Owner(ctx, 'control-work', 20)
    token = 'fixture-control-work-token-000000000000000000'
    cfg = o.svc.data/'yuki.toml'
    registry = o.svc.data/'operators.toml'
    registry.write_text('[[operators]]\nprincipal_id="'+str(uuid.uuid4())+'"\ntoken_env="CONTROL_WORK"\ncapabilities=["control.work.read","control.work.mutate","control.operation.read"]\n')
    cfg.write_text('operator_registry_file="operators.toml"\n'+cfg.read_text())
    env = {**os.environ,'CONTROL_WORK':token}
    def request(**values): return {'request_id':str(uuid.uuid4()),**values}
    def api(path, body=None, status=200):
        code,value=wire(o.svc.address,'/admin/v1/'+path,token,body)
        if code!=status:ctx.write('work-admin-failure.json',{'path':path,'status':code,'response':value})
        ctx.equal('Work Admin '+path+' status',code,status)
        return value.get('data',value)
    with Recorder() as peer:
        o.config(peer)
        with o.svc.running(environment=env):
            event=o.human('retain private-source-canary while administrating original Work')
            original,q=o.wire(peer)
            reply(q,calls=[call('accept','task_control',{'action':'accept','goal':'original Work goal'})])
            _,q=o.wire(peer)
            work,execution=o.rows('SELECT id,owner_execution FROM work_nodes')[0]
            reply(q,calls=[call('wait','task_control',{'action':'need_input','work_id':work,'reason':'new original input required'})])
            o.until(lambda:o.rows("SELECT id FROM work_nodes WHERE id=? AND status='need_input'",work))
            o.until(lambda:o.rows("SELECT id FROM executions WHERE id=? AND state='completed'",execution))
            detail=api('works/'+work)
            ctx.write('work-waiting-diagnostic.json',{'detail':detail,'metadata':o.rows('SELECT * FROM work_admin_metadata'),'turns':o.rows('SELECT execution,window,material FROM main_turns'),'windows':o.rows('SELECT * FROM main_windows')})
            ctx.equal('original waiting status exposed',detail['state'],'waiting_user')
            ctx.check('resumable actions use original state',detail['actions']['resume'])
            ctx.equal('paid logical count comes from original attempts',detail['model_requests'],2)
            ctx.check('new Work has observed creation and active timing',detail['created'] is not None and detail['active_seconds'] is not None and detail['active_seconds']>=0)
            ctx.check('metadata omits private input bodies','private-source-canary' not in json.dumps(detail))
            ctx.equal('filtered directory returns actual Work',api('works?state=waiting_user')['items'][0]['id'],work)
            ctx.equal('wrong state directory is empty',api('works?state=cancelled')['items'],[])
            ctx.equal('OS CLI sees same settled Work metadata',o.op('control','read','work.get',json.dumps({'id':work})),detail)
            api('works/'+work+'/resume',request(expected_revision=detail['revision']-1),409)
            before=o.rows('SELECT id,request_json FROM model_requests ORDER BY id')
            resume=request(expected_revision=detail['revision'])
            resumed=api('works/'+work+'/resume',resume)
            next_wire,answer=o.wire(peer)
            ctx.equal('resume retains original execution',o.rows('SELECT execution FROM work_runs WHERE work=?',work),[(execution,)])
            ctx.check('resume preserves paid wire prefix',all(row in o.rows('SELECT id,request_json FROM model_requests') for row in before))
            ctx.equal('resume replays original result despite live next request',api('works/'+work+'/resume',resume),resumed)
            ctx.check('same paid chain receives explicit management signal','Resume the original Work' in str(next_wire['messages']))
            ctx.equal('resume does not replace original persona',next_wire['messages'][0],original['messages'][0])
            ctx.equal('resume creates no extra budget root',o.rows('SELECT count(*) FROM root_budgets'),[(1,)])
            # An in-flight paid request cannot be treated as a completed resume or retried.
            live=api('works/'+work)
            api('works/'+work+'/resume',request(expected_revision=live['revision']),409)
            cancelled=api('works/'+work+'/cancel',request(expected_revision=live['revision']))
            ctx.equal('cancel immediately revokes original run',o.rows('SELECT cancelled FROM executions WHERE id=?',execution),[(1,)])
            o.until(lambda:o.rows("SELECT id FROM model_attempts WHERE state='unknown'"))
            reply(answer,'late paid result')
            closed=api('works/'+work)
            ctx.equal('cancelled Work remains cancelled after late response',closed['state'],'cancelled')
            ctx.check('cancelled Work cannot resume or cancel again',not closed['actions']['resume'] and not closed['actions']['cancel'])
            api('works/'+work+'/resume',request(expected_revision=closed['revision']),409)
            ctx.equal('cancel did not reset paid request count',closed['model_requests'],3)
            ctx.equal('cancel made no synthetic public delivery',len(peer.deliveries),0)
            output={'waiting':detail,'resume':resumed,'cancel':cancelled,'final':closed,'original_execution':execution}
        with o.svc.running(environment=env):
            ctx.equal('cancel and original request IDs survive restart',api('works/'+work)['state'],'cancelled')
            ctx.equal('cold restart cannot create a replacement execution',o.rows('SELECT execution FROM work_runs WHERE work=?',work),[(execution,)])
            old=registry.read_text();registry.write_text(old.replace('"control.work.read"','"control.health.read"'))
            api('works/'+work,status=403)
    output['faults']=faults(ctx, wire)
    ctx.write('work-management.json',output)
    return output


def faults(ctx, wire):
    o=Owner(ctx,'control-work-faults',20)
    token='fixture-control-work-faults-token-00000000000'
    cfg=o.svc.data/'yuki.toml'
    (o.svc.data/'operators.toml').write_text('[[operators]]\nprincipal_id="'+str(uuid.uuid4())+'"\ntoken_env="CONTROL_WORK"\ncapabilities=["control.work.read","control.work.mutate"]\n')
    cfg.write_text('operator_registry_file="operators.toml"\n'+cfg.read_text())
    env={**os.environ,'CONTROL_WORK':token}
    def api(path, body=None, status=200):
        code,value=wire(o.svc.address,'/admin/v1/works'+path,token,body)
        if code!=status:ctx.write('work-fault-admin-failure.json',{'path':path,'status':code,'response':value})
        ctx.equal('Work fault Admin '+path+' status',code,status)
        return value.get('data',value)
    def action(work, verb, status=200):
        return api('/'+work+'/'+verb,{'request_id':str(uuid.uuid4()),'expected_revision':api('/'+work)['revision']},status)
    def accept(peer, goal):
        event=o.human(goal)
        _,q=o.wire(peer)
        reply(q,calls=[call('accept','task_control',{'action':'accept','goal':goal})])
        body,q=o.wire(peer)
        work,execution=o.rows('SELECT w.id,w.owner_execution FROM work_nodes w JOIN executions x ON x.id=w.owner_execution WHERE x.source=?',event)[0]
        return work,execution,body,q
    def waiting(peer, goal):
        work,execution,_,q=accept(peer,goal)
        reply(q,calls=[call('wait','task_control',{'action':'need_input','work_id':work,'reason':'wait for original input'})])
        o.until(lambda:o.rows("SELECT id FROM executions WHERE id=? AND state='completed'",execution))
        return work,execution
    with Recorder() as peer:
        o.config(peer)
        with o.svc.running(environment=env):
            work,execution=waiting(peer,'original source revocation')
            o.op('person','set-enabled',o.actor,'false')
            ctx.check('revoked original actor is not offered resume',not api('/'+work)['actions']['resume'])
            action(work,'resume',409)
            o.op('person','set-enabled',o.actor,'true')
            ctx.check('re-enabled original actor restores precondition',api('/'+work)['actions']['resume'])
            reset=o.human('/ai new')
            o.until(lambda:o.rows('SELECT event FROM main_controls WHERE event=?',reset))
            ctx.check('old conversation generation cannot resume',not api('/'+work)['actions']['resume'])
            action(work,'resume',409)
            ctx.equal('denied resume leaves original paid count',api('/'+work)['model_requests'],2)
            action(work,'cancel')
            timed,timed_run,_,q=accept(peer,'resume original external wait')
            reply(q,calls=[call('external-wait','task_control',{'action':'wait','conditions':[{'kind':'time_due','after_seconds':3600}]})])
            o.until(lambda:o.rows("SELECT id FROM executions WHERE id=? AND state='completed'",timed_run))
            ctx.equal('actual armed wait projects external waiting',api('/'+timed)['state'],'waiting_external')
            action(timed,'resume')
            body,q=o.wire(peer)
            ctx.check('manual resume receives original wait and explicit signal','Resume the original Work' in str(body['messages']))
            ctx.equal('manual resume cancels only original armed wait',o.rows('SELECT state FROM work_waits WHERE work=?',timed),[('cancelled',)])
            reply(q,calls=[call('immediate-wait','task_control',{'action':'wait','conditions':[{'kind':'time_due','after_seconds':0}]})])
            body,q=o.wire(peer)
            signal=api('/'+timed)['signal']
            ctx.check('actual wait delivery has observed timestamp',signal['delivered'] is not None)
            ctx.equal('actual signal consumption is projected',signal['input_state'],'consumed')
            reply(q,'completed after original signal')
            o.until(lambda:o.rows("SELECT id FROM work_nodes WHERE id=? AND status='completed'",timed))
            parent,parent_run,_,q=accept(peer,'cancel complete parent and child family')
            reply(q,calls=[call('child','subagent_start',{'goal':'retained child','context':'private child context'})])
            a,qa=o.wire(peer);b,qb=o.wire(peer)
            child,child_run=o.rows('SELECT w.id,r.execution FROM work_nodes w JOIN work_runs r ON r.work=w.id WHERE w.parent=?',parent)[0]
            ctx.equal('delegated Work records original window',api('/'+child)['generation'],api('/'+parent)['generation'])
            action(parent,'cancel')
            o.until(lambda:o.rows("SELECT count(*) FROM model_attempts WHERE state='unknown'")[0][0]>=2)
            reply(qa,'late parent or child');reply(qb,'late child or parent')
            ctx.equal('recursive cancellation marks both actual runs',o.rows('SELECT id,cancelled FROM executions WHERE id IN (?,?) ORDER BY id',parent_run,child_run),sorted([(parent_run,1),(child_run,1)]))
            ctx.equal('recursive cancellation terminates both Work nodes',[api('/'+i)['state'] for i in (parent,child)],['cancelled','cancelled'])
            route_work,route_run=waiting(peer,'original route closure')
            o.op('account','end',o.bot['assignment'],999999)
            ctx.check('ended original receiver is not offered resume',not api('/'+route_work)['actions']['resume'])
            action(route_work,'resume',409)
            action(route_work,'cancel')
            ctx.equal('management never emitted a synthetic delivery',peer.deliveries,[])
    # Kill a real process with an outstanding paid request. The cold owner must
    # retain that attempt and acknowledge missing timing, never count downtime.
    cold=Owner(ctx,'control-work-cold',20)
    with Recorder() as peer:
        cold.config(peer)
        process=subprocess.Popen([*cold.svc.argv,'run'],stdout=subprocess.PIPE,text=True)
        try:
            ctx.equal('crash fixture owner ready',json.loads(process.stdout.readline())['status'],'ready')
            event=cold.human('unknown paid attempt at process death')
            _,q=cold.wire(peer)
            reply(q,calls=[call('accept','task_control',{'action':'accept','goal':'survive process death'})])
            _,q=cold.wire(peer)
            work,execution=cold.rows('SELECT id,owner_execution FROM work_nodes')[0]
            before=cold.rows('SELECT id,request_json FROM model_requests ORDER BY id')
            ctx.check('live run records observed timing',cold.op('control','read','work.get',json.dumps({'id':work}))['active_seconds'] is not None)
            process.kill();process.wait(timeout=10)
            reply(q,'response lost after death')
        finally:
            if process.poll() is None:process.kill();process.wait(timeout=10)
            process.stdout.close()
        with cold.svc.running():
            cold.until(lambda:cold.rows("SELECT id FROM executions WHERE id=? AND state='held'",execution))
            detail=cold.op('control','read','work.get',json.dumps({'id':work}))
            ctx.equal('cold crash time is explicitly unknown',detail['active_seconds'],None)
            ctx.equal('unknown original run is suspended',detail['state'],'suspended')
            attempts=cold.rows('SELECT id,state FROM model_attempts ORDER BY id')
            ctx.check('real missing HTTP reply is retained unknown',any(state=='unknown' for _,state in attempts))
            receipt=cold.op('control','write','work.resume',json.dumps({'id':work,'request_id':str(uuid.uuid4()),'expected_revision':detail['revision']}))
            cold.until(lambda:cold.rows("SELECT id FROM executions WHERE id=? AND state='held'",execution))
            ctx.equal('resume cannot spend again after unknown reply',cold.rows('SELECT id,state FROM model_attempts ORDER BY id'),attempts)
            ctx.equal('resume preserves exact paid requests after process death',cold.rows('SELECT id,request_json FROM model_requests ORDER BY id'),before)
            ctx.equal('resume of unknown retains one original execution',cold.rows('SELECT execution FROM work_runs WHERE work=?',work),[(execution,)])
            ctx.check('resume of unknown never emitted HTTP',peer.wires.empty())
            ctx.equal('held Work exposes actual unknown pause reason',cold.op('control','read','work.get',json.dumps({'id':work}))['pause_reason'],'unknown')
            ctx.equal('suspended listing and detail agree',cold.op('control','read','work.list',json.dumps({'state':'suspended'}))['items'][0]['id'],work)
    return {'revocation_and_route_checked':True,'generation_checked':True,'subtree':[parent,child],'crash':{'work':work,'execution':execution,'attempts':attempts,'resume':receipt}}
