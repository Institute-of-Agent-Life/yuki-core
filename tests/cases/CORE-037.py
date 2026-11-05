"""Normal authority and paid-chain sequence, then actual interrupted file commits."""
import hashlib
import json
import os
import uuid
from urllib.error import HTTPError
from urllib.request import Request, urlopen
from tests.harness.compact import Owner, call, reply
from tests.harness.crash import kill_writer
from tests.peers.chat.recorder import Recorder


def identity():
    return str(uuid.uuid4())


def wire(address, path, token, body=None, method=None):
    req = Request('http://' + address + path, method=method,
                  headers={'Content-Type': 'application/json', 'Authorization': 'Bearer ' + token},
                  data=None if body is None else json.dumps(body).encode())
    try:
        with urlopen(req, timeout=12) as response:
            return response.status, json.load(response)
    except HTTPError as error:
        with error:
            return error.code, json.load(error)


def run(ctx):
    from tests.harness.control_sdk import prepare
    results = {"sdk": prepare(ctx)}
    with Recorder() as http:
        owner = Owner(ctx, 'control-character', 20)
        owner.config(http)
        cfg = owner.svc.data / 'yuki.toml'
        cfg.write_text('operator_registry_file="operators.toml"\nbot_aliases=[" Yuki ","雪","","雪"]\n' + cfg.read_text())
        writer = 'fixture-control-writer-token-0000000000000000'
        reader = 'fixture-control-reader-token-0000000000000000'
        registry = owner.svc.data / 'operators.toml'
        registry.write_text('[[operators]]\nprincipal_id="' + identity() + '"\ntoken_env="CONTROL_WRITER"\n'
                            'capabilities=["control.character.read","control.character.mutate","control.operation.read","control.health.read"]\n'
                            '[[operators]]\nprincipal_id="' + identity() + '"\ntoken_env="CONTROL_READER"\n'
                            'capabilities=["control.character.read"]\n')
        env = {**os.environ, 'CONTROL_WRITER': writer, 'CONTROL_READER': reader}
        with owner.svc.running(environment=env):
            code, document = wire(owner.svc.address, '/admin/v1/character', reader)
            ctx.equal('restricted operator reads public character', code, 200)
            initial = document['data']
            initial_identity=owner.op('control','read','character.identity')
            ctx.equal('public identity uses canonical name and deduplicated deployment aliases',initial_identity['aliases'],['Yuki','雪'])
            ctx.equal('local CLI and Admin share the original role',
                      owner.op('control', 'read', 'character.get'), initial)
            ctx.equal('character read grant does not grant health access', wire(owner.svc.address, '/admin/v1/health', reader)[0], 403)
            code, health = wire(owner.svc.address, '/admin/v1/health', writer)
            ctx.equal('operator health endpoint is available', code, 200)
            modules = {v['name']:v for v in health['data']}
            ctx.check('health observes actual Main scheduler', modules['work']['running'] and modules['automation']['running'])
            original = owner.human('keep the original identity and SOUL after operator edit')
            before, answer = owner.wire(http)
            change = {'request_id': identity(), 'expected_revision': initial['revision'], 'display_name': '新名稱'}
            ctx.equal('read-only operator cannot become a writer',
                      wire(owner.svc.address, '/admin/v1/character', reader, change, 'PATCH')[0], 403)
            code, changed = wire(owner.svc.address, '/admin/v1/character', writer, change, 'PATCH')
            ctx.equal('authorized character write', code, 200)
            ctx.equal('presentation edit keeps permanent character ID', changed['data']['id'], initial['id'])
            current_identity=owner.op('control','read','character.identity')
            ctx.equal('role rename immediately changes current identity aliases',current_identity['aliases'],['新名稱','Yuki','雪'])
            ctx.equal('identity aliases use the permanent role ID',current_identity['id'],initial['id'])
            ctx.equal('same role request replays original result',
                      wire(owner.svc.address, '/admin/v1/character', writer, change, 'PATCH')[1], changed)
            ctx.equal('same revision cannot overwrite newer role',
                      wire(owner.svc.address, '/admin/v1/character', writer, {**change, 'request_id': identity()}, 'PATCH')[0], 409)
            soul = owner.op('control', 'read', 'character.soul.read')
            save = {'request_id': identity(), 'expected_revision': soul['revision'], 'content': 'NEW SOUL {{name}}'}
            code, saved = wire(owner.svc.address, '/admin/v1/character/soul', writer, save, 'PUT')
            ctx.equal('SOUL write and reload share one original receipt', code, 200)
            ctx.equal('SOUL activation equals committed bytes', saved['data']['active_revision'], 'sha256:' + hashlib.sha256(save['content'].encode()).hexdigest())
            reply(answer, calls=[call('original-time', 'time_get_current', {})])
            continuation, answer = owner.wire(http)
            ctx.equal('paid continuation retains original SOUL and role', continuation['messages'][0], before['messages'][0])
            reply(answer, 'done')
            owner.finished(original)
            newer = owner.human('new request must use newly saved role')
            after, answer = owner.wire(http)
            ctx.check('next logical activation sees new role and SOUL', 'NEW SOUL 新名稱' in after['messages'][0]['content'])
            reply(answer, 'done')
            owner.finished(newer)
            code, receipt = wire(owner.svc.address, '/admin/v1/operations/' + save['request_id'], writer)
            ctx.equal('original operation is readable by its issuer', code, 200)
            ctx.check('operation reader omits staged body', 'NEW SOUL' not in json.dumps(receipt))
            original_registry = registry.read_text()
            registry.write_text(original_registry.replace('"control.character.read"', '"control.preference.read"'))
            ctx.equal('already open management screen stops after revocation',
                      wire(owner.svc.address, '/admin/v1/character', reader)[0], 403)
            registry.write_text(original_registry)
            results['character'] = {'before': initial, 'after': changed['data'], 'soul_receipt': saved['data']}
        with owner.svc.running(environment=env):
            ctx.equal('role identity survives restart', owner.op('control', 'read', 'character.get')['id'], initial['id'])
            ctx.equal('cold identity retains deployment aliases and canonical new name',owner.op('control','read','character.identity'),current_identity)
            ctx.equal('saved SOUL is active after cold restart', owner.op('control', 'read', 'character.soul.read')['active_revision'], saved['data']['active_revision'])
    for phase in ('soul-prepared-before-rename', 'soul-renamed-before-activation'):
        owner = Owner(ctx, phase, 20)
        with owner.svc.running():
            original = owner.op('control', 'read', 'character.soul.read')
        body = {'request_id': identity(), 'expected_revision': original['revision'], 'content': 'recovered ' + phase}
        kill_writer(ctx, owner.svc.data, 'soul_crash_child', phase, {'YUKI_SOUL_TEST_BODY': json.dumps(body)})
        stamp = (owner.svc.data / 'SOUL.md').stat().st_mtime_ns
        with owner.svc.running():
            pending = owner.op('control', 'read', 'character.soul.read')
            ctx.equal('interrupted save retains previously active SOUL', pending['active_revision'], original['active_revision'])
            applied = owner.op('control', 'write', 'character.soul.save', json.dumps(body))
            ctx.equal('same operation recovers and remains immutable', owner.op('control', 'write', 'character.soul.save', json.dumps(body)), applied)
            if phase == 'soul-renamed-before-activation':
                ctx.equal('known file rename is never repeated', (owner.svc.data / 'SOUL.md').stat().st_mtime_ns, stamp)
            ctx.equal('recovered activation matches original intended bytes', applied['revision'], 'sha256:' + hashlib.sha256(body['content'].encode()).hexdigest())
            results[phase] = applied
    from tests.harness.control_config import run as configuration
    results['plugin_configuration'] = configuration(ctx, wire)
    from tests.harness.control_discovery import run as discovery
    results['plugin_discovery'] = discovery(ctx, wire)
    from tests.harness.control_authority import run as authority
    results['host_authority'] = authority(ctx, wire)
    from tests.harness.control_trace import run as trace
    results['trace'] = trace(ctx, wire)
    from tests.harness.control_identity import run as identities
    results['identity'] = identities(ctx, wire)
    from tests.harness.control_usage import run as usage
    results['usage'] = usage(ctx, wire)
    from tests.harness.control_presence import run as presence
    results['presence'] = presence(ctx, wire)
    from tests.harness.control_work import run as work
    results['work'] = work(ctx, wire)
    from tests.harness.control_model_probes import run as probes
    results['model_probes'] = probes(ctx, wire)
    from tests.harness.control_outbox import run as outbox
    results['outbox'] = outbox(ctx, wire)
    from tests.harness.control_failure_receipts import run as failures
    results['deterministic_failures'] = failures(ctx, wire)
    from tests.harness.permission_directory import run as permissions
    ctx.write('original-permission-directory.json',permissions(ctx))
    binary=ctx.root/'target/debug'/('yuki-core.exe' if os.name=='nt' else 'yuki-core')
    return {'phase':'implemented-unpublished-candidate','results':results,'paid_calls':0,
            'real_extension_consumption':False,'database_schema_revision':results['outbox']['database_schema_version'],
            'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
            'host_idl_sha256':hashlib.sha256((ctx.root/'sdk/rust/src/host-preview.json').read_bytes()).hexdigest(),
            'control_schema_sha256':hashlib.sha256((ctx.root/'protocol/plugin/control/methods.json').read_bytes()).hexdigest(),
            'admin_openapi_sha256':hashlib.sha256((ctx.root/'protocol/baseline-v4/admin-openapi.json').read_bytes()).hexdigest(),
            'validated_control_methods':['control.read','control.write','plugin.config.get','plugin.secrets.get','plugin.observe'],
            'validated_action_families':['character','soul','plugin-configuration','plugin-lifecycle','plugin-discovery','outbox','person','binding','presence','work','model','usage','health','trace','operation'],
            'actual_sdk_consumers':['python-wheel','rust-crate']}
