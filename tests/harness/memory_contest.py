"""Actual Main tool calls against the pinned v4 mark/alternative transition."""
import hashlib
import json
import sqlite3
from contextlib import closing
from tests.harness.compact import Owner, call, reply
from tests.harness.memory_selectors import seed
from tests.peers.chat.recorder import Recorder


def run(ctx):
    o = Owner(ctx, 'contest-transitions', 52)
    oracle = ctx.root / 'fixtures/memory/v4-contest.json'
    original = json.loads(oracle.read_bytes())
    target = {'scope_type': 'person', 'subject_ref': 'current_speaker'}
    actions = []

    def state():
        return [o.rows('SELECT number,revision,status FROM memory_facts ORDER BY number'),
                o.rows('SELECT (SELECT count(*) FROM memory_versions),(SELECT count(*) FROM memory_evidence),(SELECT count(*) FROM memory_conflict_flags),(SELECT count(*) FROM memory_conflict_actions)')]

    def invoke(name, args, tag, expected='confirmed'):
        nonlocal q
        reply(q, calls=[call(tag,name,args)])
        wire,q = o.wire(model)
        r = json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')==tag))
        ctx.equal(tag+' paid receipt '+str(r['result'].get('error','')),r['state'],expected)
        actions.append({'call':tag,'state':r['state']})
        return r
    def read(number,tag):return invoke('read_memory',{'fact_id':number,'view':'fact'},tag)['result']
    def shape(x):return {'content':x['body'],'status':x['status'],'conflict':x['conflict_state'],'evidence_count':x['readable_evidence']}
    def end(event):
        reply(q,'done')
        o.finished(event)

    with Recorder() as model:
        ids,ns,evs = seed(o,[{'person':'1001','category':'music','memory_key':'music:favorite','content':'喜欢爵士乐'},
            {'person':'1002','category':'music','memory_key':'private','content':'An unrelated fact.'}],model)
        cfg=o.svc.data/'yuki.toml';cfg.write_text(cfg.read_text().replace('root_slots=8','root_slots=32'))
        change={'operation':'contest','fact_id':ns[0],'reason':'user_requested_review'}
        alternative=dict(change,new_content='喜欢摇滚乐')
        with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:
            db.execute("CREATE TRIGGER contest_test_abort BEFORE INSERT ON memory_conflict_actions WHEN NEW.alternative IS NOT NULL BEGIN SELECT RAISE(ABORT,'injected relation failure'); END");db.commit()
        with o.svc.running():
            event=o.human('Fault before storing the relation');_,q=o.wire(model)
            read(ns[0],'fault-read');before=state()
            fault=invoke('memory_change',alternative,'abort-contest','failed')
            ctx.check('fault occurred after both fact versions and evidence','injected relation failure' in str(fault['result']))
            ctx.equal('source and alternative fully rolled back',state(),before);end(event)
        with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:
            db.execute('DROP TRIGGER contest_test_abort');db.commit()
        with o.svc.running():
            evt_mark=o.human('先标为有争议');wire,q=o.wire(model)
            ctx.check('contest visible on real paid wire',any(t['function']['name']=='memory_change' and 'contest' in t['function']['parameters']['properties']['operation']['enum'] for t in wire['tools']))
            before=state();invoke('memory_change',change,'missing-read','failed')
            projection={'before':shape(read(ns[0],'paid-original'))}
            invoke('memory_change',dict(change,fact_id=ns[1]),'foreign-original','failed')
            invoke('memory_change',dict(change,evidence_quote='not in original event'),'forged-evidence','failed')
            ctx.equal('no mutation after rejected evidence or ownership',state(),before)
            marked=invoke('memory_change',dict(operation='contest',selector={'memory_key':'music:favorite'},target=target),'mark-original')
            projection['mark_outcome']=marked['result']['outcome']
            ctx.equal('mark keeps original ID',marked['result']['new_fact_id'],ns[0])
            before=state();invoke('memory_change',alternative,'stale-contest','failed')
            ctx.equal('stale alternative cannot create a row',state(),before)
            projection['marked']=shape(read(ns[0],'marked-fact'));end(evt_mark)
            evt_alt=o.human('我喜欢摇滚乐');_,q=o.wire(model);read(ns[0],'re-read-original')
            alt=invoke('memory_change',alternative,'add-contradiction')
            alt_num=alt['result']['new_fact_id'];ctx.check('distinct returned alternative',alt_num!=ns[0] and alt['result']['old_fact_id']==ns[0])
            projection['alternative_outcome']=alt['result']['outcome']
            projection['after_alternative']=shape(read(ns[0],'preserved-source'))
            other=read(alt_num,'read-disputed-alternative');projection['alternative']=shape(other)
            ctx.equal('alternative inherited category/key from the real original',[other['metadata']['category'],other['fact_key']],['music','music:favorite'])
            ctx.equal('original default confidence for an alternative',other['confidence'],0.9)
            projection['links']=[{'type':kind,'source_is_alternative':src==other['fact'],'target_is_original':dst==ids[0]} for src,dst,kind in o.rows('SELECT source,target,relation_type FROM memory_contradictions')]
            ctx.equal('both visible transitions, receipts and edge match pinned v4',projection,original['projection'])
            invoke('memory_change',{'operation':'restore','fact_id':alt_num},'no-restore-contested','failed')
            invoke('memory_change',dict(change,fact_id=alt_num),'no-active-write-to-alternative','failed')
            ctx.equal('default current facts omit disputed alternative',o.rows('SELECT number FROM retrieval_documents WHERE number IN (?,?)',ns[0],alt_num),[(ns[0],)])
            ex=o.rows('SELECT id FROM executions WHERE source=?',evt_alt)[0][0]
            result=o.op('memory','retrieve',ex,o.actor,'search-active','music:favorite')
            ctx.equal('real ranked result distinguishes active disputed original',[(x['fact_id'],x['conflict_state']) for x in result['items']],[(ns[0],'contested')]);end(evt_alt)
            kept=state()
        with o.svc.running():
            cold=o.op('effect','read',ex,alt['id'],o.actor)
            ctx.equal('cold immutable contest receipt',cold['result'],alt['result'])
            ctx.equal('cold lookup did not create another edge or version',state(),kept)
            o.op('privacy','erase','erase-original-proof',o.actor,json.dumps([evs[0]]))
            ctx.equal('origin withdrawal prevents both bodies being recovered',o.rows('SELECT count(*) FROM memory_versions WHERE fact IN (?,?) AND body IS NOT NULL',ids[0],other['fact']),[(0,)])
            ctx.equal('erase wins over disputed status',o.rows('SELECT status,conflict_state FROM memory_fact_states WHERE id IN (?,?)',ids[0],other['fact']),[('erased','clear')]*2)
            ctx.equal('unrelated original survives source withdrawal',o.rows('SELECT status FROM memory_fact_states WHERE id=?',ids[1]),[('active',)])
    return {'oracle_sha256':hashlib.sha256(oracle.read_bytes()).hexdigest(),'oracle_source':original['source_commit'],'projection':projection,'cases':actions,'late_fault_rolled_back':True,'cold_receipt':True,'erasure_verified':True}
