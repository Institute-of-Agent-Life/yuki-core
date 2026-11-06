"""Actual failed preflight -> CAS requeue -> same QQ effect, with unknown refusal."""
import importlib.util
import json
import os
import sys
import uuid
from concurrent.futures import ThreadPoolExecutor
from tests.harness.compact import Owner
from tests.harness.control_config import wait
from tests.peers.onebot.peer import Peer as QQ


def run(ctx, wire):
    spec = importlib.util.spec_from_file_location('outbox_aux', ctx.root/'tests/cases/CORE-050.py')
    aux = importlib.util.module_from_spec(spec); spec.loader.exec_module(aux)
    owner = Owner(ctx, 'outbox', 40)
    folder = ctx.working/'outbox-peer'; folder.mkdir()
    token = 'fixture-outbox-control-000000000000000000000'
    environment = {**os.environ, 'CONTROL_OUTBOX': token, 'OUTBOX_QQ': 'fixture-outbox-qq'}
    with owner.svc.running():
        entry = owner.op('entry','private',owner.user['assignment'],owner.bot['assignment'])['entry']
    registry = owner.svc.data/'operators.toml'
    registry.write_text('[[operators]]\nprincipal_id='+json.dumps(str(uuid.uuid4()))+'\ntoken_env="CONTROL_OUTBOX"\ncapabilities=["control.plugin.read","control.plugin.mutate","control.operation.read"]\n')
    cfg=owner.svc.data/'yuki.toml'
    cfg.write_text('operator_registry_file="operators.toml"\n'+cfg.read_text()+f'''
[qq]
namespace="compact"
designated_ingress="100"
[[qq.bots]]
self_id="100"
stage={json.dumps(owner.bot['assignment'])}
token_env="OUTBOX_QQ"
[[plugins]]
id="outbox.news"
namespace="outbox.news"
program={json.dumps(sys.executable)}
args={json.dumps([str(ctx.root/'tests/peers/aux-model/peer.py'), str(folder),str(ctx.control_python_sdk)])}
grants=["notification.publish","forget.owner"]
[plugins.notifications]
agent=false
[plugins.notifications.targets]
{json.dumps(entry)}={json.dumps(owner.bot['assignment'])}
''')
    peer = aux.Peer(owner,folder)
    def publish(key):
        return peer.call('notification.publish',{'event_key':key,'event_type':'fixture','external_source':'control-case',
            'target':{'target_type':'private','target_id':owner.actor,'conversation_id':entry},
            'occurred_at':'2026-11-05T00:00:00Z','summary':'outbox','text':'original '+key})
    def row(publication):
        return owner.rows('SELECT c.id,c.revision,d.state,c.attempts FROM notification_outbox_control c JOIN notification_deliveries d USING(publication,ordinal) WHERE publication=?', publication)[0]
    def retry(item,body=None):
        body=body or {'request_id':str(uuid.uuid4()),'expected_revision':item[1]}
        return wire(owner.svc.address,'/admin/v1/plugin-outbox/'+str(item[0])+'/retry',token,body,'POST'),body
    def receive(qq, acknowledge=True):
        while True:
            frame=qq.receive()
            if frame['action']=='send_private_msg': break
            qq.send({'echo':frame.get('echo'),'status':'ok','retcode':0,'data':{}})
        if acknowledge: qq.send({'echo':frame['echo'],'status':'ok','retcode':0,'data':{'message_id':37001}})
        return frame
    with owner.svc.running(environment=environment):
        wait((folder/'ready.json').exists)
        original=publish('safe-offline')
        wait(lambda:row(original['notification_id'])[2]=='failed')
        item=row(original['notification_id'])
        ctx.equal('actual offline preflight leaves original effect undispatched',owner.rows('SELECT state FROM operations WHERE namespace=(SELECT delivery_execution FROM notification_publications WHERE id=?)',original['notification_id']),[('intent',)])
        ids=owner.rows('SELECT event,delivery_execution,origin FROM notification_publications WHERE id=?',original['notification_id'])
        ctx.equal('outbox failure is not an unknown physical attempt',owner.op('control','read','plugin.outbox.get',json.dumps({'id':str(item[0])}))['error_category'],'bot_unavailable')
        ((code,_),_)=retry(item,{'request_id':str(uuid.uuid4()),'expected_revision':item[1]-1})
        ctx.equal('outbox compares exact failed revision',code,409)
        initial_registry=registry.read_text();registry.write_text(initial_registry.replace('"control.plugin.mutate"','"control.preference.read"'))
        ctx.equal('revoked operator cannot requeue old safe failure',retry(item)[0][0],403)
        registry.write_text(initial_registry)
        qq=QQ(owner.svc.address,'100','fixture-outbox-qq')
        try:
            requests=[{'request_id':str(uuid.uuid4()),'expected_revision':item[1]} for _ in range(2)]
            with ThreadPoolExecutor(2) as pool: results=list(pool.map(lambda body:retry(item,body)[0],requests))
            ctx.equal('concurrent CAS requeues only one original delivery',sorted(result[0] for result in results),[200,409])
            accepted=requests[next(i for i,v in enumerate(results) if v[0]==200)]
            success=results[next(i for i,v in enumerate(results) if v[0]==200)]
            sent=receive(qq)
            wait(lambda:row(original['notification_id'])[2]=='confirmed')
            ctx.check('receiver gets original failed body','original safe-offline' in json.dumps(sent))
            ctx.equal('safe retry retains original source execution and origin',owner.rows('SELECT event,delivery_execution,origin FROM notification_publications WHERE id=?',original['notification_id']),ids)
            ctx.equal('same retry receipt survives later completion',retry(item,accepted)[0],success)
            ctx.equal('successful physical receipt clears the earlier preflight error',owner.op('control','read','plugin.outbox.get',json.dumps({'id':str(item[0])}))['error_category'],None)
            ctx.equal('confirmed delivery cannot be retried',retry(row(original['notification_id']))[0][0],409)
            ctx.equal('manual requeue creates one actual physical effect',owner.rows('SELECT count(*) FROM social_outbound'),[(1,)])
            unknown=publish('unknown')
            unknown_wire=receive(qq,False)
        finally: qq.close()
        wait(lambda:row(unknown['notification_id'])[2]=='unknown')
        ctx.equal('lost physical acknowledgement cannot be manually retried',retry(row(unknown['notification_id']))[0][0],409)
        exhausted=publish('exhausted')
        for attempt in range(1,6):
            wait(lambda:row(exhausted['notification_id'])[2]=='failed')
            current=row(exhausted['notification_id'])
            ctx.equal('each safe failed dispatch boundary is counted',current[3],attempt)
            ctx.equal('retry count is bounded',retry(current)[0][0],200 if attempt<5 else 409)
        revoked=publish('revoked')
        wait(lambda:row(revoked['notification_id'])[2]=='failed')
        revoked_row=row(revoked['notification_id'])
        owner.op('person','set-enabled',owner.actor,'false')
        denied=retry(revoked_row)[0]
        ctx.equal('operator cannot replace disabled original recipient',denied[0],409)
        ctx.equal('source rejection keeps original state mismatch category',denied[1]['problem']['code'],'state_mismatch')
        owner.op('person','set-enabled',owner.actor,'true')
        operation=wire(owner.svc.address,'/admin/v1/operations/'+accepted['request_id'],token)[1]['data']
        ctx.equal('retry has original common control receipt',operation['status'],'succeeded')
        snapshots=owner.rows('SELECT operation_id,state FROM operations ORDER BY operation_id')
    with owner.svc.running(environment=environment):
        ctx.equal('cold replay retains exact original control result',retry(item,accepted)[0],success)
        ctx.equal('cold unknown refusal stays closed',retry(row(unknown['notification_id']))[0][0],409)
        ctx.equal('cold replay creates no replacement effects',owner.rows('SELECT operation_id,state FROM operations ORDER BY operation_id'),snapshots)
    return {'original':original,'sent':sent,'unknown':unknown_wire,'receipt':operation,'safe_retry':'original effect only','database_schema_version':owner.rows('SELECT max(version) FROM _sqlx_migrations')[0][0],'paid_calls':0}
