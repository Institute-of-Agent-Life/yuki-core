"""Per-call errors never dispatch, while independent calls retain their original order."""
import json
import hashlib
from tests.harness.compact import Owner
from tests.peers.chat.recorder import Recorder

PROTOCOLS=['chat','openai_responses','deepseek_responses','deepseek_responses_legacy','anthropic_messages','gemini']
def answer(protocol,calls=()):
    if protocol=='chat':
        return {'choices':[{'finish_reason':'tool_calls' if calls else 'stop','message':{'role':'assistant','content':None if calls else 'done','tool_calls':[{'id':i,'type':'function','function':{'name':n,'arguments':raw}} for i,n,raw in calls]}}]}
    if 'responses' in protocol:
        return {'id':'response-original','status':'completed','output':[{'id':'item-'+i,'type':'function_call','call_id':i,'name':n,'arguments':raw} for i,n,raw in calls] if calls else [{'id':'end','type':'message','role':'assistant','content':[{'type':'output_text','text':'done'}]}]}
    def args(raw):
        try:return json.loads(raw)
        except ValueError:return raw
    if protocol=='anthropic_messages':
        return {'id':'message-original','type':'message','role':'assistant','stop_reason':'tool_use' if calls else 'end_turn','content':[{'type':'tool_use','id':i,'name':n,'input':args(raw)} for i,n,raw in calls] if calls else [{'type':'text','text':'done'}]}
    return {'candidates':[{'finishReason':'STOP','content':{'role':'model','parts':[{'functionCall':{'id':i,'name':n,'args':args(raw)},'thoughtSignature':'original-signature'} for i,n,raw in calls] if calls else [{'text':'done'}]}}]}

def receipts(protocol,wire):
    if protocol=='chat':return [(m['tool_call_id'],json.loads(m['content'])) for m in wire['messages'] if m['role']=='tool']
    if 'responses' in protocol:return [(m['call_id'],json.loads(m['output'])) for m in wire['input'] if m.get('type')=='function_call_output']
    if protocol=='anthropic_messages':return [(b['tool_use_id'],json.loads(b['content'])) for m in wire['messages'] for b in m['content'] if b['type']=='tool_result']
    return [(p['functionResponse']['id'],json.loads(next(iter(p['functionResponse']['response'].values())))) for m in wire['contents'] for p in m['parts'] if 'functionResponse' in p]

def run(ctx):
    oracle_path=ctx.root/'fixtures/tool-results/v4-call-rejections.json'
    oracle=json.loads(oracle_path.read_bytes())
    ctx.equal('rejection oracle binds the original fixed source',oracle['source_commit'],'329ca7c487414d2f54ddd98ff4727446ed2446cd')
    ctx.equal('original continuation cases passed',oracle['pytest_exit'],0)
    all_results=[]
    for protocol in PROTOCOLS:
        o=Owner(ctx,'call-isolation-'+protocol,43)
        path='/v1/messages' if protocol=='anthropic_messages' else '/v1beta/models/fixture:generateContent' if protocol=='gemini' else '/v1/responses' if 'responses' in protocol else '/v1/chat/completions'
        with Recorder(path=path) as model:
            o.config(model,'protocol="'+protocol+'"\nroot_tool_calls=4\n')
            calls=[('before','put',json.dumps({'target':'sequence','value':{'n':1}})),('bad-read','get','{"target":17}'),('bad-write','update_short_state','{"slot":0,"text":"must not write"}'),('absent','not_installed','{"grant":"admin"}'),('after','get','{"target":"sequence"}'),('clock','time_get_current','{}'),('send','send','{"text":"one original delivery"}'),('malformed','get','{')]
            original=answer(protocol,calls)
            with o.svc.running():
                event=o.human('Continue independent original calls')
                _,q=o.wire(model);q.put((original,0))
                wire,q=o.wire(model)
                r=receipts(protocol,wire)
                ctx.equal(protocol+' every original call ID is paired in order',[i for i,_ in r],[i for i,_,_ in calls])
                result=dict(r)
                ctx.equal(protocol+' independent effects confirm',[result[i]['state'] for i in ['before','after','clock','send']],['confirmed']*4)
                ctx.equal(protocol+' bad calls are individually failed',[result[i]['state'] for i in ['bad-read','bad-write','absent','malformed']],['failed']*4)
                for i in ['bad-read','bad-write','absent','malformed']:
                    ctx.equal(protocol+' rejected call has no dispatch '+i,result[i]['result']['executed'],False)
                    ctx.equal(protocol+' original rejection category '+i,result[i]['result']['error'],'tool_not_declared' if i=='absent' else 'invalid_tool_arguments')
                ctx.equal(protocol+' write barrier precedes later read',result['after']['result']['value']['value'],{'n':1})
                ctx.equal(protocol+' invalid mutation writes no short state',o.rows('SELECT count(*) FROM short_state'),[(0,)])
                ctx.equal(protocol+' only four valid calls consume the business budget',o.rows('SELECT used_calls FROM root_tool_budgets'),[(4,)])
                ctx.equal(protocol+' one physical external send',len(model.deliveries),1)
                ctx.equal(protocol+' original provider reply remains exact',json.loads(o.rows('SELECT response FROM model_attempts ORDER BY rowid LIMIT 1')[0][0]),original)
                q.put((answer(protocol),0));o.finished(event)
                before=o.rows('SELECT operation_id,state,result FROM operations ORDER BY rowid')
            with o.svc.running():
                ctx.equal(protocol+' cold start retains all original receipts',o.rows('SELECT operation_id,state,result FROM operations ORDER BY rowid'),before)
                ctx.equal(protocol+' cold start never dispatches rejected calls',len(model.deliveries),1)
                ctx.equal(protocol+' no new purchase on cold start',o.rows('SELECT count(*) FROM model_attempts'),[(2,)])
                bad_event=o.human('Duplicate IDs cannot be paired')
                _,q=o.wire(model)
                duplicate=answer(protocol,[('duplicate','put','{"target":"must-not-exist","value":1}'),('duplicate','put','{"target":"must-not-exist","value":2}')])
                q.put((duplicate,0))
                o.until(lambda:o.rows("SELECT state FROM executions WHERE source=?",bad_event)==[('held',)])
                ctx.equal(protocol+' duplicate IDs never commit any partial effect',o.rows('SELECT operation_id,state,result FROM operations ORDER BY rowid'),before)
                ctx.equal(protocol+' duplicate IDs retain original invalid body',json.loads(o.rows('SELECT response FROM model_attempts ORDER BY rowid DESC LIMIT 1')[0][0]),duplicate)
                ctx.equal(protocol+' duplicate ID failure never dispatches externally',len(model.deliveries),1)
            proof={'protocol':protocol,'original':original,'receipts':r,'physical_sends':len(model.deliveries),'retained_operations':before,'duplicate_ids_held':True}
            ctx.write('tool-call-isolation-'+protocol+'.json',proof)
            all_results.append(proof)
    return {'oracle_sha256':hashlib.sha256(oracle_path.read_bytes()).hexdigest(),'protocols':all_results,'paid_calls':0,'source':'physical Main and original SQLite writer'}
