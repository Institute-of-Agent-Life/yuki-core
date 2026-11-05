"""Explicit Admin probes use actual HTTP once, with current authority and no Work."""
import hashlib
import importlib.util
import json
import queue
from concurrent.futures import ThreadPoolExecutor
from tests.harness.compact import reply
from tests.peers.chat.recorder import Recorder


def run(ctx, wire):
    spec=importlib.util.spec_from_file_location('profile_case',ctx.root/'tests/cases/CORE-048.py')
    profiles=importlib.util.module_from_spec(spec);spec.loader.exec_module(profiles)
    with profiles.Recorder() as peer, ThreadPoolExecutor(max_workers=2) as pool:
        owner,env=profiles.setup(ctx,'probe',peer)
        token=env['PROFILE_OPERATOR_WRITER_A'];denied=env['PROFILE_OPERATOR_UNPRIVILEGED']
        def api(method,body,credential=token):
            return wire(owner.svc.address,'/admin/v1/models/'+method,credential,body)
        def draft(endpoint=peer.url,**extra):
            return {'id':'draft','provider':'fixture','protocol':'chat_completions',
                    'base_url':endpoint.removesuffix('/chat/completions'),'model':'draft-model',
                    'timeout_seconds':2,'max_retries':9,'default_temperature':0.7,
                    'default_max_output_tokens':100,'thinking_enabled':True,**extra}
        with owner.svc.running(environment=env):
            before=profiles.file_hashes(owner)
            ctx.equal('unprivileged probe does not dispatch',api('test',{'profile_id':'primary'},denied)[0],403)
            ctx.check('denied probe emitted no HTTP',peer.wires.empty())
            ctx.equal('probe rejects arbitrary request headers',api('test',{'draft_profile':draft(headers={'Authorization':'forged'})})[0],422)
            ctx.equal('missing saved profile is not invented',api('test',{'profile_id':'missing'})[0],404)
            future=pool.submit(api,'test',{'profile_id':'primary'})
            try:raw,q=peer.receive();body=json.loads(raw)
            except queue.Empty:
                ctx.write('model-probe-prepare-failure.json',{'http':future.result(timeout=1),'cli':owner.svc.command('operator','control','write','model.test',json.dumps({'profile_id':'primary'}),accepted=False)})
                raise
            ctx.equal('saved probe uses actual original model',body['model'],'before')
            ctx.equal('probe has one fixed input and no private context',body['messages'],[{'role':'user','content':'Reply OK.'}])
            ctx.check('probe bounds output and omits tools',body['max_tokens']<=16 and not body.get('tools'))
            ctx.equal('saved probe resolves original secret',peer.authorization_hashes[-1],profiles.credential_hash('fixture-key-before'))
            reply(q,'OK');code,result=future.result(timeout=8)
            ctx.equal('probe succeeds on actual response',code,200)
            ctx.check('probe reports observed latency',result['data']['ok'] and result['data']['latency_seconds']>=0)
            ctx.check('public result contains no upstream reply','Reply OK.' not in json.dumps(result) and 'fixture-key' not in json.dumps(result))
            for status,category,error in [(401,'authentication',{}),(429,'rate_limit',{}),(429,'quota',{'code':'insufficient_quota'}),(503,'unavailable',{})]:
                future=pool.submit(api,'test',{'draft_profile':draft(),'api_key':'draft-secret-canary'})
                raw,q=peer.receive();sent=json.loads(raw)
                ctx.equal('draft stays on its explicit model',sent['model'],'draft-model')
                q.put(({'error':{'message':'secret-error-canary',**error}},0,status))
                code,result=future.result(timeout=8)
                ctx.equal('provider fault remains categorized '+str(status),result['data']['error_category'],category)
                ctx.check('probe never retries despite configured max_retries',peer.wires.empty())
                ctx.check('provider diagnostics are not public','secret-error-canary' not in json.dumps(result))
            future=pool.submit(api,'test',{'draft_profile':draft(timeout_seconds=0.15)})
            _,q=peer.receive();code,result=future.result(timeout=8)
            ctx.equal('timeout is one-shot',result['data']['error_category'],'timeout')
            reply(q,'late timeout reply')
            future=pool.submit(api,'test',{'draft_profile':draft()})
            _,q=peer.receive();q.put((b'{"choices":',32))
            code,result=future.result(timeout=8)
            ctx.equal('partial body is unavailable without retry',result['data']['error_category'],'unavailable')
            ctx.check('partial response did not create a retry',peer.wires.empty())

            # All adapters share the same ephemeral compiled connection, while
            # retaining their original auth, body and completion contracts.
            variants=[
                ('responses','/v1/responses',{'id':'r','object':'response','status':'completed','output':[{'type':'message','role':'assistant','content':[{'type':'output_text','text':'OK'}]}]}),
                ('anthropic_messages','/v1/messages',{'id':'m','type':'message','role':'assistant','content':[{'type':'text','text':'OK'}],'stop_reason':'end_turn'}),
                ('gemini','/v1/models/draft-model:generateContent',{'candidates':[{'content':{'role':'model','parts':[{'text':'OK'}]},'finishReason':'STOP'}]}),
            ]
            for protocol,path,response in variants:
                with Recorder(path) as alternate:
                    profile=draft(base_url=alternate.url.split('/v1/')[0]+'/v1',protocol=protocol)
                    future=pool.submit(api,'test',{'draft_profile':profile,'api_key':'alternate-secret'})
                    raw,q=alternate.receive();sent=json.loads(raw)
                    if protocol=='anthropic_messages':
                        ctx.check('Messages probe uses original auth',alternate.header_shapes[-1]['api_key'])
                        ctx.check('Messages probe disables thinking','thinking' not in sent)
                    if protocol=='gemini':
                        ctx.check('Gemini probe uses original auth',alternate.header_shapes[-1]['google_key'])
                    q.put((response,0));code,result=future.result(timeout=8)
                    ctx.equal(protocol+' probe HTTP status',code,200)
                    ctx.check(protocol+' native response accepted',result['data']['ok'])

            queries=[]
            def get(handler):
                answer=queue.Queue();queries.append((handler.path,hashlib.sha256(handler.headers.get('Authorization','').encode()).hexdigest()))
                peer.wires.put((handler.path,b'',answer));value,status=answer.get(timeout=8)
                raw=json.dumps(value).encode();handler.send_response(status)
                if status==302:handler.send_header('Location',peer.url)
                handler.send_header('Content-Length',str(len(raw)));handler.end_headers();handler.wfile.write(raw)
            peer.server.RequestHandlerClass.do_GET=get
            future=pool.submit(api,'discover',{'profile_id':'primary'})
            path,_,q=peer.wires.get(timeout=8)
            ctx.equal('discovery uses original base path',path,'/v1/models')
            q.put(({'data':[{'id':'first','display_name':'First','capabilities':['invented']}],'has_more':True,'next':'untrusted'},200))
            code,result=future.result(timeout=8)
            ctx.equal('discovery returns only original model labels',result['data']['models'],[{'id':'first','display_name':'First'}])
            ctx.equal('discovery does not follow untrusted pagination',len(queries),1)
            ctx.equal('discovery uses the saved secret',queries[0][1],profiles.credential_hash('fixture-key-before'))
            future=pool.submit(api,'discover',{'draft_connection':{'provider':'fixture','protocol':'chat_completions','base_url':peer.url.removesuffix('/chat/completions'),'timeout_seconds':2}})
            _,_,q=peer.wires.get(timeout=8);q.put(({'error':'private-discovery-canary'},404))
            code,result=future.result(timeout=8)
            ctx.equal('unsupported discovery is explicit',result['data'],{'models':[],'error_category':'unsupported'})

            future=pool.submit(api,'discover',{'profile_id':'primary'})
            _,_,q=peer.wires.get(timeout=8);q.put(({},302))
            code,result=future.result(timeout=8)
            ctx.equal('discovery redirect is not followed',result['data']['error_category'],'protocol')
            ctx.check('redirect does not forward saved credentials',peer.wires.empty())
            future=pool.submit(api,'discover',{'draft_connection':{'provider':'gemini','protocol':'gemini','base_url':peer.url.removesuffix('/chat/completions'),'timeout_seconds':2}})
            _,_,q=peer.wires.get(timeout=8);q.put(({'models':[{'name':'models/observed','displayName':'Observed','supportedGenerationMethods':['not-inferred']}]},200))
            code,result=future.result(timeout=8)
            ctx.equal('Gemini discovery retains labels without inferred capabilities',result['data']['models'],[{'id':'models/observed','display_name':'Observed'}])
            future=pool.submit(owner.op,'control','write','model.test',json.dumps({'profile_id':'primary'}))
            _,q=peer.receive();reply(q,'OK')
            ctx.check('OS CLI uses same probe service',future.result(timeout=8)['ok'])
            future=pool.submit(api,'test',{'profile_id':'primary'})
            _,q=peer.receive()
            registry=owner.svc.data/'operators.toml';old=registry.read_text();registry.write_text(old.replace('"control.config.file.mutate"','"control.health.read"'))
            reply(q,'private late probe response')
            ctx.equal('revocation during physical probe blocks publication',future.result(timeout=8)[0],403)
            ctx.equal('revoked operator cannot issue next probe',api('test',{'profile_id':'primary'})[0],403)
            ctx.check('revocation never dispatched a replacement',peer.wires.empty())
            registry.write_text(old)
            ctx.equal('all probes preserve files byte-for-byte',profiles.file_hashes(owner),before)
            for table in ('work_nodes','executions','model_requests','model_attempts','root_budgets'):
                ctx.equal('explicit probe creates no '+table,owner.rows('SELECT count(*) FROM '+table),[(0,)])
            registry.write_text(old.replace('"control.config.file.mutate"','"control.config.file.mutate", "control.operation.read", "control.character.read", "control.character.mutate"'))
            update=profiles.body(owner.op('profiles','read'),'receipt-model')
            code,saved=api('apply',update)
            ctx.equal('model save succeeds through original Admin route',code,200)
            code,receipt=wire(owner.svc.address,'/admin/v1/operations/'+update['request_id'],token)
            ctx.equal('shared control reader locates model operation',code,200)
            metadata=receipt['data']
            ctx.equal('shared control receipt carries actual saved revision',metadata['result_revision'],saved['data']['revision'])
            ctx.equal('shared control receipt records completion',metadata['status'],'succeeded')
            ctx.check('new model receipt has observed timestamps',metadata['created_at'] is not None and metadata['updated_at'] is not None)
            ctx.check('model receipt never reveals staged file or credential','fixture-key' not in json.dumps(receipt) and 'receipt-model' not in json.dumps(receipt))
            code,qualified=wire(owner.svc.address,'/admin/v1/operations/'+metadata['operation_id'],token)
            ctx.equal('qualified model operation retains original identity',qualified,receipt)
            code,character=wire(owner.svc.address,'/admin/v1/character',token)
            conflict={'request_id':update['request_id'],'expected_revision':character['data']['revision'],'display_name':'must not overwrite model receipt'}
            ctx.equal('same request cannot overwrite a different control operation',wire(owner.svc.address,'/admin/v1/character',token,conflict,'PATCH')[0],409)
            result={'physical_posts':len(peer.header_shapes),'physical_discovery_requests':len(queries),'protocols':['chat_completions',*[v[0] for v in variants]],'file_sha256':before,'no_retries':True,'no_work':True}
    ctx.write('model-probes.json',result)
    return result
