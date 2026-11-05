"""Trace projections retain real model/effect states, with separate content authority."""
import json
import os
import uuid
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def run(ctx, wire):
    with Recorder() as http:
        owner=Owner(ctx,'control-trace',20);owner.config(http)
        cfg=owner.svc.data/'yuki.toml';cfg.write_text('operator_registry_file="operators.toml"\n'+cfg.read_text())
        metadata='trace-metadata-fixture-0000000000000000000'
        content='trace-content-fixture-00000000000000000000'
        registry=owner.svc.data/'operators.toml'
        registry.write_text('[[operators]]\nprincipal_id="'+str(uuid.uuid4())+'"\ntoken_env="TRACE_METADATA"\ncapabilities=["control.execution.metadata.read"]\n[[operators]]\nprincipal_id="'+str(uuid.uuid4())+'"\ntoken_env="TRACE_CONTENT"\ncapabilities=["control.execution.metadata.read","control.execution.content.read"]\n')
        env={**os.environ,'TRACE_METADATA':metadata,'TRACE_CONTENT':content}
        with owner.svc.running(environment=env):
            event=owner.human('a public trace source')
            body,answer=owner.wire(http)
            execution=owner.rows('SELECT id FROM executions WHERE source=?',event)[0][0]
            reply(answer,calls=[call('trace-accept','task_control',{'action':'accept','goal':'retain trace continuation'})])
            body,answer=owner.wire(http)
            continuation=owner.human('follow-up trace input')
            owner.until(lambda:owner.rows('SELECT event FROM main_inputs WHERE execution=? AND event=?',execution,continuation))
            answer.put(({'choices':[{'message':{'role':'assistant','content':None,'tool_calls':[call('trace-get','get',{'target':json.dumps({'api_key':'PRIVATE-TRACE-ENCODED','nested':{'secret':'PRIVATE-TRACE-NESTED'}})})]},'finish_reason':'tool_calls'}],
                'provider_state':{'signature':'PRIVATE-TRACE-SIGNATURE','encrypted_content':'PRIVATE-TRACE-ENCRYPTED','thoughtSignature':'PRIVATE-TRACE-THOUGHT','api_key':'PRIVATE-TRACE-KEY','authorization':'PRIVATE-TRACE-AUTH','code':'PRIVATE-TRACE-PAIR','nested':[{'token':'PRIVATE-TRACE-TOKEN'}]}},0))
            body,answer=owner.wire(http)
            ctx.check('follow-up input remains on original trace execution','follow-up trace input' in str(body['messages']))
            reply(answer,'a visible response')
            owner.until(lambda: owner.rows("SELECT id FROM executions WHERE id=? AND state='completed'",execution))
            path=f'/admin/v1/traces/turn/{execution}'
            status,meta=wire(owner.svc.address,path,metadata)
            ctx.equal('metadata-only reader sees real trace timeline',status,200)
            ctx.check('trace contains actual model transitions',any(x['kind']=='model' for x in meta['data']['items']))
            ctx.check('metadata projection contains no payload',all('payload' not in x for x in meta['data']['items']))
            ctx.equal('metadata permission alone cannot read body',wire(owner.svc.address,path+'?content=true',metadata)[0],403)
            status,full=wire(owner.svc.address,path+'?content=true',content)
            ctx.equal('independent content permission reads body',status,200)
            ctx.write("trace-public-projection.json", full)
            raw=json.dumps(full)
            ctx.check('public response remains observable','a visible response' in raw)
            for secret in ['SIGNATURE','ENCRYPTED','THOUGHT','KEY','AUTH','PAIR','TOKEN','ENCODED','NESTED']:
                ctx.check('private trace '+secret+' absent','PRIVATE-TRACE-'+secret not in raw)
            ctx.check('opaque provider fields are explicitly omitted','opaque_provider_state' in raw)
            status,first=wire(owner.svc.address,path+'?limit=1',metadata)
            cursor=first['data']['next_cursor']
            ctx.check('trace has stable numeric cursor',isinstance(cursor,int))
            status,second=wire(owner.svc.address,path+'?after='+str(cursor),metadata)
            ctx.check('trace page does not repeat the first original entry',all(row['id']>cursor for row in second['data']['items']))
            original_ids=[v['id'] for v in meta['data']['items']]
            ctx.equal('event and original turn select same timeline',[v['id'] for v in wire(owner.svc.address,f'/admin/v1/traces/event/{event}',metadata)[1]['data']['items']],original_ids)
            ctx.equal('continuation event locates its actual original execution',[v['id'] for v in wire(owner.svc.address,f'/admin/v1/traces/event/{continuation}',metadata)[1]['data']['items']],original_ids)
        with owner.svc.running(environment=env):
            ctx.equal('cold restart preserves diagnostic cursor',[v['id'] for v in wire(owner.svc.address,path,metadata)[1]['data']['items']],original_ids)
            saved=registry.read_text();registry.write_text(saved.replace('token_env="TRACE_CONTENT"','token_env="TRACE_CONTENT"\nenabled=false'))
            ctx.equal('revoked observer cannot keep reading trace',wire(owner.svc.address,path+'?content=true',content)[0],409)
            registry.write_text(saved)
            owner.op('privacy','forget','trace-forget',owner.actor)
            ctx.equal('original source erasure removes trace body and rows',wire(owner.svc.address,path+'?content=true',content)[1]['data']['items'],[])
    return {'execution':execution,'source_event':event,'original_trace_ids':original_ids,'opaque_material':'omitted','erasure':'verified'}
