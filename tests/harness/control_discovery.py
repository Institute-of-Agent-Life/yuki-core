"""Discover real local declarations, approve them, then observe actual processes."""
import json
import os
import sys
import uuid
from tests.harness.compact import Owner
from tests.harness.control_config import wait
from tests.harness.crash import kill_writer


def alive(pid):
    if os.name == 'nt':
        import ctypes
        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        kernel.OpenProcess.restype = ctypes.c_void_p
        handle = kernel.OpenProcess(0x00100000, False, pid)
        if not handle:
            return False
        try:
            kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
            return kernel.WaitForSingleObject(handle, 0) == 258
        finally:
            kernel.CloseHandle.argtypes = [ctypes.c_void_p]
            kernel.CloseHandle(handle)
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def run(ctx, wire):
    owner = Owner(ctx, 'disc', 20)
    folder = ctx.working / 'discovered-peer'
    folder.mkdir()
    token = 'discovery-operator-fixture-0000000000000000'
    registry = owner.svc.data / 'operators.toml'
    registry.write_text('[[operators]]\nprincipal_id="'+str(uuid.uuid4())+'"\ntoken_env="DISCOVERY_OPERATOR"\ncapabilities=["control.plugin.read","control.plugin.mutate","control.plugin.config.content.read","control.plugin.config.mutate","control.operation.read"]\n')
    env = {**os.environ, 'DISCOVERY_OPERATOR': token}
    config = owner.svc.data / 'yuki.toml'
    base = 'operator_registry_file="operators.toml"\n' + config.read_text()
    config.write_text(base)
    declaration = f'''
[[plugins]]
id="discovered.fixture"
namespace="discovered.fixture"
program={json.dumps(sys.executable)}
args={json.dumps([str(ctx.root/'tests/peers/control/configuration.py'),str(folder),str(ctx.control_python_sdk)])}
enabled=true
grants=["plugin.config.read","plugin.secrets.read"]
[plugins.configuration]
secret_fields=["api_key"]
[plugins.configuration.schema]
type="object"
additionalProperties=false
required=["endpoint"]
[plugins.configuration.schema.properties.endpoint]
type="string"
[plugins.configuration.schema.properties.api_key]
type="string"
writeOnly=true
'''
    path = '/admin/v1/plugins/discovered.fixture'
    def view():
        return wire(owner.svc.address,path,token)[1]['data']
    def act(action,expected=200,body=None):
        if body is None:
            body={'request_id':str(uuid.uuid4()),'expected_revision':0 if action=='discover' else view()['revision']}
        status,result=wire(owner.svc.address,path+'/actions/'+action,token,body,'POST')
        ctx.equal('discovery lifecycle '+action,status,expected)
        return result.get('data',result),body
    def starts():
        file=folder/'starts.jsonl'
        return [json.loads(line)['pid'] for line in file.read_text().splitlines()] if file.exists() else []
    with owner.svc.running(environment=env):
        ctx.equal('empty owner has no invented installation',wire(owner.svc.address,path,token)[0],404)
        config.write_text(base+declaration)
        act('discover',409,{'request_id':str(uuid.uuid4()),'expected_revision':1})
        ctx.equal('conflicting discover does not install',wire(owner.svc.address,path,token)[0],404)
        discovered,request=act('discover')
        ctx.equal('new declaration requires explicit approval',view()['status'],'discovered')
        ctx.equal('discovery cannot activate configured enabled flag',starts(),[])
        ctx.equal('unapproved declaration has no effective grants',view()['approved_permissions'],[])
        unavailable=wire(owner.svc.address,path+'/configuration',token)
        ctx.equal('unapproved configuration definition stays unavailable',unavailable[0],503)
        ctx.equal('unapproved configuration uses typed unavailable problem',unavailable[1]['problem']['code'],'unavailable')
        act('enable',409)
        doctor,_=act('doctor')
        ctx.equal('doctor identifies missing approval',doctor['effective_state']['diagnostics']['problems'],['approval_required'])
        act('approve')
        old=view()
        settings=wire(owner.svc.address,path+'/configuration',token)[1]['data']
        body={'request_id':str(uuid.uuid4()),'expected_revision':settings['revision'],'values':{'endpoint':'discovered','api_key':{'action':'clear'}}}
        ctx.equal('approved offline settings save',wire(owner.svc.address,path+'/configuration',token,body,'POST')[0],200)
        act('enable')
        wait(lambda:(folder/'ready.json').exists())
        pid=starts()[0]
        ctx.check('discovered installation starts an actual process',alive(pid))
        generation=owner.rows("SELECT generation FROM host_installations WHERE plugin='discovered.fixture'")[0][0]
        act('discover')
        ctx.equal('unchanged discovery retains running generation',owner.rows("SELECT generation FROM host_installations WHERE plugin='discovered.fixture'"),[(generation,)])
        ctx.equal('unchanged discovery never restarts process',starts(),[pid])
        act('disable',409,{'request_id':str(uuid.uuid4()),'expected_revision':old['revision']})
        # Same public manifest, different executable arguments: approval still closes.
        old_args=json.dumps([str(ctx.root/'tests/peers/control/configuration.py'),str(folder),str(ctx.control_python_sdk)])
        new_args=json.dumps([str(ctx.root/'tests/peers/control/configuration.py'),str(folder),str(ctx.control_python_sdk),'changed-deployment'])
        changed=declaration.replace(old_args,new_args)
        original_deployment=view()
        config.write_text(base+changed)
        act('discover')
        wait(lambda:not alive(pid))
        ctx.equal('changed local deployment invalidates prior approval',view()['status'],'discovered')
        ctx.equal('executable change retains public wire manifest',view()['manifest_hash'],original_deployment['manifest_hash'])
        ctx.check('deployment gets a new public revision',view()['revision']!=original_deployment['revision'])
        ctx.equal('changed declaration preserves original call generation as closed',owner.rows("SELECT online,enabled FROM host_installations WHERE plugin='discovered.fixture'"),[(0,0)])
        ctx.equal('old discovery request replays without changing newer declaration',act('discover',body=request)[0],discovered)
        ctx.equal('replayed discovery never restores old approval',view()['status'],'discovered')
        saved_registry=registry.read_text()
        registry.write_text(saved_registry.replace(',"control.plugin.mutate"',''))
        config.write_text('invalid configuration fixture')
        act('discover',403)
        registry.write_text(saved_registry)
        ctx.equal('completed discovery replay does not parse later broken config',act('discover',body=request)[0],discovered)
        act('discover',422)
        config.write_text(base+changed)
    with owner.svc.running(environment=env):
        ctx.equal('cold owner cannot approve a changed declaration implicitly',view()['status'],'discovered')
        ctx.equal('cold unapproved declaration never launches',starts(),[pid])
        act('approve')
        act('enable')
        wait(lambda:len(starts())==2)
        second=starts()[-1]
        ctx.check('new approved deployment starts a different process',second!=pid and alive(second))
        config.write_text(base)
        act('discover')
        wait(lambda:not alive(second))
        ctx.equal('removed installation remains diagnosable',view()['status'],'unavailable')
        act('enable',409)
        act('approve',409)
        ctx.equal('removed installation cannot be revived by stale approval',len(starts()),2)
    config.write_text(base+changed)
    crash_body={'plugin_id':'discovered.fixture','request_id':str(uuid.uuid4()),'expected_revision':0}
    kill_writer(ctx,owner.svc.data,'plugin_discovery_crash_child','plugin-discovery-committed',{'YUKI_PLUGIN_TEST_BODY':json.dumps(crash_body)})
    ctx.equal('committed discovery crash starts no process',len(starts()),2)
    with owner.svc.running(environment=env):
        pending=owner.op('control','read','operation.get',json.dumps({'request_id':crash_body['request_id']}))
        ctx.equal('cold receipt retains pending discovery',pending['status'],'pending')
        ctx.equal('cold refresh retains unapproved discovered state',view()['status'],'discovered')
        recovered=owner.op('control','write','plugin.discover',json.dumps(crash_body))
        ctx.equal('original discovery recovers runtime registry without starting it',len(starts()),2)
        ctx.equal('recovered discovery has its original stable receipt',owner.op('control','write','plugin.discover',json.dumps(crash_body)),recovered)
        complete=owner.op('control','read','operation.get',json.dumps({'request_id':crash_body['request_id']}))
        ctx.equal('discovery recovery preserves operation identity',complete['operation_id'],pending['operation_id'])
        ctx.equal('recovered discovery records completion',complete['status'],'succeeded')
    return {'discovered_request':request['request_id'],'processes':starts(),'changed_deployment_requires_approval':True,'removed_processes_closed':True,'crash_receipt':complete}
