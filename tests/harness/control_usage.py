"""Usage comes from actual HTTP attempts and the original logical reservation, never estimates."""
import datetime as dt
from http.client import HTTPConnection, IncompleteRead
from contextlib import closing
import json
import os
import threading
import uuid
from urllib.parse import urlencode, urlsplit
from zoneinfo import ZoneInfo
from tests.harness.compact import Owner
from tests.peers.chat.recorder import Recorder


def run(ctx, wire):
    owner = Owner(ctx, 'usage', 20)
    token = 'fixture-control-usage-reader-token-00000000000000'
    cfg=owner.svc.data/'yuki.toml'
    base='operator_registry_file="operators.toml"\n'+cfg.read_text()
    cfg.write_text(base)
    registry=owner.svc.data/'operators.toml'
    registry.write_text('[[operators]]\nprincipal_id="'+str(uuid.uuid4())+'"\ntoken_env="CONTROL_USAGE"\ncapabilities=["control.config.file.content.read"]\n')
    env={**os.environ,'CONTROL_USAGE':token}
    end=(dt.datetime.now(dt.UTC)+dt.timedelta(hours=1)).isoformat()
    query={'range':'all','timezone':'UTC','end':end}
    def stats(**changes):
        params={**query,**changes}
        code,value=wire(owner.svc.address,'/admin/v1/models/stats?'+urlencode(params),token)
        ctx.equal('authorized original model usage query',code,200)
        return value['data']
    main_source=None
    with Recorder() as http:
        owner.config(http,'profile_task="chat_agent"\n')
        with owner.svc.running(environment=env):
            main_source=owner.human('usage-private-body-canary')
            _,answer=owner.wire(http)
            answer.put(({'choices':[{'finish_reason':'stop','message':{'role':'assistant','content':'usage-private-result-canary'}}],
                         'usage':{'prompt_tokens':100,'completion_tokens':10,'total_tokens':110,'prompt_tokens_details':{'cached_tokens':20}}},0))
            owner.until(lambda: owner.rows("SELECT id FROM model_attempts WHERE state='response'"))
            first=stats()['summary']
            ctx.write('usage-first-response.json',first)
            ctx.equal('actual Main usage comes from response fields',(first['invocations'],first['prompt_tokens'],first['cached_prompt_tokens']),(1,100,20))
        # Separate test activations use the same writer and real HTTP without the Main scheduler.
        cfg.write_text(base)
        with owner.svc.running(environment=env):
            def op(*args):return owner.op('model',*args)
            def logical(key,safe=False,model='manual-model'):
                source=owner.human('usage-'+key+'-private')
                execution=owner.op('ingress','claim',source)['execution']
                op('root',execution,3,1000)
                op('activate',execution,key+'-activation','original-usage-fixture')
                op('reserve',key+'-activation',key,10,20,*(['--safe-rate-limit-once'] if safe else []),'--request',json.dumps({'model':model,'messages':[{'role':'user','content':'usage-sensitive-material'}]}))
                return source,execution
            physical=[]
            def send(key,attempt,body,status=200,missing=0):
                permit=op('dispatch',key,attempt)
                ctx.check('physical usage attempt dispatched '+attempt,permit['dispatch_now'])
                seen={}
                def transmit():
                    peer=urlsplit(http.url)
                    with closing(HTTPConnection(peer.hostname,peer.port,timeout=5)) as c:
                        c.request('POST',peer.path,permit['request_raw'],headers={'Content-Type':'application/json'})
                        response=c.getresponse();seen['status']=response.status
                        try:seen['body']=response.read()
                        except IncompleteRead as exc:seen['body']=exc.partial;seen['incomplete']=True
                thread=threading.Thread(target=transmit);thread.start()
                raw,q=http.receive();ctx.equal('physical request uses original reserved body',raw,permit['request_raw'])
                q.put((body,missing,status));thread.join(timeout=8)
                ctx.check('physical HTTP attempt ended',not thread.is_alive())
                physical.append({'logical':key,'attempt':attempt,'status':seen['status'],'incomplete':seen.get('incomplete',False)})
                return seen
            logical('retry',True)
            send('retry','retry-1',{'rejected':'before_model'},429)
            op('rejected','retry-1',429,'--unbilled-before-model','{"rejected":"before_model"}','{}')
            send('retry','retry-2',{'answer':'done'})
            op('response','retry-2','{"answer":"done"}',json.dumps({'input':60,'output':5,'total':65,'cache':0}),'[]')
            logical('partial')
            seen=send('partial','partial-1',{'partial':'known usage'},200,5)
            ctx.check('unknown attempt has a real interrupted body',seen['incomplete'])
            op('hold','partial-1','{"partial":"usage-private-partial"}',json.dumps({'input':40,'output':3}),'incomplete_body','true',200)
            _,pending=logical('pending',model='pending-only')
            op('dispatch','pending','pending-1')
            current=stats()
            ctx.write('usage-current.json',current)
            ctx.equal('profile task comes from the frozen original contract',next(g for g in current['groups'] if g['model']=='fixture')['sources'][0]['task'],'chat_agent')
            summary=current['summary']
            ctx.equal('retry attempts remain one logical invocation',(summary['invocations'],summary['successes'],summary['failures']),(3,2,1))
            ctx.equal('physical known subtotals survive unknown total',(summary['prompt_tokens'],summary['completion_tokens'],summary['total_tokens'],summary['unknown_usage']),(200,18,175,1))
            ctx.equal('field coverage counts reported logical calls',summary['coverage'],{'prompt_tokens':3,'completion_tokens':3,'total_tokens':2,'cached_prompt_tokens':2,'cache_creation_input_tokens':0,'cache_creation_5m_input_tokens':0,'cache_creation_1h_input_tokens':0})
            ctx.equal('cache denominator excludes calls without cache field',summary['cache_read'],{'reported_calls':2,'prompt_tokens':160,'cached_prompt_tokens':20,'ratio':0.125})
            selected=stats(model='fixture')
            ctx.equal('filter narrows summary',selected['summary']['invocations'],1)
            ctx.equal('filter options retain complete time window',selected['options'],current['options'])
            ctx.equal('task grouping preserves known totals',stats(group_by='task')['summary'],summary)
            first_time=owner.rows('SELECT min(dispatched_at) FROM model_attempts')[0][0].replace(' ','T')+'Z'
            ctx.equal('window end excludes exactly equal dispatch time',stats(end=first_time)['summary']['invocations'],0)
            for date,zone in [('2026-11-05T12:00:00Z','Asia/Shanghai'),('2018-11-04T12:00:00Z','America/Sao_Paulo'),('2026-11-01T12:00:00Z','America/Los_Angeles')]:
                view=stats(range='today',timezone=zone,end=date)
                expected=dt.datetime.fromisoformat(date).astimezone(ZoneInfo(zone)).replace(hour=0,minute=0,second=0,microsecond=0,fold=0).astimezone(dt.UTC)
                ctx.equal('local midnight matches original ZoneInfo including DST '+zone,dt.datetime.fromisoformat(view['range']['start']),expected)
            for changes in [{'timezone':'invented/not-a-zone'},{'end':'2026-01-01T00:00:00'},{'range':'week'},{'group_by':'user'}]:
                ctx.equal('invalid usage query rejects',wire(owner.svc.address,'/admin/v1/models/stats?'+urlencode({**query,**changes}),token)[0],422)
            ctx.check('usage projection excludes private model material','usage-private' not in json.dumps(current) and 'usage-sensitive' not in json.dumps(current))
            owner.op('privacy','forget','usage-owner-forget',owner.actor)
            after=stats()
            ctx.equal('erasure preserves known charges and dimensions',after['summary'],summary)
            ctx.equal('erasure preserves original model options',after['options'],current['options'])
            ctx.equal('erasure revokes retry authority but retains original nonbilling fact',owner.rows("SELECT t.retry_proof,EXISTS(SELECT 1 FROM model_usage_unbilled u WHERE u.attempt=t.id) FROM model_attempts t WHERE id='retry-1'"),[(0,1)])
            owner.svc.command('operator','model','dispatch','retry','after-erasure-retry',accepted=False)
            registry.write_text(registry.read_text().replace('control.config.file.content.read','control.character.read'))
            ctx.equal('same browser loses usage immediately after revoke',wire(owner.svc.address,'/admin/v1/models/stats?'+urlencode(query),token)[0],403)
            registry.write_text(registry.read_text().replace('control.character.read','control.config.file.content.read'))
        with owner.svc.running(environment=env):
            cold=stats()
            ctx.equal('cold restart retains original charged calls',cold['summary']['prompt_tokens'],200)
            ctx.equal('unanswered dispatched attempt becomes explicit unknown',cold['summary']['unknown_usage'],2)
            ctx.equal('recovered unknown counts once',cold['summary']['invocations'],4)
            ctx.equal('CLI and Admin use the same usage service',owner.op('control','read','model.stats',json.dumps(query)),cold)
        ctx.write('usage-physical-attempts.json',physical)
        return {'before_erasure':current,'after_restart':cold,'original_main_source':main_source,'physical_attempts':len(physical)+1,'paid_calls':0}
