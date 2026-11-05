"""Replay retained v4 usage facts, then separate real attempts, windows and live grants."""
import datetime as dt
import hashlib
import json
import os
import sqlite3
import subprocess
import sys
import uuid
from contextlib import closing
from http.client import HTTPConnection
from threading import Thread
from urllib.error import HTTPError
from urllib.parse import urlencode, urlsplit
from urllib.request import Request, urlopen
from tests.harness.timezones import ZoneInfo
from tests.harness.compact import Owner
from tests.peers.chat.recorder import Recorder


def wire(address,path,token,body=None):
    req=Request('http://'+address+path,headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'},data=None if body is None else json.dumps(body).encode())
    try:
        with urlopen(req,timeout=10) as r:return r.status,json.load(r)
    except HTTPError as e:
        with e:return e.code,json.load(e)


def run(ctx):
    built=subprocess.run([sys.executable,str(ctx.root/'ci/publish-contract.py'),'build','--out',str(ctx.output/'sdk-distributions'),'--verify-work',str(ctx.working/'sdk-install')],capture_output=True,text=True)
    if built.returncode:ctx.write('sdk-build-failure.json',{'stdout':built.stdout,'stderr':built.stderr})
    ctx.equal('candidate SDK distributions verify',built.returncode,0)
    original=(ctx.root/'tests/fixtures/usage/known-coverage-cache.json').read_bytes()
    ctx.equal('fixed upstream usage artifact hash',hashlib.sha256(original).hexdigest(),'a003f6406ae4162a7d5c407436b151855f6784e3743ee749eb87689b9179496d')
    golden=json.loads(original)
    owner=Owner(ctx,'usage-coverage',41)
    token='fixture-usage-055-0000000000000000000000'
    cfg=owner.svc.data/'yuki.toml';cfg.write_text('operator_registry_file="operators.toml"\n'+cfg.read_text())
    registry=owner.svc.data/'operators.toml';registry.write_text('[[operators]]\nprincipal_id='+json.dumps(str(uuid.uuid4()))+'\ntoken_env="USAGE_READER"\ncapabilities=["control.config.file.content.read"]\n')
    env={**os.environ,'USAGE_READER':token}
    query={'range':'today','timezone':'Asia/Taipei','end':'2026-11-01T04:00:00Z'}
    def read(**changes):
        code,body=wire(owner.svc.address,'/admin/v1/models/stats?'+urlencode({**query,**changes}),token)
        ctx.equal('actual Admin usage query succeeds',code,200)
        return body['data']
    def model(*args):return owner.op('model',*args)
    roots=[]
    def reserve(key,safe=False):
        event=owner.human('private-usage-source-'+key);root=owner.op('ingress','claim',event)['execution'];roots.append(root)
        model('root',root,3,10000);model('activate',root,key+'-activation','retained-profile-'+key)
        model('reserve',key+'-activation',key,100,1000,*(['--safe-rate-limit-once'] if safe else []),'--request',json.dumps({'model':'original-model','private':'usage-body-canary'}))
        return root
    fields=[
        {'input':100,'output':20,'total':120,'cache':40,'cache_creation':10,'cache_creation_5m':3,'cache_creation_1h':7},
        {'input':900,'output':20,'total':920},
        {'output':7,'cache':30},
        {'input':0,'output':20,'total':0,'cache':0},
        {'input':0,'cache':5,'cache_creation':12},
        {'input':10,'output':20,'total':4,'cache':2},
    ]
    with owner.svc.running(environment=env):
        for n,usage in enumerate(fields):
            key='retained-'+str(n);reserve(key);model('dispatch',key,key+'-attempt')
            if n==2:model('rejected',key+'-attempt',500,'{"failed":true}',json.dumps(usage))
            else:model('response',key+'-attempt','{"answer":"retained"}',json.dumps(usage),'[]')
    # Reconstruct the old aggregate fixture as isolated retained attempt data.
    # The extra unknown physical receipt is historical fixture data, never a retry permit.
    with closing(sqlite3.connect(owner.svc.data/'core.sqlite')) as db:
        db.execute("UPDATE model_attempts SET dispatched_at='2026-11-01T03:00:00Z',received_at='2026-11-01T03:00:02Z'")
        db.execute("UPDATE model_usage_dimensions SET task='chat_agent',provider='original-provider',model='original-model',profile_id='flash'")
        db.execute("UPDATE model_usage_dimensions SET task='memory_dream' WHERE logical IN ('retained-4','retained-5')")
        db.execute("UPDATE model_usage_dimensions SET profile_id='cache-only' WHERE logical='retained-4'")
        db.execute("UPDATE model_attempts SET ordinal=1 WHERE id='retained-5-attempt'")
        db.execute("INSERT INTO model_attempts(id,logical,ordinal,state,dispatched_at,ever_unknown) VALUES('retained-unknown','retained-5',0,'unknown','2026-11-01T03:00:00Z',1)")
        db.commit()
    with Recorder() as http:
        owner.config(http,'profile_task="chat_agent"\n');cfg.write_text(cfg.read_text().replace('model="fixture"','model="new-route-model"'))
        with owner.svc.running(environment=env):
            before_ledger=owner.rows('SELECT id,logical,state,usage_json,dispatched_at,received_at FROM model_attempts ORDER BY id')
            view=read();ctx.write('retained-source-report.json',view)
            ctx.equal('six calls preserve every original known field and denominator',view['summary'],golden['summary'])
            ctx.equal('original task provider model profile survive route change',view['groups'],golden['groups'])
            ctx.equal('cache ratio uses exactly four co-reported calls',view['summary']['cache_read'],{'reported_calls':4,'prompt_tokens':110,'cached_prompt_tokens':47,'ratio':47/110})
            ctx.check('current route is not queried for old options','new-route-model' not in json.dumps(view))
            ctx.equal('zero cache/input is reported rather than absent',read(profile_id='cache-only')['summary']['cache_read'],{'reported_calls':1,'prompt_tokens':0,'cached_prompt_tokens':5,'ratio':None})
            ctx.equal('filter options cover the complete original window',read(profile_id='cache-only')['options'],view['options'])
            empty=read(profile_id='absent');ctx.equal('absent samples have no ratio',empty['summary']['cache_read']['ratio'],None)
            ctx.equal('absent sample coverage stays zero',empty['summary']['coverage']['prompt_tokens'],0)
            ctx.equal('task grouping retains field coverage',read(group_by='task')['summary'],view['summary'])
            all_view=read(range='all');ctx.equal('all means retained records with no lower bound',all_view['range']['start'],None)
            for root in roots:
                ctx.equal('no price never becomes zero dollars',model('usage',root)['cost_usd'],None)
            for preset,zone,end in [('today','America/New_York','2026-03-09T03:30:00Z'),('today','America/New_York','2026-11-02T04:30:00Z'),('24h','America/New_York','2026-03-09T03:30:00Z'),('7d','America/New_York','2026-11-02T04:30:00Z')]:
                t=dt.datetime.fromisoformat(end);expected=t.astimezone(ZoneInfo(zone)).replace(hour=0,minute=0,second=0,microsecond=0,fold=0).astimezone(dt.UTC) if preset=='today' else t-dt.timedelta(days=1 if preset=='24h' else 7)
                result=read(range=preset,timezone=zone,end=end)
                ctx.equal('calendar/rolling boundary '+preset+end,dt.datetime.fromisoformat(result['range']['start']),expected)
            ctx.equal('usage projection never rewrites original attempt evidence',owner.rows('SELECT id,logical,state,usage_json,dispatched_at,received_at FROM model_attempts ORDER BY id'),before_ledger)
            route='/admin/v1/models/stats?'+urlencode(query)
            def pair(person):
                code=owner.op('personal','issue',person,str(uuid.uuid4()),'chat')['code']
                status,body=wire(owner.svc.address,'/client/v1/pair','',{'code':code,'device_label':'usage fixture'})
                ctx.equal('usage device pairs',status,200);return body['data']
            ordinary=pair(owner.actor)
            ctx.equal('ordinary current device cannot read usage',wire(owner.svc.address,route,ordinary['token'])[0],403)
            owner.op('person','set-admin',owner.actor,'true')
            ctx.equal('current superuser device can read usage',wire(owner.svc.address,route,ordinary['token'])[0],200)
            owner.op('person','set-admin',owner.actor,'false')
            ctx.equal('same URL refuses revoked superuser immediately',wire(owner.svc.address,route,ordinary['token'])[0],403)
            owner.op('person','set-admin',owner.actor,'true')
            owner.op('personal','revoke',ordinary['device_id'])
            ctx.equal('same URL refuses revoked device immediately',wire(owner.svc.address,route,ordinary['token'])[0],401)
        # Disable Main before the explicit real-attempt scenario.
        cfg.write_text(cfg.read_text().split('\n[main]')[0])
        with owner.svc.running(environment=env):
            reserve('split-fields',True)
            def send(attempt,usage,status):
                permit=model('dispatch','split-fields',attempt);seen={}
                def transmit():
                    peer=urlsplit(http.url)
                    with closing(HTTPConnection(peer.hostname,peer.port,timeout=5)) as c:
                        c.request('POST',peer.path,permit['request_raw'],headers={'Content-Type':'application/json'});r=c.getresponse();seen['status']=r.status;seen['body']=r.read()
                thread=Thread(target=transmit);thread.start();raw,q=http.receive();q.put(({'usage':usage},0,status));thread.join(8)
                ctx.check('physical usage response arrived',not thread.is_alive() and seen['status']==status)
                ctx.equal('attempt uses original request bytes',raw,permit['request_raw'])
                if status==429:model('rejected',attempt,status,'--unbilled-before-model','{"rejected":"before_model"}',json.dumps(usage))
                else:model('response',attempt,'{"answer":"done"}',json.dumps(usage),'[]')
            send('split-input',{'input':900},429);send('split-cache',{'cache':30,'output':0},200)
            now=(dt.datetime.now(dt.UTC)+dt.timedelta(minutes=1)).isoformat()
            split=read(range='24h',timezone='UTC',end=now)['summary']
            ctx.equal('separate attempts cannot manufacture a cache pair',split['cache_read'],{'reported_calls':0,'prompt_tokens':0,'cached_prompt_tokens':0,'ratio':None})
            ctx.equal('independent reported fields still sum across attempts',(split['prompt_tokens'],split['cached_prompt_tokens'],split['invocations']),(900,30,1))
            unknown_before=owner.rows("SELECT count(*) FROM model_attempts WHERE logical='retained-5'")[0][0]
            owner.svc.command('operator','model','dispatch','retained-5','forbidden-extra-attempt',accepted=False)
            ctx.equal('usage read grants no new unknown retry',owner.rows("SELECT count(*) FROM model_attempts WHERE logical='retained-5'")[0][0],unknown_before)
        # Place attempts on either side of midnight without altering their usage.
        with closing(sqlite3.connect(owner.svc.data/'core.sqlite')) as db:
            db.execute("UPDATE model_attempts SET dispatched_at='2026-11-02T04:59:59Z',received_at='2026-11-02T04:59:59Z' WHERE id='split-input'")
            db.execute("UPDATE model_attempts SET dispatched_at='2026-11-02T05:00:00Z',received_at='2026-11-02T05:00:01Z' WHERE id='split-cache'");db.commit()
        with owner.svc.running(environment=env):
            boundary=read(range='today',timezone='America/New_York',end='2026-11-02T05:01:00Z')['summary']
            ctx.equal('window includes attempt at start but excludes earlier attempt',(boundary['invocations'],boundary['prompt_tokens'],boundary['cached_prompt_tokens']),(1,0,30))
            ctx.equal('end is exclusive at exact dispatch instant',read(range='today',timezone='America/New_York',end='2026-11-02T05:00:00Z')['summary']['invocations'],0)
            ctx.equal('cold retained fixture remains unchanged',read()['summary'],golden['summary'])
            ctx.check('usage never exposes model/source body','usage-body-canary' not in json.dumps(read()))
    binary=ctx.root/'target/debug'/('yuki-core.exe' if os.name=='nt' else 'yuki-core')
    return {'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'host_idl_sha256':hashlib.sha256((ctx.root/'sdk/rust/src/host-preview.json').read_bytes()).hexdigest(),'usage_schema_sha256':hashlib.sha256((ctx.root/'protocol/control/usage.json').read_bytes()).hexdigest(),'database_schema_revision':json.loads((ctx.output/'sqlite-build.json').read_text())['schema_version'],'source_fixture_sha256':hashlib.sha256(original).hexdigest(),'source_ci_commit':'9138a528872bb9775b9ac05ddb38b5c1de6d7df2','baseline_commit':'329ca7c487414d2f54ddd98ff4727446ed2446cd','fixture_kind':'retained aggregate reconstructed as isolated attempt facts; separate real loopback retry','retained_report':view,'split_attempt_report':split,'boundary_report':boundary,'authority_statuses':[200,403,403,401],'paid_calls':0,'retained_data_only':True,'cost_estimates_created':False}
