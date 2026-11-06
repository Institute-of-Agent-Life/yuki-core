"""Receiving assignment CAS, ingest pause, and immutable original ownership."""
import json
import os
import uuid
from tests.harness.compact import Owner


def run(ctx, wire):
    owner = Owner(ctx, 'control-presence', 20)
    token = 'fixture-control-presence-token-000000000000000'
    cfg = owner.svc.data / 'yuki.toml'
    cfg.write_text('operator_registry_file="operators.toml"\n' + cfg.read_text())
    registry = owner.svc.data / 'operators.toml'
    registry.write_text('[[operators]]\nprincipal_id="' + str(uuid.uuid4()) + '"\ntoken_env="CONTROL_PRESENCE"\ncapabilities=' + json.dumps([v for v in json.loads((ctx.root/'protocol/admin/capabilities.json').read_text())['capabilities'] if v.startswith('identity.') or v == 'control.operation.read']) + '\n')
    env = {**os.environ, 'CONTROL_PRESENCE': token}
    def request(**values): return {'request_id':str(uuid.uuid4()), **values}
    def api(path, body=None, status=200, selected=token):
        code,value = wire(owner.svc.address,'/admin/v1/'+path,selected,body)
        if code != status: ctx.write('presence-failure.json', {'path':path,'status':code,'body':value})
        ctx.equal('presence API '+path+' status',code,status)
        return value.get('data',value)
    with owner.svc.running(environment=env):
        page = api('presences?limit=1')
        ctx.equal('real receiving assignment listed', page['items'][0]['id'], owner.bot['assignment'])
        current = page['items'][0]
        ctx.equal('CLI and Admin share original presence',owner.op('control','read','presence.get',json.dumps({'id':current['id']})),current)
        event = owner.human('original history remains after ingest pause')
        pair = owner.op('personal','issue',owner.actor,str(uuid.uuid4()),'chat')
        code,paired=wire(owner.svc.address,'/client/v1/pair','',{'code':pair['code'],'device_label':'presence superuser'})
        ctx.equal('presence test device paired',code,200)
        owner.op('person','set-admin',owner.actor,'true')
        bearer=paired['data']['token']
        pause=request(expected_revision=current['revision'],ingest_eligible=False)
        result=api('presences/'+current['id'],pause,selected=bearer)
        ctx.equal('pause original request is replayable',api('presences/'+current['id'],pause,selected=bearer),result)
        after=api('presences/'+current['id'])
        ctx.check('ingest pause does not disable receiving account',after['enabled'] and not after['ingest_eligible'])
        api('presences/'+current['id'],request(expected_revision=current['revision'],enabled=False),409)
        frame={'source':'blocked-presence-event','account':owner.user['account'],'bot_stage':current['id'],'occurred':222,'observed':222,'kind':'message','segments':[{'type':'text','text':'must not persist'}]}
        owner.svc.command('operator','ingress','record',json.dumps(frame),'--accept',accepted=False)
        ctx.equal('ingest pause leaves durable history unchanged',owner.rows('SELECT id FROM events'),[(event,)])
        changed=api('presences/'+current['id'],request(expected_revision=after['revision'],enabled=False,ingest_eligible=True))
        ctx.equal('combined state update increments revision once',changed['revision'],after['revision']+1)
        enabled=api('presences/'+current['id'],request(expected_revision=changed['revision'],enabled=True))
        ctx.check('unpaused source is actually admitted',owner.op('ingress','record',json.dumps(frame),'--accept')['accepted'])
        # Operator-only registration is not granted to a human superuser.
        registration=request(expected_revision=0,platform='qq',external_account_id='39001')
        api('presences',registration,403,selected=bearer)
        created=api('presences',registration)
        stage=created['resource_id']
        ctx.equal('registration replay keeps original receiver',api('presences',registration),created)
        api('presences',{**request(expected_revision=0), 'platform':'qq','external_account_id':'39001'},409)
        before=api('presences/'+stage)
        same=request(expected_revision=before['revision'],expected_assignment_id=stage,platform='qq',external_account_id='39001')
        ctx.equal('registering same expected SELF retains original stage',api('presences',same)['resource_id'],stage)
        transfer=request(expected_revision=before['revision'],to_person=owner.actor)
        api('presences/'+stage+'/transfer',transfer,403,selected=bearer)
        moved=api('presences/'+stage+'/transfer',transfer)
        binding=api('bindings/'+moved['resource_id'])
        ctx.equal('transfer creates human successor bound to original person',binding['person_id'],owner.actor)
        ctx.equal('former SELF stage ends',api('presences/'+stage)['status'],'ended')
        ctx.equal('transfer replay returns original successor',api('presences/'+stage+'/transfer',transfer),moved)
        # Taking a human account requires its precise expected assignment and revision.
        replace=request(expected_revision=binding['revision'],expected_assignment_id=binding['id'],platform='qq',external_account_id='39001')
        restored=api('presences',replace)
        ctx.equal('old human owner remains immutable',api('bindings/'+binding['id'])['person_id'],owner.actor)
        ctx.equal('replaced human stage ends',api('bindings/'+binding['id'])['status'],'ended')
        ctx.check('new receiver is not the ended original stage',restored['resource_id'] not in [stage,binding['id']])
        rid=restored['resource_id']
        end=request(expected_revision=restored['revision'])
        stopped=api('presences/'+rid+'/unregister',end)
        ctx.equal('unregister has no invented successor',owner.rows('SELECT count(*) FROM assignments WHERE previous=?',rid),[(0,)])
        api('presences/'+rid,request(expected_revision=stopped['revision'],enabled=True),409)
        api('presences',request(expected_revision=0,platform='app',external_account_id='invented'),409)
        ctx.equal('original receiver and entry survive unrelated changes',api('presences/'+current['id'])['revision'],enabled['revision'])
        result={'original':current,'pause':result,'transferred':moved,'restored':restored,'ended':stopped}
        registry.write_text(registry.read_text().replace('identity.presence.read','identity.binding.read'))
        api('presences/'+rid,status=403)
    with owner.svc.running(environment=env):
        ctx.equal('ingest and ownership transitions survive restart',owner.op('control','read','presence.get',json.dumps({'id':rid}))['status'],'ended')
    ctx.write('presence.json',result)
    return result
