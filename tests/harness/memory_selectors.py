"""Physical paid selectors retain exact original targets and failed-read source dependencies."""
import hashlib,json,sqlite3
from contextlib import closing
from tests.harness.compact import Owner,call,reply
from tests.peers.chat.recorder import Recorder


def run(ctx):
    oracle_path=ctx.root/'fixtures/memory/v4-selectors.json'
    oracle=json.loads(oracle_path.read_bytes())
    results=[]
    for case in oracle['cases']:
        o=Owner(ctx,'selectors-'+case['name'],44)
        with Recorder() as model:
            ids,numbers,events=seed(o,case['facts'],model)
            # Preserve the actual original ordering inputs, including timestamps.
            with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:
                for fid,row in zip(ids,case['facts']):db.execute('UPDATE memory_versions SET created_at=? WHERE fact=?',(row['updated_at'],fid))
                db.commit()
            with o.svc.running():
                event=o.human('withdraw the exact original fact');wire,q=o.wire(model)
                def tool(name,args,label):
                    nonlocal wire,q
                    reply(q,calls=[call(label,name,args)]);wire,q=o.wire(model)
                    return json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')==label))
                if case['ok']:
                    selected=case['states'].index('invalidated')
                    tool('read_memory',{'view':'fact','fact_id':numbers[selected]},'original-read')
                selected=tool('memory_change',{'operation':'invalidate','selector':case['selector'],'target':{'scope_type':'person','subject_ref':'current_speaker'}},'selector')
                ctx.equal(case['name']+' agrees with the actual original selector outcome',selected['state'],'confirmed' if case['ok'] else 'failed')
                if not case['ok']:
                    ctx.equal(case['name']+' keeps original rejection reason',selected['result']['reason_code'],case['reason_code'])
                    expected=[]
                    for index,original in zip(case['candidate_indices'],case['candidate_rows']):
                        expected.append(dict(original,fact_id=numbers[index],memory_ref='M'+str(numbers[index])))
                    ctx.equal(case['name']+' keeps all original bounded candidate fields',selected['result']['candidates'],expected)
                    ctx.check(case['name']+' candidates never contain source quotes',all('evidence' not in c and 'proof' not in c for c in selected['result']['candidates']))
                states=[o.rows('SELECT status FROM memory_facts WHERE id=?',fid)[0][0] for fid in ids]
                ctx.equal(case['name']+' changes only the original exact fact',states,['retracted' if v=='invalidated' else v for v in case['states']])
                ctx.write('selector-case-'+case['name']+'.json',{'selected':selected,'states':states,'event':event})
                reply(q,'done');o.finished(event)
                execution=o.rows('SELECT id FROM executions WHERE source=?',event)[0][0]
                receipts=o.rows('SELECT operation_id,state,result FROM operations WHERE namespace=? ORDER BY rowid',execution)
                requests=o.rows('SELECT count(*) FROM model_requests')
            with o.svc.running():
                ctx.equal(case['name']+' cold owner keeps original receipts',o.rows('SELECT operation_id,state,result FROM operations WHERE namespace=? ORDER BY rowid',execution),receipts)
                ctx.equal(case['name']+' cold owner does not repurchase selection',o.rows('SELECT count(*) FROM model_requests'),requests)
                if case['name']=='ambiguous':
                    source=events[case['candidate_indices'][0]]
                    ctx.equal('failed candidate read retains its original source dependency',o.rows('SELECT count(*) FROM main_dependencies WHERE execution=? AND event=?',execution,source),[(1,)])
                    o.op('privacy','erase','erase-selector',o.actor,json.dumps([source]))
                    ctx.equal('forget clears retained candidate payload while keeping original receipt key',json.loads(o.rows("SELECT result FROM operations WHERE namespace=? AND operation_id=?",execution,selected['id'])[0][0]),{'erased':True})
            results.append({'name':case['name'],'selector_result':selected,'states':states,'requests':requests})
    return {'oracle_sha256':hashlib.sha256(oracle_path.read_bytes()).hexdigest(),'cases':results,'boundaries':boundaries(ctx),'pagination':pagination(ctx)}


def boundaries(ctx):
    o=Owner(ctx,'selector-boundaries',44)
    target={'scope_type':'person','subject_ref':'current_speaker'}
    with Recorder() as model:
        ids,numbers,events=seed(o,[{'memory_key':'tea','content':'original tea','category':'drink','person':'1001'}],model)
        fid,n,source=ids[0],numbers[0],events[0]
        with o.svc.running():
            event=o.human('reject selector shortcuts');wire,q=o.wire(model)
            def tool(name,args,label):
                nonlocal wire,q
                reply(q,calls=[call(label,name,args)]);wire,q=o.wire(model)
                return json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')==label))
            invalid=[
                {'operation':'invalidate','selector':{'memory_key':'tea'},'target':target},
                {'operation':'invalidate','selector':{'memory_key':'tea'}},
                {'operation':'invalidate','selector':{'category':'drink'},'target':target},
                {'operation':'invalidate','selector':{'memory_key':'tea'},'fact_id':n,'target':target},
                {'operation':'create','selector':{'memory_key':'tea'},'memory_key':'new','new_content':'must not create','target':target},
                {'operation':'create','fact_id':n,'memory_key':'other-key','new_content':'must not create','target':target},
                {'operation':'invalidate','selector':{'memory_key':' \x00\n '},'target':target},
            ]
            for i,args in enumerate(invalid):
                rejected=tool('memory_change',args,'invalid-'+str(i));ctx.equal('invalid selector boundary '+str(i)+' fails',rejected['state'],'failed')
            ctx.equal('selector boundaries create no version or new fact',o.rows('SELECT (SELECT count(*) FROM memory_facts),(SELECT count(*) FROM memory_versions)'),[(1,1)])
            reply(q,'done');o.finished(event)
        with o.svc.running():
            event=o.human('read and update original selector metadata');wire,q=o.wire(model)
            tool('read_memory',{'view':'fact','fact_id':n},'read-metadata')
            revised=tool('memory_change',{'operation':'update_metadata','selector':{'memory_key':'tea'},'target':target,'category':'drinks'},'metadata-selector')
            ctx.equal('unique selector metadata uses the paid original read',revised['result']['revision'],2)
            stale=tool('memory_change',{'operation':'invalidate','selector':{'memory_key':'tea'},'target':target},'stale-selector')
            ctx.equal('selector cannot bypass the stale paid version',stale['state'],'failed')
            tool('read_memory',{'view':'fact','fact_id':n},'read-retract')
            withdrawn=tool('memory_change',{'operation':'invalidate','selector':{'memory_key':'tea'},'target':target},'retract-selector')
            ctx.equal('fresh original read authorizes exact withdrawal',withdrawn['result']['revision'],3)
            reply(q,'done');o.finished(event)
        with o.svc.running():
            event=o.human('restore my original withdrawn tea');wire,q=o.wire(model)
            tool('read_memory',{'view':'fact','fact_id':n},'read-restore')
            restored=tool('memory_change',{'operation':'restore','selector':{'memory_key':'tea'},'target':target},'restore-selector')
            ctx.write('restore-selector-result.json',restored)
            ctx.equal('restore selector succeeds after original read',restored['state'],'confirmed')
            ctx.equal('restore selector binds the original withdrawn fact',[restored['result']['fact'],restored['result']['revision']],[fid,4])
            reply(q,'done');o.finished(event)
        with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:
            db.execute("UPDATE extract_fact_details SET valid_until='2000-01-01T00:00:00Z' WHERE fact=?",(fid,));db.commit()
        with o.svc.running():
            event=o.human('inspect an expired selector');wire,q=o.wire(model)
            tool('read_memory',{'view':'fact','fact_id':n},'read-expired')
            expired=tool('memory_change',{'operation':'invalidate','selector':{'memory_key':'tea'},'target':target},'expired-selector')
            ctx.equal('selector excludes expired facts before any mutation',expired['result']['reason_code'],'memory_candidate_not_found')
            ctx.equal('expired selector does not add a version',o.rows('SELECT revision FROM memory_facts WHERE id=?',fid),[(4,)])
            reply(q,'done');o.finished(event)
            o.op('privacy','erase','erase-selector-source',o.actor,json.dumps([source]))
        with o.svc.running():
            event=o.human('find a forgotten selector');wire,q=o.wire(model)
            forgotten=tool('memory_change',{'operation':'restore','selector':{'memory_key':'tea'},'target':target},'forgotten-selector')
            ctx.equal('forgotten selector exposes no candidate',forgotten['result']['candidates'],[])
            reply(q,'done');o.finished(event)
    return {'metadata':revised,'withdrawal':withdrawn,'restored':restored,'paid_calls':0}


def seed(o,rows,model):
    """Create fixtures inside their actual Main executions, then finish each original turn."""
    o.config(model,'root_tool_calls=32\n')
    ids=[];numbers=[];events=[]
    with o.svc.running():
        for i,row in enumerate(rows):
            actor=o.actor
            if row['person']=='1001':event=o.human(row['content'])
            else:
                actor='00000000-0000-4000-8000-000000000823'
                o.op('person','create',actor)
                account=o.op('account','open','qq','compact','201',1,'--person',actor,'--start',1)
                event=o.op('ingress','record',json.dumps({'source':'foreign-'+str(i),'account':account['account'],'bot_stage':o.bot['assignment'],'kind':'message','occurred':1000,'observed':1000,'segments':[{'type':'text','text':row['content']}]}),'--accept')['event']
            _,q=o.wire(model)
            ex=o.rows('SELECT id FROM executions WHERE source=?',event)[0][0];events.append(event)
            result=o.op('memory','edit',ex,actor,'seed-'+str(i),json.dumps({'action':'add','subject':{'kind':'person','person':actor},'fact_key':row['memory_key'],'body':row['content'],'metadata':{'category':row['category'],'kind':'fact'},'evidence':[{'kind':'event','id':event,'quote':row['content']}]}))
            ids.append(result['fact']);numbers.append(o.rows('SELECT number FROM memory_facts WHERE id=?',result['fact'])[0][0])
            reply(q,'seed complete');o.finished(event)
    return ids,numbers,events


def pagination(ctx):
    o=Owner(ctx,'selector-page',44)
    with Recorder() as model:
        o.config(model,'root_tool_calls=32\n')
        with o.svc.running():
            text='; '.join('owned entry '+str(i) for i in range(129))
            event=o.human(text);wire,q=o.wire(model)
            ex=o.rows('SELECT id FROM executions WHERE source=?',event)[0][0]
            for i in range(129):
                created=o.op('memory','edit',ex,o.actor,'page-'+str(i),json.dumps({'action':'add','subject':{'kind':'person','person':o.actor},'fact_key':'tea' if i==128 else 'tea-'+str(i),'body':'owned entry '+str(i),'evidence':[{'kind':'event','id':event,'quote':'owned entry '+str(i)}]}))
            fid=created['fact'];n=o.rows('SELECT number FROM memory_facts WHERE id=?',fid)[0][0]
            reply(q,calls=[call('late-read','read_memory',{'view':'fact','fact_id':n})]);wire,q=o.wire(model)
            reply(q,calls=[call('late-exact','memory_change',{'operation':'invalidate','selector':{'memory_key':'tea'},'target':{'scope_type':'person','subject_ref':'current_speaker'}})]);wire,q=o.wire(model)
            result=json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')=='late-exact'))
            ctx.equal('exact match beyond the first authorized page is retained',result['result']['fact'],fid)
            ctx.equal('bounded candidate selection changes only the late exact fact',o.rows("SELECT count(*) FROM memory_facts WHERE status='retracted'"),[(1,)])
            reply(q,'done');o.finished(event)
    return {'authorized_facts':129,'exact_fact':fid,'receipt':result}
