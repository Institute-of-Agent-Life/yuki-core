"""Real Host calls cannot manufacture an actor, admin role, or operator capability."""
import json
import sys
import uuid
from tests.harness.compact import Owner
from tests.harness.control_config import wait


def scenario(ctx, wire, runtime):
    owner = Owner(ctx, 'control-invocation-'+runtime, 20)
    folder = ctx.working / ('control-invocation-peer-'+runtime); folder.mkdir()
    cfg = owner.svc.data / 'yuki.toml'
    program = sys.executable if runtime=='python' else str(ctx.control_rust_sdk)
    args = [str(ctx.root / 'tests/peers/control/invocation.py'), str(folder), str(ctx.control_python_sdk)] if runtime=='python' else [str(folder)]
    with cfg.open('a') as file:
        file.write('\n[[plugins]]\nid="control.fixture"\nnamespace="control.fixture"\nprogram=' + json.dumps(program) + '\nargs=' + json.dumps(args) + '\ngrants=["command.invoke","control.invoke"]\ncommands=["/manage"]\n')
    with owner.svc.running():
        wait(lambda: (folder / 'ready').exists())
        code = owner.op('personal','issue',owner.actor,str(uuid.uuid4()),'chat')['code']
        status, paired = wire(owner.svc.address,'/client/v1/pair','',{'code':code,'device_label':'control public reader'})
        ctx.equal('ordinary chat device pairs for public character',status,200)
        token = paired['data']['token']
        ctx.equal('ordinary chat person reads shared public role',wire(owner.svc.address,'/client/v1/character',token)[0],200)
        ctx.equal('public reader cannot read private SOUL',wire(owner.svc.address,'/client/v1/character/soul',token)[0],403)
        owner.human('/manage original human identity')
        command = wait(lambda: json.loads((folder / 'command.json').read_text()))
        handle = command['params']['handle']
        serial = 0
        def rpc(method, action, args, *, selected=handle, extra=None):
            nonlocal serial
            serial += 1
            params = {'handle':selected,'method':action, 'args' if method=='control.read' else 'body':args}
            params.update(extra or {})
            # The independent peer consumes and unlinks these frames immediately. On
            # Windows a reader can see complete bytes before Python closes the file.
            frame = folder / f'request-{serial}.json'
            stage = frame.with_suffix('.tmp')
            stage.write_text(json.dumps({'jsonrpc':'2.0','id':serial,'method':method,'params':params}),encoding='utf-8')
            stage.replace(frame)
            try:
                return wait(lambda: json.loads((folder / f'response-{serial}.json').read_bytes()))
            except AssertionError as exc:
                ctx.write('control-invocation-barrier.json',{'runtime':runtime,'serial':serial,'method':method,'action':action,'frame_pending':frame.exists(),'host':owner.rows("SELECT enabled,online FROM host_installations WHERE plugin='control.fixture'")})
                raise AssertionError(f'{runtime} control response {serial} ({action}) missing') from exc
        public = rpc('control.read','character.public',{})
        ctx.check('bound ordinary command reads public character','result' in public)
        ctx.check('plugin supplied superuser is rejected by closed schema','error' in rpc('control.read','character.get',{},extra={'superuser':True,'actor':owner.actor}))
        ctx.check('invented invocation handle has no authority','error' in rpc('control.read','character.public',{},selected=str(uuid.uuid4())))
        ctx.check('ordinary caller does not gain admin through plugin grant','error' in rpc('control.read','character.get',{}))
        owner.op('person','set-admin',owner.actor,'true')
        ctx.check('real current superuser has management capability','result' in rpc('control.read','character.get',{}))
        original = rpc('control.read','character.get',{})['result']
        body = {'request_id':str(uuid.uuid4()),'expected_revision':original['revision'],'display_name':'Host original person'}
        ctx.equal('bound host and local domain apply same role update',rpc('control.write','character.update',body)['result']['display_name'],'Host original person')
        ctx.check('superuser does not inherit implemented operator-only unbind','error' in rpc('control.write','binding.unbind',{'id':owner.user['assignment'],'request_id':str(uuid.uuid4()),'expected_revision':1}))
        owner.op('person','set-admin',owner.actor,'false')
        ctx.check('revoked role rejects already open plugin invocation','error' in rpc('control.read','character.get',{}))
        owner.op('person','set-enabled',owner.actor,'false')
        ctx.check('disabled original person stops public plugin reads','error' in rpc('control.read','character.public',{}))
        ctx.equal('disabled person stops open browser reads',wire(owner.svc.address,'/client/v1/character',token)[0],401)
        owner.op('host','enable','control.fixture','false')
    return {'transport':'real framed Host command','actor':owner.actor,'checks':'current original person and capability on every call','self_reported_roles':'rejected'}


def run(ctx,wire):
    return {runtime:scenario(ctx,wire,runtime) for runtime in ['python','rust']}
