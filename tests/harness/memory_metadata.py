"""Versioned fact metadata, original paid reads, atomic failures and retained erasure keys."""
import hashlib
import json
import sqlite3
from contextlib import closing
from tests.harness.compact import Owner, call, reply
from tests.harness.service import Service, previous_schema
from tests.peers.chat.recorder import Recorder


def run(ctx):
    o=Owner(ctx,'metadata-domain',43)
    def read(ex,fid):return o.op('memory','read',ex,o.actor,'--fact',fid)['items'][0]
    def edit(ex,key,body,ok=True):return o.svc.command('operator','memory','edit',ex,o.actor,key,json.dumps(body),accepted=ok)
    def change(action,row,**kw):return dict(action=action,fact=row['fact'],revision=row['revision'],proof=row['proof'],**kw)
    with o.svc.running():
        event=o.human('original tea facts; update metadata with real evidence')
        ex=o.op('ingress','claim',event)['execution']
        source={'kind':'event','id':event,'quote':'original tea facts'}
        original={'action':'add','subject':{'kind':'person','person':o.actor},'fact_key':'tea','body':'original tea','confidence':0.67,'evidence':[source],
                  'metadata':{'category':'  tea\x00\n category  ','kind':'fact','importance':3}}
        first=edit(ex,'create',original);fid=first['fact']
        row=read(ex,fid)
        ctx.equal('original normalization removes controls and repeated spaces',row['metadata']['category'],'tea category')
        update=change('metadata',row,fields={'memory_key':'  preferred\t tea ','category':'preference','kind':'preference','importance':4},evidence=[source])
        revised=edit(ex,'metadata',update)
        row=read(ex,fid)
        ctx.equal('metadata changes keep original fact and omitted confidence',[row['fact'],row['revision'],row['body'],row['confidence'],row['fact_key']],[fid,2,'original tea',0.67,'preferred tea'])
        ctx.equal('each revision preserves its actual metadata',o.rows('SELECT revision,memory_key,category,kind,importance FROM fact_metadata_versions WHERE fact=? ORDER BY revision',fid),[(1,'tea','tea category','fact',3),(2,'preferred tea','preference','preference',4)])
        ctx.equal('FTS follows current original key and category',o.rows('SELECT memory_key,category FROM memory_retrieval_fts'),[('preferred tea','preference')])
        for number,fields in enumerate([{'category':'\x00\n '},{'memory_key':''},{'kind':'unregistered'},{'importance':0},{'importance':6}]):
            edit(ex,'invalid-'+str(number),change('metadata',row,fields=fields,evidence=[dict(source,quote='update metadata')]),ok=False)
        edit(ex,'missing-evidence',change('metadata',row,fields={'category':'changed'},evidence=[]),ok=False)
        edit(ex,'forged-evidence',change('metadata',row,fields={'category':'changed'},evidence=[dict(source,quote='not in original event')]),ok=False)
        ctx.equal('invalid metadata rolls back versions, history and newly attached evidence',o.rows('SELECT (SELECT count(*) FROM memory_versions),(SELECT count(*) FROM fact_metadata_versions),(SELECT count(*) FROM memory_evidence)'),[(2,2,1)])
        edit(ex,'correct',change('correct',row,body='later corrected tea'))
        ctx.equal('replaying metadata reads only its original receipt',edit(ex,'metadata',update),revised)
        fresh=read(ex,fid)
        ctx.equal('correction retains unspecified classification',[fresh['body'],fresh['metadata']['category'],fresh['metadata']['kind'],fresh['metadata']['importance']],['later corrected tea','preference','preference',4])
        edit(ex,'withdraw',change('retract',fresh))
        retracted=read(ex,fid);edit(ex,'restore',change('restore',retracted,evidence=[source]))
        snapshot=read(ex,fid)
        before=o.rows('SELECT revision,memory_key,category,kind,importance FROM fact_metadata_versions WHERE fact=? ORDER BY revision',fid)
    with o.svc.running():
        ctx.equal('cold replay keeps original metadata receipt',edit(ex,'metadata',update),revised)
        ctx.equal('cold restore preserves every version',o.rows('SELECT revision,memory_key,category,kind,importance FROM fact_metadata_versions WHERE fact=? ORDER BY revision',fid),before)
    with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:
        db.execute("UPDATE extract_fact_details SET category='changed during read' WHERE fact=?",(fid,));db.commit()
    with o.svc.running():
        edit(ex,'stale-metadata',change('metadata',snapshot,fields={'category':'must not overwrite'},evidence=[source]),ok=False)
        ctx.check('same fact revision cannot hide changed metadata proof',read(ex,fid)['proof']!=snapshot['proof'])
        o.op('privacy','erase','erase-metadata',o.actor,json.dumps([event]))
        ctx.equal('erasure clears all historical metadata content',o.rows('SELECT count(*) FROM fact_metadata_versions WHERE memory_key IS NOT NULL OR category IS NOT NULL OR kind IS NOT NULL OR importance IS NOT NULL OR temporal_mode IS NOT NULL OR valid_from IS NOT NULL OR valid_until IS NOT NULL'),[(0,)])
        ctx.equal('erasure preserves version identities',o.rows('SELECT count(*) FROM fact_metadata_versions'),[(5,)])
        ctx.equal('erasure removes metadata FTS',o.rows('SELECT count(*) FROM memory_retrieval_fts'),[(0,)])
    oracle=ctx.root/'fixtures/memory/v4-metadata.json'
    ctx.equal('fixed original metadata tests pass',json.loads(oracle.read_bytes())['pytest_exit'],0)
    normalized=normalization(ctx,json.loads(oracle.read_bytes()))
    return {'oracle_sha256':hashlib.sha256(oracle.read_bytes()).hexdigest(),'upgrade':upgrade(ctx),'normalization':normalized,'domain':{'first':first,'metadata':revised,'versions':before,'erased_versions':5},'paid':paid(ctx)}


def paid(ctx):
    o=Owner(ctx,'metadata-paid',43)
    with Recorder() as model:
        o.config(model,'root_tool_calls=32\n')
        with o.svc.running():
            event=o.human('remember my tea with useful categories')
            wire,q=o.wire(model)
            def tool(id,name,args):
                nonlocal wire,q
                reply(q,calls=[call(id,name,args)]);wire,q=o.wire(model)
                return json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')==id))
            created=tool('new','memory_change',{'operation':'create','memory_key':'tea','new_content':'tea facts','category':'daily','kind':'fact','importance':3,'confidence':0.71})
            ctx.equal('paid create accepts real metadata',created['state'],'confirmed')
            n=created['result']['fact_id'];fid=created['result']['fact']
            unread=tool('unread','memory_change',{'operation':'update_metadata','fact_id':n,'category':'unread'})
            ctx.equal('paid metadata cannot mutate an unread original fact',unread['state'],'failed')
            tool('read','read_memory',{'view':'fact','fact_id':n})
            revised=tool('metadata','memory_change',{'operation':'update_metadata','fact_id':n,'memory_key':'preferred tea','category':'drink preference','kind':'preference','importance':4})
            ctx.equal('paid update consumes original read revision',revised['result']['revision'],2)
            view=tool('view','read_memory',{'view':'fact','fact_id':n})
            ctx.equal('paid view returns persisted classification',[view['result']['fact_key'],view['result']['metadata']['category'],view['result']['metadata']['kind'],view['result']['metadata']['importance'],view['result']['confidence']],['preferred tea','drink preference','preference',4,0.71])
            bad=tool('bad','memory_change',{'operation':'update_metadata','fact_id':n,'importance':0})
            ctx.equal('paid schema rejects invalid importance without a mutation',bad['state'],'failed')
            ctx.equal('invalid paid metadata creates no version',o.rows('SELECT count(*) FROM memory_versions'),[(2,)])
            reply(q,'done');o.finished(event)
            receipts=o.rows('SELECT operation_id,state,result FROM operations ORDER BY rowid')
        with o.svc.running():
            ctx.equal('cold metadata keeps all original paid receipts',o.rows('SELECT operation_id,state,result FROM operations ORDER BY rowid'),receipts)
        return {'updated':revised,'view':view,'retained_receipts':receipts,'paid_calls':0}


def upgrade(ctx):
    fixture=ctx.root/'fixtures/memory/reader43-metadata.json'
    old=json.loads(fixture.read_bytes())
    svc=Service(ctx,ctx.working/'metadata-upgrade')
    previous_schema(ctx,svc.data,43)
    (svc.data/'SOUL.md').write_text('Fixed public character.',encoding='utf-8')
    # Load the recorded rows without replaying their creation triggers. Restore
    # the original reader-43 triggers before migration or any owner operation.
    with closing(sqlite3.connect(svc.data/'core.sqlite')) as db:
        triggers=db.execute("SELECT name,sql FROM sqlite_master WHERE type='trigger'").fetchall()
        for name,_ in triggers:db.execute('DROP TRIGGER "'+name+'"')
        for table,data in old['tables'].items():
            db.execute('DELETE FROM "'+table+'"')
            columns=','.join('"'+c+'"' for c in data['columns'])
            db.executemany('INSERT INTO "'+table+'"('+columns+') VALUES('+','.join('?' for _ in data['columns'])+')',data['rows'])
        for _,sql in triggers:db.execute(sql)
        db.execute('INSERT INTO memory_retrieval_fts(rowid,content,memory_key,category) SELECT number,content,fact_key,category FROM retrieval_documents')
        ctx.equal('recorded reader 43 rows have valid foreign keys',db.execute('PRAGMA foreign_key_check').fetchall(),[])
        db.commit()
    ctx.equal('populated reader 43 migrates to current metadata reader',svc.command('migrate')['schema_version'],54)
    def rows(sql):
        with closing(sqlite3.connect((svc.data/'core.sqlite').as_uri()+'?mode=ro',uri=True)) as db:return db.execute(sql).fetchall()
    before=rows('SELECT * FROM memory_operations ORDER BY original_key')
    with svc.running():
        for original in old['operations']:
            got=svc.command('operator','memory','edit',old['execution'],old['actor'],original['key'],json.dumps(original['body']))
            ctx.equal('reader 43 '+original['key']+' retains exact original receipt after upgrade',got,original['result'])
        ctx.equal('upgrade never invents past metadata',rows('SELECT revision,memory_key,category,kind,importance FROM fact_metadata_versions'),[(2,'original-key','legacy category','preference',4)])
        ctx.equal('old receipt replay keeps exact request hashes',rows('SELECT * FROM memory_operations ORDER BY original_key'),before)
        ctx.equal('old receipt replay creates no fact versions',rows('SELECT count(*) FROM memory_versions'),[(2,)])
        view=svc.command('operator','memory','read',old['execution'],old['actor'],'--fact',old['fact'])['items'][0]
        changed=svc.command('operator','memory','edit',old['execution'],old['actor'],'new-metadata',json.dumps({'action':'metadata','fact':old['fact'],'revision':view['revision'],'proof':view['proof'],'fields':{'category':'post-upgrade'},'evidence':old['operations'][0]['body']['evidence']}))
        ctx.equal('new writer preserves known old metadata while creating next version',rows('SELECT revision,category FROM fact_metadata_versions ORDER BY revision'),[(2,'legacy category'),(3,'post-upgrade')])
    with svc.running():
        ctx.equal('cold metadata upgrade retains its original version',rows('SELECT revision FROM memory_facts'),[(3,)])
        ctx.equal('cold upgraded database remains referentially valid',rows('PRAGMA foreign_key_check'),[])
    return {'fixture_sha256':hashlib.sha256(fixture.read_bytes()).hexdigest(),'original_source_commit':old['source_commit'],'original_receipts':old['operations'],'new_receipt':changed}


def normalization(ctx,oracle):
    o=Owner(ctx,'metadata-normalize',43)
    checked=[]
    with o.svc.running():
        event=o.human('original normalization evidence');ex=o.op('ingress','claim',event)['execution']
        for index,vector in enumerate(oracle['normalization']):
            result=o.op('memory','edit',ex,o.actor,'normalize-'+str(index),json.dumps({'action':'add','subject':{'kind':'person','person':o.actor},'fact_key':'original','body':'normalization evidence','evidence':[{'kind':'event','id':event,'quote':'original normalization evidence'}],'metadata':{'memory_key':vector['input'],'category':vector['input']}}))
            row=o.op('memory','read',ex,o.actor,'--fact',result['fact'])['items'][0]
            ctx.equal('Rust metadata matches original normalization '+str(index),[row['fact_key'],row['metadata']['category']],[vector['expected']]*2)
            checked.append(vector)
    return checked
