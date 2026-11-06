"""R04: configuration edits do not spawn stopped, new, or failed plugins."""
import hashlib
import json
import os
import sys
import time
import uuid
from concurrent.futures import ThreadPoolExecutor
from tests.harness.compact import Owner
from tests.harness.crash import kill_writer


def wait(predicate):
    end = time.monotonic() + 12
    while time.monotonic() < end:
        try:
            value = predicate()
            if value:
                return value
        except (OSError, ValueError):
            pass
        time.sleep(0.02)
    raise AssertionError('real plugin state did not reach expected barrier')


def run(ctx, wire):
    owner = Owner(ctx, 'control-offline-plugin', 20)
    folder = ctx.working / 'configuration-peer'
    folder.mkdir()
    config = owner.svc.data / 'yuki.toml'
    token = 'fixture-plugin-config-operator-000000000000000'
    env = {**os.environ, 'CONTROL_CONFIG': token}
    registry = owner.svc.data / 'operators.toml'
    registry.write_text('[[operators]]\nprincipal_id="' + str(uuid.uuid4()) + '"\ntoken_env="CONTROL_CONFIG"\ncapabilities=["control.plugin.config.content.read","control.plugin.config.mutate","control.plugin.read","control.plugin.mutate","control.operation.read","control.health.read"]\n')
    config.write_text('operator_registry_file="operators.toml"\n' + config.read_text() + '\n' + f'''
[[plugins]]
id="offline.fixture"
namespace="offline.fixture"
program={json.dumps(sys.executable)}
args={json.dumps([str(ctx.root / 'tests/peers/control/configuration.py'), str(folder), str(ctx.control_python_sdk)])}
enabled=false
grants=["plugin.config.read","plugin.secrets.read","plugin.observe"]
[plugins.configuration]
secret_fields=["api_key"]
[plugins.configuration.schema]
type="object"
additionalProperties=false
required=["endpoint"]
[plugins.configuration.schema.properties.endpoint]
type="string"
minLength=1
[plugins.configuration.schema.properties.label]
type="string"
default="unset"
[plugins.configuration.schema.properties.api_key]
type="string"
writeOnly=true
''')
    path = '/admin/v1/plugins/offline.fixture'
    def action(name, extra=None, expected=200, body=None):
        if body is None:
            current = wire(owner.svc.address, path, token)[1]['data']
            body = {'request_id':str(uuid.uuid4()),'expected_revision':current['revision'],**(extra or {})}
        code, result = wire(owner.svc.address,path+'/actions/'+name,token,body,'POST')
        ctx.equal('actual plugin '+name+' result',code,expected)
        return result.get('data',result),body
    def receipt(body):
        return wire(owner.svc.address,'/admin/v1/operations/'+body['request_id'],token)[1]['data']
    def read():
        status, data = wire(owner.svc.address, path + '/configuration', token)
        ctx.equal('configuration is readable without a runtime instance', status, 200)
        return data['data']
    def save(document, endpoint, secret, request=None, expected=200):
        body = {'request_id': request or str(uuid.uuid4()), 'expected_revision': document['revision'],
                'values': {'endpoint': endpoint, 'api_key': secret}}
        status, data = wire(owner.svc.address, path + '/configuration', token, body, 'POST')
        ctx.equal('configuration mutation outcome', status, expected)
        return data.get('data', data), body
    def starts():
        file = folder / 'starts.jsonl'
        return len(file.read_text().splitlines()) if file.exists() else 0
    def status():
        return wire(owner.svc.address, path, token)[1]['data']['status']
    def ready(endpoint):
        return wait(lambda: json.loads((folder / 'ready.json').read_text()).get('endpoint') == endpoint)
    def health(expected):
        code, result = wire(owner.svc.address, '/admin/v1/health', token)
        ctx.equal('authorized health uses original Admin list envelope', code, 200)
        items = {v['name']:v for v in result['data']}
        ctx.equal('health follows actual installed process', items['plugins']['running'], expected)
        ctx.check('health checks actual writer and reader', items['database']['writer_accepting'] and items['database']['read_available'])
        ctx.equal('no configured Main means no running Work scheduler', items['work']['running'], False)
        ctx.equal('unobserved remote completion health stays unknown', items['persistent_environment']['completion_receiver_running'], None)
        return items
    with owner.svc.running(environment=env):
        initial = read()
        health(False)
        doctor,doctor_body=action('doctor')
        ctx.equal('doctor validates stopped required settings',doctor['effective_state']['diagnostics']['problems'],['configuration_invalid'])
        ctx.equal('doctor never launches stopped plugin',starts(),0)
        action('enable',body=doctor_body,expected=409)
        ctx.equal('doctor has actual operation receipt',receipt(doctor_body)['status'],'succeeded')
        ctx.equal('new installation definition exists before first enable', initial['valid'], False)
        ctx.equal('schema default available without launch', initial['values'], {'label': 'unset'})
        ctx.equal('initially disabled does not launch', starts(), 0)
        ctx.equal('offline observation does not masquerade as configuration', wire(owner.svc.address, path + '/observe', token)[0], 422)
        changed, body = save(initial, 'first', {'action': 'replace', 'value': 'PRIVATE-PLUGIN-CANARY'})
        ctx.equal('public configuration only exposes secret presence', changed['secrets'], {'api_key': {'configured': True}})
        ctx.check('secret value omitted from mutation response', 'PRIVATE-PLUGIN-CANARY' not in json.dumps(changed))
        ctx.equal('new offline edit does not launch', starts(), 0)
        ctx.equal('identical configuration mutation replays original result', wire(owner.svc.address, path + '/configuration', token, body, 'POST')[1]['data'], changed)
        save(initial, 'conflict', {'action': 'keep'}, expected=409)
        save(changed, 'invalid-secret', '', expected=422)
        save(changed, 'missing-secret', {}, expected=422)
        enabled,enable_body=action('enable')
        ctx.equal('same enable request replays original receipt',action('enable',body=enable_body)[0],enabled)
        ctx.equal('enable receipt settles only after actual handshake',receipt(enable_body)['status'],'succeeded')
        ready('first')
        ctx.equal('explicit first enable starts exactly one process', starts(), 1)
        health(True)
        ctx.equal('health read does not launch another process', starts(), 1)
        observed = wire(owner.svc.address, path + '/observe', token)
        ctx.equal('running observation uses real plugin', observed[0], 200)
        ctx.equal('observation returns original running configuration', observed[1]['data']['endpoint'], 'first')
        ctx.equal('only selected secret reaches installed plugin', json.loads((folder / 'ready.json').read_text())['secret_sha256'], hashlib.sha256(b'PRIVATE-PLUGIN-CANARY').hexdigest())
        # Authentication and generation are checked again after the real observation returns.
        (folder / 'observing').unlink()
        (folder / 'hold').touch()
        with ThreadPoolExecutor(max_workers=1) as pool:
            response = pool.submit(wire, owner.svc.address, path + '/observe', token)
            wait(lambda: (folder / 'observing').exists())
            declaration = registry.read_text()
            registry.write_text(declaration.replace('capabilities=[', 'enabled=false\ncapabilities=['))
            (folder / 'hold').unlink()
            ctx.equal('revocation while plugin observes rejects late response', response.result()[0], 409)
            registry.write_text(declaration)
        action('disable')
        wait(lambda: status() == 'disabled')
        health(False)
        changed, _ = save(read(), 'fail-start', {'action': 'keep'})
        ctx.equal('stopped plugin edit does not restart it', starts(), 1)
        owner.op('host', 'enable', 'offline.fixture', 'true')
        wait(lambda: status() == 'failed')
        health(False)
        ctx.equal('failed explicit enable attempted one process', starts(), 2)
        changed, _ = save(read(), 'recovered', {'action': 'clear'})
        ctx.equal('failed plugin configuration repair does not restart it', starts(), 2)
        ctx.equal('secret clear is separate from keep', changed['secrets'], {'api_key': {'configured': False}})
        owner.op('host', 'enable', 'offline.fixture', 'true')
        ready('recovered')
        ctx.equal('explicit retry starts repaired plugin', starts(), 3)
        ctx.equal('cleared secret absent in actual plugin', json.loads((folder / 'ready.json').read_text())['secret_sha256'], hashlib.sha256(b'').hexdigest())
        action('disable')
        scoped = owner.op('control','read','plugin.configuration.read',json.dumps({'plugin_id':'offline.fixture','scope_type':'user','owner_id':owner.actor}))
        scoped_body = {'plugin_id':'offline.fixture','scope_type':'user','owner_id':owner.actor,'request_id':str(uuid.uuid4()),'expected_revision':scoped['revision'],'values':{'endpoint':'PRIVATE-USER-CONFIG','api_key':{'action':'replace','value':'PRIVATE-USER-SECRET'}}}
        owner.op('control','write','plugin.configuration.save',json.dumps(scoped_body))
        ctx.check('scope-owned secret is stored only in private configuration', bool(owner.rows("SELECT key FROM core_meta WHERE key LIKE 'plugin-config-values:%' AND instr(value,'PRIVATE-USER-SECRET')>0")))
        owner.op('privacy','forget','config-person-forget',owner.actor)
        ctx.equal('person erasure removes scoped settings and receipt body',owner.rows("SELECT key FROM core_meta WHERE instr(value,'PRIVATE-USER-SECRET')>0 OR instr(value,'PRIVATE-USER-CONFIG')>0"),[])
        action('approve',{'permissions':['made.up.superuser']},expected=409)
        approved,approval_body=action('approve',{'permissions':['plugin.config.read','plugin.secrets.read']})
        shown=wire(owner.svc.address,path,token)[1]['data']
        ctx.equal('requested definition survives narrower approval',shown['requested_permissions'],['plugin.config.read','plugin.observe','plugin.secrets.read'])
        ctx.equal('only explicitly approved grants remain effective',shown['approved_permissions'],['plugin.config.read','plugin.secrets.read'])
        ctx.equal('approval does not start a process',starts(),3)
        (folder/'fail-handshake').touch()
        _,failed_body=action('enable',expected=409)
        ctx.equal('failed handshake records failed operation',receipt(failed_body)['status'],'failed')
        ctx.equal('failed handshake has bounded public error',receipt(failed_body)['error_category'],'precondition_failed')
        action('enable',body=failed_body,expected=409)
        ctx.equal('failed original request never launches a retry',starts(),4)
        (folder/'fail-handshake').unlink()
        action('enable')
        wait(lambda:starts()==5)
        wait(lambda:'plugin.observe' not in [m['name'] for m in json.loads((folder/'negotiated.json').read_text())['method_revisions']])
        ctx.equal('handshake binds only the approved grants',json.loads((folder/'negotiated.json').read_text())['grants'],['plugin.config.read','plugin.secrets.read'])
        ctx.equal('revoked observe grant unavailable in real instance',wire(owner.svc.address,path+'/observe',token)[0],422)
        action('disable')
        declaration=registry.read_text()
        registry.write_text(declaration.replace(',"control.plugin.mutate"',''))
        action('enable',expected=403)
        registry.write_text(declaration)
        ctx.equal('removed lifecycle authority causes no launch',starts(),5)
        before = read()
    with owner.svc.running(environment=env):
        ctx.equal('cold restart preserves disabled configuration', read(), before)
        ctx.equal('cold restart does not launch disabled plugin', starts(), 5)
        shown=wire(owner.svc.address,path,token)[1]['data']
        ctx.equal('cold owner retains effective operator approval',shown['approved_permissions'],['plugin.config.read','plugin.secrets.read'])
        before_crash=owner.op('control','read','plugin.get',json.dumps({'plugin_id':'offline.fixture'}))
    crash_body={'plugin_id':'offline.fixture','request_id':str(uuid.uuid4()),'expected_revision':before_crash['revision']}
    kill_writer(ctx,owner.svc.data,'plugin_management_crash_child','plugin-management-prepared',{'YUKI_PLUGIN_TEST_BODY':json.dumps(crash_body)})
    ctx.equal('committed enable intent before crash spawned no process',starts(),5)
    with owner.svc.running(environment=env):
        wait(lambda:starts()==6)
        pending=owner.op('control','read','operation.get',json.dumps({'request_id':crash_body['request_id']}))
        ctx.equal('cold receipt keeps pending original operation',pending['status'],'pending')
        recovered=owner.op('control','write','plugin.enable',json.dumps(crash_body))
        ctx.equal('original enable recovers existing cold generation',starts(),6)
        ctx.equal('recovered request replays immutable completion',owner.op('control','write','plugin.enable',json.dumps(crash_body)),recovered)
        complete=owner.op('control','read','operation.get',json.dumps({'request_id':crash_body['request_id']}))
        ctx.equal('recovery keeps original operation identity',complete['operation_id'],pending['operation_id'])
        ctx.equal('recovered process completion is observable',complete['status'],'succeeded')
        action('disable')
    return {'starts': starts(), 'scope': 'global', 'offline_states': ['never_enabled', 'stopped', 'start_failed'], 'final_revision': before['revision'],'lifecycle_crash_receipt':complete}
