"""Original paid reads, actual writer rollback and erase propagation across a merge."""
import hashlib
import json
import sqlite3
from contextlib import closing
from tests.harness.compact import Owner, call, reply
from tests.harness.memory_selectors import seed
from tests.peers.chat.recorder import Recorder


def run(ctx):
    o=Owner(ctx,'merge-facts',51)
    oracle=ctx.root/'fixtures/memory/v4-merge.json'
    own={'person':'1001','category':'music'}
    target={'scope_type':'person','subject_ref':'current_speaker'}
    logs=[]
    with Recorder() as model:
        ids,ns,evs=seed(o,[dict(own,memory_key='music:jazz',content='I like jazz.'),dict(own,memory_key='music:favorite',content='Jazz is my favorite.'),dict(own,person='1002',memory_key='music:foreign',content='Private outsider fact.'),dict(own,memory_key='music:stale',content='A different preference.')],model)
        cfg=o.svc.data/'yuki.toml';cfg.write_text(cfg.read_text().replace('root_slots=8','root_slots=32'))
        def state():
            return [o.rows('SELECT number,revision,status FROM memory_facts ORDER BY number'),o.rows('SELECT (SELECT count(*) FROM memory_versions),(SELECT count(*) FROM memory_evidence),(SELECT count(*) FROM memory_equivalences)')]
        def invoke(name,args,tag,expected):
            nonlocal q
            reply(q,calls=[call(tag,name,args)]);wire,q=o.wire(model)
            receipt=json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')==tag))
            ctx.equal(tag+' paid receipt',receipt['state'],expected)
            logs.append({'call':tag,'state':receipt['state']});return receipt
        def read(n,tag):return invoke('read_memory',{'fact_id':n,'view':'fact'},tag,'confirmed')['result']
        merge={'operation':'merge','target':target,'fact_id':ns[0],'merge_fact_id':ns[1],'reason':'same preference'}
        # Test-only SQLite fault at the second version insert, after source retirement.
        with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:
            db.execute("CREATE TRIGGER merge_test_abort BEFORE INSERT ON memory_versions WHEN NEW.revision=2 AND (SELECT number FROM memory_facts WHERE id=NEW.fact)="+str(int(ns[1]))+" BEGIN SELECT RAISE(ABORT,'injected destination failure'); END");db.commit()
        with o.svc.running():
            evt=o.human('Merge the two facts that describe my preference');_,q=o.wire(model)
            read(ns[0],'read-source');before=state()
            invoke('memory_change',merge,'missing-destination-read','failed')
            read(ns[1],'read-destination')
            rejected=invoke('memory_change',merge,'second-version-abort','failed')
            ctx.check('injected error reached the real second version insert','injected destination failure' in str(rejected['result']))
            ctx.equal('mid-merge abort rolled back revisions, evidence and relation',state(),before)
            reply(q);o.finished(evt)
        with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:
            db.execute('DROP TRIGGER merge_test_abort');db.commit()
        with o.svc.running():
            evt=o.human('These music facts express the same preference');wire,q=o.wire(model)
            decl=next(t['function']['parameters'] for t in wire['tools'] if t['function']['name']=='memory_change')
            ctx.check('paid view declares both original merge selectors','merge' in decl['properties']['operation']['enum'] and {'merge_selector','merge_fact_id'}<=decl['properties'].keys())
            first=read(ns[0],'first-original');second=read(ns[1],'second-original')
            for tag,args in [('self-merge',dict(merge,merge_fact_id=ns[0])),('foreign-merge',dict(merge,merge_fact_id=ns[2])),('conflicting-target',dict(merge,merge_selector={'memory_key':'music:favorite'}))]:
                before=state();invoke('memory_change',args,tag,'failed');ctx.equal(tag+' no partial changes',state(),before)
            read(ns[3],'original-stale-control')
            invoke('memory_change',{'operation':'update_metadata','fact_id':ns[3],'target':target,'importance':4},'advance-target','confirmed')
            before=state();invoke('memory_change',dict(merge,merge_fact_id=ns[3]),'stale-target','failed');ctx.equal('stale paid target leaves source active',state(),before)
            args={'operation':'merge','selector':{'memory_key':'music:jazz'},'merge_selector':{'memory_key':'music:favorite'},'target':target,'reason':'same preference'}
            accepted=invoke('memory_change',args,'original-merge','confirmed')
            ctx.equal('retains destination and supersedes source',[accepted['result']['fact_id'],accepted['result']['source_fact_id'],accepted['result']['outcome']],[ns[1],ns[0],'committed'])
            src=read(ns[0],'see-superseded');dest=read(ns[1],'see-kept')
            ctx.equal('old fact reports true terminal status',src['status'],'superseded')
            ctx.equal('destination keeps original content and metadata',[dest['body'],dest['metadata']],[second['body'],second['metadata']])
            ctx.check('copied evidence and original merge event on retained target',set(e[0] for e in o.rows('SELECT event FROM memory_evidence WHERE fact=?',ids[1])) >= {evs[0],evs[1],evt})
            projection={'source_status_before':first['status'],'source_status_after':src['status'],'destination_status':dest['status'],'kept_destination_id':dest['fact_id']==ns[1],'kept_destination_content':dest['body']=='Jazz is my favorite.','source_evidence_before':first['readable_evidence'],'destination_evidence_before':second['readable_evidence'],'destination_evidence_after':dest['readable_evidence']}
            ctx.equal('retirement, retained destination and transferred evidence agree with V4',projection,json.loads(oracle.read_bytes())['original_transition'])
            ctx.equal('merged source cannot return in FTS',o.rows('SELECT rowid FROM memory_retrieval_fts WHERE rowid=?',ns[0]),[])
            invoke('memory_change',{'operation':'restore','fact_id':ns[0],'target':target},'cannot-restore-superseded','failed')
            reply(q);o.finished(evt)
            ex=o.rows('SELECT id FROM executions WHERE source=?',evt)[0][0]
            kept=state()
        with o.svc.running():
            cold=o.op('effect','read',ex,accepted['id'],o.actor)
            ctx.equal('cold original merge receipt immutable',cold['result'],accepted['result'])
            ctx.equal('cold lookup does not add any revision or relation',state(),kept)
            o.op('privacy','erase','erase-merge-source',o.actor,json.dumps([evs[0]]))
            ctx.equal('copied source erasure scrubs both fact histories',o.rows('SELECT count(*) FROM memory_versions WHERE fact IN (?,?) AND body IS NOT NULL',ids[0],ids[1]),[(0,)])
            ctx.equal('copied quote erased together with all target evidence',o.rows('SELECT count(*) FROM memory_evidence WHERE fact IN (?,?) AND quote IS NOT NULL',ids[0],ids[1]),[(0,)])
            ctx.equal('unrelated original owner remains active',o.rows('SELECT status FROM memory_facts WHERE id=?',ids[2]),[('active',)])
    return {'oracle_sha256':hashlib.sha256(oracle.read_bytes()).hexdigest(),'oracle_source':json.loads(oracle.read_bytes())['source_commit'],'cases':logs,'retained_fact':ns[1],'superseded_fact':ns[0],'original_transition':projection,'atomic_rollback':True,'cold_receipt':True,'copied_evidence_erased':True}
