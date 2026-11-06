"""Original date semantics through physical Main calls and the versioned writer."""
import hashlib,json,sqlite3
from contextlib import closing
from tests.harness.compact import Owner,call,reply
from tests.harness.service import Service,previous_schema
from tests.peers.chat.recorder import Recorder


def configure(o,model,extra):
    o.config(model,extra)
    # A bounded batch needs one actual response per vector plus the final
    # answer. Split before the ordinary 24-request segment boundary.
    p=o.svc.data/'yuki.toml';text=p.read_text();assert text.count('root_slots=8\n')==1
    p.write_text(text.replace('root_slots=8\n','root_slots=32\n'))


def run(ctx):
    path=ctx.root/'fixtures/memory/v4-temporal.json'
    original=json.loads(path.read_bytes());results=[]
    for index,zone in enumerate(dict.fromkeys(v['zone'] for v in original['vectors'])):
        o=Owner(ctx,'temporal-zone-'+str(index),44)
        with Recorder() as model:
            configure(o,model,'timezone='+json.dumps(zone)+'\nroot_tool_calls=128\n')
            with o.svc.running():
                event=o.human('Please remember my original dates');wire,q=o.wire(model)
                def tool(label,name,args):
                    nonlocal wire,q
                    reply(q,calls=[call(label,name,args)]);wire,q=o.wire(model)
                    return json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')==label))
                seen=0
                for number,v in enumerate(original['vectors']):
                    if v['zone']!=zone:continue
                    if seen==10:
                        reply(q,'done');o.finished(event)
                        event=o.human('Please remember my original dates');wire,q=o.wire(model);seen=0
                    seen+=1
                    args={'operation':'create','memory_key':'date-'+str(number),'category':'dated','new_content':'original date '+str(number)}
                    args.update({k:v[k] for k in ['valid_from','valid_until'] if v[k] is not None})
                    before=o.rows('SELECT count(*) FROM memory_versions')[0][0]
                    result=tool('date-'+str(number),'memory_change',args)
                    ctx.equal('original temporal parser outcome '+str(number),result['state'],'confirmed' if v['ok'] else 'failed')
                    if v['ok']:
                        fid=result['result']['fact']
                        expected=[(v['mode'],v['start'],v['end'])]
                        ctx.equal('original UTC temporal values '+str(number),o.rows('SELECT temporal_mode,valid_from,valid_until FROM extract_fact_details WHERE fact=?',fid),expected)
                        ctx.equal('first temporal version bound to original fact '+str(number),o.rows('SELECT temporal_mode,valid_from,valid_until FROM fact_metadata_versions WHERE fact=?',fid),expected)
                    else:
                        ctx.equal('invalid time rolls back new fact and version '+str(number),o.rows('SELECT (SELECT count(*) FROM memory_versions),(SELECT count(*) FROM memory_facts),(SELECT count(*) FROM extract_fact_details)'),[(before,before,before)])
                    results.append({'vector':number,'state':result['state']})
                forged=tool('forged-zone','memory_change',{'operation':'create','memory_key':'forged','new_content':'date','valid_from':'2032-01-01','timezone':'UTC'})
                ctx.equal('model cannot override deployment timezone',forged['state'],'failed')
                reply(q,'done');o.finished(event)
                receipts=o.rows('SELECT operation_id,state,result FROM operations ORDER BY rowid')
            with o.svc.running():
                ctx.equal('cold launch retains temporal receipts in '+zone,o.rows('SELECT operation_id,state,result FROM operations ORDER BY rowid'),receipts)
    return {'oracle_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'vectors':results,'operations':operations(ctx,original),'upgrade':upgrade(ctx),'paid_calls':0}


def operations(ctx,original):
    o=Owner(ctx,'temporal-operations',44)
    with Recorder() as model:
        configure(o,model,'root_tool_calls=64\n')
        with o.svc.running():
            event=o.human('Please remember and update my tea dates');wire,q=o.wire(model)
            def tool(label,name,args):
                nonlocal wire,q
                reply(q,calls=[call(label,name,args)]);wire,q=o.wire(model)
                return json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')==label))
            number=None;history=[]
            for index,step in enumerate(original['operations']):
                if number is not None:tool('read-'+str(index),'read_memory',{'view':'fact','fact_id':number})
                args={'operation':step['operation'],**step['fields']}
                if number is None:args.update(memory_key='tea',category='drink',new_content='tea dates')
                else:args['fact_id']=number
                result=tool('change-'+str(index),'memory_change',args)
                ctx.equal('real original temporal mutation '+str(index),result['state'],'confirmed')
                number=result['result']['fact_id'];fid=result['result']['fact']
                expected=(step['start'],step['end']);history.append((index+1,*expected))
                ctx.equal('current original interval '+str(index),o.rows('SELECT valid_from,valid_until FROM extract_fact_details WHERE fact=?',fid),[expected])
                ctx.equal('version snapshots original interval '+str(index),o.rows('SELECT revision,valid_from,valid_until FROM fact_metadata_versions WHERE fact=? ORDER BY revision',fid),history)
            tool('read-invalid','read_memory',{'view':'fact','fact_id':number})
            before=o.rows('SELECT count(*) FROM memory_versions')[0][0]
            invalid=tool('backwards','memory_change',{'operation':'update_metadata','fact_id':number,'category':'must roll back','valid_from':'2040-01-01','valid_until':'2030-01-01'})
            ctx.equal('inverted metadata update rejects',invalid['state'],'failed')
            ctx.equal('inverted update leaves prior metadata and version',o.rows('SELECT category,(SELECT count(*) FROM memory_versions) FROM extract_fact_details WHERE fact=?',fid),[('updated',before)])
            tool('next','memory_change',{'operation':'update_metadata','fact_id':number,'valid_until':'2037-01-01'})
            stale=tool('stale','memory_change',{'operation':'update_metadata','fact_id':number,'valid_until':'2038-01-01'})
            ctx.equal('prior read cannot overwrite changed temporal version',stale['state'],'failed')
            history=o.rows('SELECT revision,valid_from,valid_until FROM fact_metadata_versions WHERE fact=? ORDER BY revision',fid)
            reply(q,'done');o.finished(event)
            receipts=o.rows('SELECT operation_id,state,result FROM operations ORDER BY rowid')
        with o.svc.running():
            ctx.equal('cold original mutation receipts retain exact dates',o.rows('SELECT operation_id,state,result FROM operations ORDER BY rowid'),receipts)
            ctx.equal('cold original versions retain exact dates',o.rows('SELECT revision,valid_from,valid_until FROM fact_metadata_versions WHERE fact=? ORDER BY revision',fid),history)
            o.op('privacy','erase','erase-dates',o.actor,json.dumps([event]))
            ctx.equal('forget removes every temporal snapshot',o.rows('SELECT count(*) FROM fact_metadata_versions WHERE valid_from IS NOT NULL OR valid_until IS NOT NULL OR temporal_mode IS NOT NULL'),[(0,)])
            ctx.equal('forget removes current temporal projection',o.rows('SELECT count(*) FROM extract_fact_details WHERE valid_from IS NOT NULL OR valid_until IS NOT NULL OR temporal_mode IS NOT NULL'),[(0,)])
        return {'history':history,'cold_receipts':len(receipts),'forgotten_event':event}


def upgrade(ctx):
    path=ctx.root/'fixtures/memory/reader44-temporal.json';old=json.loads(path.read_bytes())
    svc=Service(ctx,ctx.working/'temporal-upgrade');previous_schema(ctx,svc.data,44)
    (svc.data/'SOUL.md').write_text('Fixed public character.')
    with closing(sqlite3.connect(svc.data/'core.sqlite')) as db:
        triggers=db.execute("SELECT name,sql FROM sqlite_master WHERE type='trigger'").fetchall()
        for name,_ in triggers:db.execute('DROP TRIGGER "'+name+'"')
        for name,data in old['tables'].items():
            db.execute('DELETE FROM "'+name+'"')
            columns=','.join('"'+c+'"' for c in data['columns'])
            db.executemany('INSERT INTO "'+name+'"('+columns+') VALUES('+','.join('?' for _ in data['columns'])+')',data['rows'])
        for _,sql in triggers:db.execute(sql)
        db.execute('INSERT INTO memory_retrieval_fts(rowid,content,memory_key,category) SELECT number,content,fact_key,category FROM retrieval_documents')
        ctx.equal('recorded reader 44 fixture has valid relations',db.execute('PRAGMA foreign_key_check').fetchall(),[])
        db.commit()
    def rows(sql,*args):
        with closing(sqlite3.connect((svc.data/'core.sqlite').as_uri()+'?mode=ro',uri=True)) as db:return db.execute(sql,args).fetchall()
    erased,active=old['erased_fact'],old['active_fact']
    original=rows('SELECT * FROM memory_operations ORDER BY original_key')
    active_details=rows('SELECT * FROM extract_fact_details WHERE fact=?',active)
    count=rows('SELECT count(*) FROM memory_versions')
    ctx.equal('old reader really retained erased dates',rows('SELECT kind,importance,valid_until FROM extract_fact_details WHERE fact=?',erased),[('episode',4,'2032-01-01T00:00:00Z')])
    ctx.equal('populated reader 44 upgrades without a down migration',svc.command('migrate')['schema_version'],52)
    ctx.equal('upgrade clears only erased detail content',rows('SELECT category,kind,importance,temporal_mode,valid_from,valid_until FROM extract_fact_details WHERE fact=?',erased),[(None,)*6])
    ctx.equal('upgrade keeps active original dates',rows('SELECT * FROM extract_fact_details WHERE fact=?',active),active_details)
    ctx.equal('upgrade preserves original operation identities and hashes',rows('SELECT * FROM memory_operations ORDER BY original_key'),original)
    with svc.running():
        op=old['operations'][1]
        got=svc.command('operator','memory','edit',op['execution'],old['actor'],op['key'],json.dumps(op['body']))
        ctx.equal('old dated receipt replays without replacing active dates',got,op['result'])
        ctx.equal('replay never resets original temporal metadata',rows('SELECT * FROM extract_fact_details WHERE fact=?',active),active_details)
        ctx.equal('replay never creates an extra fact version',rows('SELECT count(*) FROM memory_versions'),count)
    with svc.running():
        ctx.equal('cold upgraded erasure remains cleared',rows('SELECT valid_from,valid_until FROM extract_fact_details WHERE fact=?',erased),[(None,None)])
        ctx.equal('cold populated upgrade has valid relations',rows('PRAGMA foreign_key_check'),[])
    return {'fixture_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'source_commit':old['source_commit'],'binary_sha256':old['binary_sha256'],'retained_operations':len(original)}
