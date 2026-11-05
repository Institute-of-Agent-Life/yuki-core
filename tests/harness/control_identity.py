"""Original Admin person/binding operations with durable receipts and live authority."""
import json
import os
import uuid
from tests.harness.compact import Owner


def run(ctx, wire):
    owner = Owner(ctx, 'control-identity', 20)
    token = 'fixture-control-identity-token-000000000000000'
    cfg = owner.svc.data / 'yuki.toml'
    cfg.write_text('operator_registry_file="operators.toml"\n' + cfg.read_text())
    (owner.svc.data / 'operators.toml').write_text('[[operators]]\nprincipal_id="' + str(uuid.uuid4()) + '"\ntoken_env="CONTROL_IDENTITY"\ncapabilities=' + json.dumps([v for v in json.loads((ctx.root / 'protocol/admin/capabilities.json').read_text())['capabilities'] if v.startswith('identity.') or v=='control.operation.read']) + '\n')
    env = {**os.environ, 'CONTROL_IDENTITY': token}
    receipts = []
    def request(**values): return {'request_id':str(uuid.uuid4()), **values}
    def api(path, body=None, expected=200, selected=token):
        code, value = wire(owner.svc.address, '/admin/v1/' + path, selected, body)
        if code != expected: ctx.write('identity-http-failure.json',{'path':path,'status':code,'expected':expected,'problem':value.get('problem')})
        ctx.equal('Admin ' + path + ' status', (code, value.get('problem')), (expected, None) if expected == 200 else (expected, value.get('problem')))
        return value.get('data', value)
    with owner.svc.running(environment=env):
        create = request(expected_revision=0, display_name='  original owner  ')
        created = api('persons',create)
        person = created['resource_id']
        ctx.equal('create normalizes original display name', api('persons/' + person)['display_name'], 'original owner')
        ctx.equal('same create request never creates another person',api('persons',create),created)
        api('persons',{**create,'display_name':'different'},409)
        api('persons',{**request(expected_revision=0),'id':person},422)
        receipt = api('operations/' + create['request_id'])
        ctx.equal('control receipt identifies actual created resource',receipt['result_resource_id'],person)
        receipts.append(receipt)
        # A write method cannot become a read shortcut, even on the OS channel.
        owner.svc.command('operator','control','read','person.disable',json.dumps({'id':person}),accepted=False)
        owner.svc.command('operator','control','write','person.get',json.dumps({'id':person,**request()}),accepted=False)
        page=api('persons?limit=1')
        ctx.equal('person page has exactly requested size',len(page['items']),1)
        ctx.check('person page cursor has an actual continuation',bool(page['next_cursor']) and bool(api('persons?after='+page['next_cursor'])['items']))
        rename = request(display_name='renamed owner')
        ctx.equal('rename returns original v4 result',api('persons/'+person+'/rename',rename),{'person_id':person,'replay':False})
        ctx.equal('rename replay is explicit',api('persons/'+person+'/rename',rename),{'person_id':person,'replay':True})
        ctx.equal('rename retains original name once',owner.rows('SELECT name FROM person_names WHERE person=?',person),[('original owner',)])
        before=api('persons/'+person)
        api('persons/'+person+'/disable',request(expected_revision=before['revision']-1),409)
        disable=request(expected_revision=before['revision'])
        disabled=api('persons/'+person+'/disable',disable)
        ctx.equal('disabled person is actually disabled',api('persons/'+person)['enabled'],False)
        ctx.equal('same disable replays before revision check',api('persons/'+person+'/disable',disable),disabled)
        api('persons/'+person+'/enable',request(expected_revision=api('persons/'+person)['source_revision']))
        # Current device grants follow the canonical root, not the device's claimed role.
        pair=request()
        code=api('persons/'+person+'/client-pair',pair)
        ctx.equal('one-time code absent on replay',api('persons/'+person+'/client-pair',pair)['code'],None)
        status, paired=wire(owner.svc.address,'/client/v1/pair','',{'code':code['code'],'device_label':'identity test'})
        ctx.equal('actual first-party device paired',status,200)
        device=paired['data']; bearer=device['token']
        api('persons/'+person,expected=403,selected=bearer)
        api('persons/'+person+'/superuser/grant',request(expected_revision=api('persons/'+person)['revision']))
        ctx.equal('current superuser device reads actual current person',api('persons/'+person,selected=bearer)['id'],person)
        binding_request=request(expected_revision=api('persons/'+person)['revision'],platform='qq',external_account_id='3701',display_name='binding source')
        bound=api('persons/'+person+'/bindings',binding_request)
        binding=bound['resource_id']
        original=api('bindings/'+binding)
        ctx.equal('binding owns the original person',original['person_id'],person)
        ctx.check('binding known start is ISO time',isinstance(original['started_at'],str) and 'T' in original['started_at'])
        unknown=owner.op('account','open','qq','compact','3702',2,'--person',person)['assignment']
        ctx.equal('unknown assignment start remains unknown',api('bindings/'+unknown)['started_at'],None)
        ctx.equal('new binding replay retains stage',api('persons/'+person+'/bindings',binding_request),bound)
        ctx.equal('actual external-account filter',api('bindings?platform=qq&external_account_id=3701')['items'][0]['id'],binding)
        api('bindings/'+binding+'/unbind',request(expected_revision=original['revision']),403,selected=bearer)
        ctx.equal('denied operator-only mutation leaves stage active',api('bindings/'+binding)['status'],'active')
        api('bindings/'+binding+'/disable',request(expected_revision=original['revision']))
        api('bindings/'+binding+'/enable',request(expected_revision=original['revision']),409)
        api('bindings/'+binding+'/enable',request(expected_revision=api('bindings/'+binding)['revision']))
        target=api('persons',request(expected_revision=0,display_name='target'))['resource_id']
        transfer=request(to_person=target)
        transferred=api('bindings/'+binding+'/transfer',transfer)
        current=transferred['binding_id']
        ctx.check('transfer creates a different stage',current!=binding)
        ctx.equal('old assignment original person never changes',api('bindings/'+binding)['person_id'],person)
        ctx.equal('old assignment ended',api('bindings/'+binding)['status'],'ended')
        ctx.equal('new stage actual target',api('bindings/'+current)['person_id'],target)
        ctx.equal('same transfer returns original successor',api('bindings/'+binding+'/transfer',transfer)['binding_id'],current)
        unbind=request(expected_revision=api('bindings/'+current)['revision'])
        ended=api('bindings/'+current+'/unbind',unbind)
        ctx.equal('OS-deployed operator can actually unbind',ended['effective_state']['status'],'ended')
        ctx.equal('unbind replay retains original revision',api('bindings/'+current+'/unbind',unbind),ended)
        api('bindings/'+current+'/enable',request(expected_revision=ended['revision']),409)
        # A disabled alias and disabled root are independently restored by the original enable route.
        owner.op('person','set-enabled',person,'false')
        merge=request(to_person=target)
        api('persons/'+person+'/merge',merge)
        before=api('persons/'+person)
        ctx.equal('merge retains disabled original alias',before['source_enabled'],False)
        api('persons/'+target+'/disable',request(expected_revision=before['revision']))
        api('persons/'+person+'/enable',request(expected_revision=before['source_revision']))
        after=api('persons/'+person)
        ctx.check('enable restores both original alias and current root',after['source_enabled'] and after['enabled'])
        api('persons/'+target+'/superuser/revoke',request(expected_revision=after['revision']))
        api('persons/'+target,expected=403,selected=bearer)
        revoke=request()
        revoked=api('devices/'+device['device_id']+'/revoke',revoke)
        ctx.equal('actual original device revoke result',revoked,{'device_id':device['device_id'],'revoked':True,'replay':False})
        ctx.equal('revoke receipt survives retry',api('devices/'+device['device_id']+'/revoke',revoke)['replay'],True)
        ctx.equal('revocation blocks existing browser token',wire(owner.svc.address,'/client/v1/character',bearer)[0],401)
        forgotten=api('persons/'+target+'/forget',request())
        ctx.check('forget invokes actual closure',forgotten['deleted'] and 'managed_storage' in forgotten)
        ctx.equal('erase scrubs private identity receipt',api('operations/'+rename['request_id'])['status'],'succeeded')
        values=owner.rows("SELECT value FROM core_meta WHERE key LIKE 'control-receipt:%'")
        ctx.check('forgotten names and account labels absent from stored receipt results',all('binding source' not in v[0] and 'renamed owner' not in v[0] for v in values))
        ctx.equal('forgotten old aliases removed',owner.rows('SELECT name FROM person_names WHERE person=?',person),[])
        ctx.equal('forgotten binding label removed',api('bindings/'+binding)['display_name'],'')
        api('persons/'+target+'/enable',request(expected_revision=api('persons/'+target)['revision']),409)
    with owner.svc.running(environment=env):
        ctx.equal('cold restart preserves exact create receipt metadata',api('operations/'+create['request_id'])['result_resource_id'],person)
        ctx.equal('cold restart preserves ended stage',api('bindings/'+current)['status'],'ended')
    return {'person':person,'transferred_stage':current,'original_receipt':receipts[0], 'surface':'actual Admin HTTP and shared local operator', 'private_device_token_saved':False}
