"""Original facts are restored conditionally; stale, foreign and erased sources never resurrect."""
import hashlib
import json
import sqlite3
from contextlib import closing
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def run(ctx):
    o=Owner(ctx,'restore-domain',43)
    def read(ex,fid,actor=None):
        return o.op('memory','read',ex,actor or o.actor,'--fact',fid,'--evidence')['items'][0]
    def edit(ex,key,body,actor=None,ok=True):
        return o.svc.command('operator','memory','edit',ex,actor or o.actor,key,json.dumps(body),accepted=ok)
    def change(action,row,**fields):
        return dict(action=action,fact=row['fact'],revision=row['revision'],proof=row['proof'],**fields)
    with o.svc.running():
        event=o.human('original evidence for owned memory')
        ex=o.op('ingress','claim',event)['execution']
        other='00000000-0000-4000-8000-000000000823'
        o.op('person','create',other)
        account=o.op('account','open','qq','compact','201',1,'--person',other,'--start',1)
        o.human('own group observation','700')
        foreign_event=o.op('ingress','record',json.dumps({'source':'foreign-original','account':account['account'],'bot_stage':o.bot['assignment'],'kind':'message','space':'700','occurred':1000,'observed':1000,'segments':[{'type':'text','text':'shared group observer'}]}),'--accept')['event']
        foreign_exec=o.op('ingress','claim',foreign_event)['execution']
        source={'kind':'event','id':event,'quote':'original evidence'}
        base={'action':'add','subject':{'kind':'person','person':o.actor},'fact_key':'drink','body':'first tea','confidence':0.67,'evidence':[source]}
        original=edit(ex,'create',base);fid=original['fact']
        row=read(ex,fid)
        edit(ex,'correct',change('correct',row,body='latest tea'))
        row=read(ex,fid);edit(ex,'withdraw',change('retract',row))
        withdrawn=read(ex,fid)
        ctx.equal('withdrawal removes searchable text',o.rows('SELECT content FROM memory_retrieval_fts'),[])
        restore=change('restore',withdrawn,evidence=[source])
        edit(ex,'stale-before-withdraw',change('restore',row,evidence=[source]),ok=False)
        edit(ex,'no-proof-source',dict(restore,evidence=[]),ok=False)
        edit(ex,'fabricated-source',dict(restore,evidence=[dict(source,quote='owned memory'),dict(source,quote='fabricated')]),ok=False)
        ctx.equal('rejected restores leave version and evidence counts unchanged',o.rows('SELECT (SELECT count(*) FROM memory_versions),(SELECT count(*) FROM memory_evidence)'),[(3,1)])
        foreign=read(foreign_exec,fid,other)
        ctx.equal('shared group summary grants no private evidence',foreign['readable_evidence'],0)
        edit(foreign_exec,'foreign-restore',change('restore',foreign,evidence=[{'kind':'event','id':foreign_event,'quote':'shared group observer'}]),actor=other,ok=False)
        restored=edit(ex,'restore',restore)
        row=read(ex,fid)
        ctx.equal('restore retains original ID, latest content and omitted confidence',[row['fact'],row['revision'],row['status'],row['body'],row['confidence']],[fid,4,'active','latest tea',0.67])
        ctx.equal('FTS includes exactly restored current text',o.rows('SELECT content FROM memory_retrieval_fts'),[('latest tea',)])
        edit(ex,'active-is-not-withdrawn',change('restore',row,evidence=[source]),ok=False)
        edit(ex,'later',change('correct',row,body='later preserved tea'))
        ctx.equal('old restore only reads its original receipt',edit(ex,'restore',restore),restored)
        ctx.equal('old restore cannot overwrite later correction',read(ex,fid)['body'],'later preserved tea')
    with o.svc.running():
        ctx.equal('cold replay returns the same restore receipt',edit(ex,'restore',restore),restored)
        ctx.equal('cold replay creates no new version',o.rows('SELECT count(*) FROM memory_versions'),[(5,)])
        row=read(ex,fid);edit(ex,'withdraw-again',change('retract',row))
        snapshot=read(ex,fid)
    # Offline fixture changes at a constant fact revision model replaced physical evidence.
    with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:
        db.execute('UPDATE memory_evidence SET id=id+500 WHERE fact=?',(fid,));db.commit()
    with o.svc.running():
        edit(ex,'replaced-evidence',change('restore',snapshot,evidence=[source]),ok=False)
        fresh=read(ex,fid)
        ctx.check('constant evidence count does not preserve original proof',fresh['proof']!=snapshot['proof'])
        edit(ex,'fresh-evidence',change('restore',fresh,evidence=[source]))
        row=read(ex,fid);edit(ex,'withdraw-for-expiry',change('retract',row))
        expired_read=read(ex,fid)
    with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:
        db.execute("INSERT INTO extract_fact_details(fact,valid_until) VALUES(?,'2001-01-01T00:00:00Z')",(fid,));db.commit()
    with o.svc.running():
        before=o.rows('SELECT count(*) FROM memory_versions')
        edit(ex,'expired',change('restore',expired_read,evidence=[source]),ok=False)
        ctx.equal('expired restore leaves versions unchanged',o.rows('SELECT count(*) FROM memory_versions'),before)
    with o.svc.running():
        o.op('privacy','erase','restore-forget',o.actor,json.dumps([event]))
        edit(ex,'forgotten',change('restore',expired_read,evidence=[source]),ok=False)
        ctx.equal('forgotten fact remains erased',o.rows('SELECT status FROM memory_facts WHERE id=?',fid),[('erased',)])
        ctx.equal('forgotten versions retain no body',o.rows('SELECT count(*) FROM memory_versions WHERE body IS NOT NULL'),[(0,)])
    oracle=ctx.root/'fixtures/memory/v4-restore.json'
    ctx.equal('fixed original restoration tests pass',json.loads(oracle.read_bytes())['pytest_exit'],0)
    return {'oracle_sha256':hashlib.sha256(oracle.read_bytes()).hexdigest(),'domain':{'restored':restored,'original':original,'forgotten_unrecoverable':True},'paid':paid(ctx)}


def paid(ctx):
    o=Owner(ctx,'restore-paid',43)
    with Recorder() as model:
        o.config(model,'root_tool_calls=32\n')
        with o.svc.running():
            event=o.human('restore my original tea observation')
            wire,q=o.wire(model)
            declaration=next(t['function'] for t in wire['tools'] if t['function']['name']=='memory_change')
            ctx.check('restore is declared only with implemented operations','restore' in declaration['parameters']['properties']['operation']['enum'])
            def tool(id,name,args):
                nonlocal wire,q
                reply(q,calls=[call(id,name,args)])
                wire,q=o.wire(model)
                return json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')==id))
            created=tool('new','memory_change',{'operation':'create','memory_key':'tea','new_content':'latest remembered tea','confidence':0.61})
            ctx.equal('omitted quote binds real original inbound text',created['state'],'confirmed')
            n=created['result']['fact_id'];fid=created['result']['fact']
            tool('read','read_memory',{'view':'fact','fact_id':n})
            withdrawn=tool('withdraw','memory_change',{'operation':'invalidate','fact_id':n})
            ctx.equal('model withdrawal succeeds',withdrawn['state'],'confirmed')
            stale=tool('stale','memory_change',{'operation':'restore','fact_id':n})
            ctx.equal('stale paid read cannot restore newer withdrawn fact',stale['state'],'failed')
            reply(q,'withdrawn');o.finished(event)
            event=o.human('please restore the withdrawn original fact')
            wire,q=o.wire(model)
            missing=tool('unread','memory_change',{'operation':'restore','fact_id':n})
            ctx.equal('a new execution must read the original fact itself',missing['state'],'failed')
            tool('evidence','read_memory',{'view':'evidence','fact_id':n})
            detail=tool('detail-only','memory_change',{'operation':'restore','fact_id':n})
            ctx.equal('evidence view does not grant a fact mutation',detail['state'],'failed')
            tool('read-withdrawn','read_memory',{'view':'fact','fact_id':n})
            wrong=tool('wrong-state','memory_change',{'operation':'restore','fact_id':n,'expected_fact_state':'active'})
            ctx.equal('expected original state is enforced',wrong['state'],'failed')
            restored=tool('restore','memory_change',{'operation':'restore','fact_id':n,'expected_fact_state':'invalidated'})
            ctx.equal('paid restore returns confirmed original version',[restored['state'],restored['result']['fact_id'],restored['result']['revision']],['confirmed',n,3])
            ctx.equal('paid restore preserves content and confidence',o.rows('SELECT v.body,v.confidence FROM memory_versions v JOIN memory_facts f ON f.id=v.fact AND f.revision=v.revision WHERE f.id=?',fid),[('latest remembered tea',0.61)])
            ctx.equal('exact default quote comes from original event',o.rows('SELECT quote FROM memory_evidence WHERE fact=? ORDER BY id',fid),[('restore my original tea observation',),('please restore the withdrawn original fact',)])
            reply(q,'done');o.finished(event)
            before=o.rows('SELECT operation_id,state,result FROM operations ORDER BY rowid')
        with o.svc.running():
            ctx.equal('cold paid original receipts remain paired',o.rows('SELECT operation_id,state,result FROM operations ORDER BY rowid'),before)
            ctx.equal('cold paid recovery restores no new version',o.rows('SELECT count(*) FROM memory_versions'),[(3,)])
        return {'restored':restored,'retained_operations':before,'paid_calls':0}
