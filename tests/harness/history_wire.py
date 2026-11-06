"""Replay the fixed v4 public query cases against Rust's original ledger.

Normal: shared event IDs, literal/ASR filters, source intersection, numeric pages,
context, original delivery receipt. Faults: private source, erased cursor/anchor,
argument conflict, revoked source and immutable replay after later input.
"""
import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import sqlite3
import uuid
from contextlib import closing
from tests.harness.service import Service


def run(ctx):
    fixture = json.loads((ctx.root / 'fixtures/history/v4-original-query.json').read_bytes())
    ctx.equal('original history source is the fixed behavioral SHA', fixture['source_commit'],
              '329ca7c487414d2f54ddd98ff4727446ed2446cd')
    svc = Service(ctx, ctx.working / 'original-history-wire')
    svc.command('init', '--memory-mode', 'lexical-only')
    who = str(uuid.uuid4())
    def op(*args, ok=True):
        return svc.command('operator', *map(str, args), accepted=ok)
    def rows(sql, *args):
        with closing(sqlite3.connect((svc.data/'core.sqlite').as_uri()+'?mode=ro', uri=True)) as db:
            return db.execute(sql, args).fetchall()
    with svc.running():
        op('person', 'create', who, '--name', '成员')
        human = op('account', 'open', 'qq', 'history-wire', '10001', 1, '--person', who, '--start', 1)
        bot = op('account', 'open', 'qq', 'history-wire', '20001', 1, '--start', 1)
        alternate = op('account', 'open', 'telegram', 'history-wire', '30001', 1, '--person', who, '--start', 1)
        alternate_bot = op('account', 'open', 'telegram', 'history-wire', '40001', 1, '--start', 1)
        def event(key, text, minute=0, account=human, private=False, voice=False):
            frame = {'source':key, 'account':account['account'], 'bot_stage':(alternate_bot if account==alternate else bot)['assignment'],
                     'kind':'message','observed':1767312000000+minute*60000,
                     'occurred':1767312000000+minute*60000,
                     'segments':[{'type':'text','text':text}]}
            if not private: frame['space']=json.dumps(['qq','history-wire','80001'])
            if voice: frame['segments']=[{'type':'media','kind':'record','reference':'voice'}]
            return op('ingress','record',json.dumps(frame),'--accept')['event']
        original=event('hello','hello',-1)
        execution=op('ingress','claim',original)['execution']
        ids=[original,event('first','first context'),event('center','雷達站 electric',1),event('last','last context',2),event('voice','',3,voice=True)]
        voice_run=op('ingress','claim',ids[-1])['execution']
        op('history','transcribe',voice_run,who,ids[-1],json.dumps([
            {'source':'current','segment_index':0,'text':'乾坤 電動車'},
            {'source':'reply','segment_index':0,'text':'private quoted decoy'}]))
        ids.append(event('private','雷達站 private',4,account=alternate,private=True))
        group=rows('SELECT entry_id FROM events WHERE id=?',original)[0][0]
    # A retained confirmed v4-style delivery, loaded with the owner physically stopped.
    receipt=str(uuid.uuid4()); failed_receipt=str(uuid.uuid4())
    with closing(sqlite3.connect(svc.data/'core.sqlite')) as db:
        db.execute('PRAGMA foreign_keys=ON')
        db.execute("INSERT INTO operations(namespace,operation_id,action,target,payload,state,result) VALUES(?,?,'external','send','{}','failed','{}')",(execution,failed_receipt))
        db.execute("INSERT INTO events(source_scope,source_key,entry_id,account_id,bot_stage,observed_at,envelope,mentions) VALUES('fixture.control','gap',?,?,?,1,'{\"kind\":\"personal_action\"}','[]')",(group,bot['account'],bot['assignment']))
        db.execute("INSERT INTO operations(namespace,operation_id,action,target,payload,state,result) VALUES(?,?,'external','send','{}','confirmed','{}')",(execution,receipt))
        db.execute("INSERT INTO social_outbound(stage,entry,locator,namespace,child,original_actor,body,observed_at) VALUES(?,?,?,?,?,?,?,?)",(bot['assignment'],group,'700',execution,receipt,who,json.dumps({'message':[{'type':'text','data':{'text':'original receipt'}}]}),1767312300000))
        ids.append(db.execute('SELECT event FROM personal_deliveries ORDER BY outbound DESC LIMIT 1').fetchone()[0])
        slot=db.execute('SELECT id FROM history_items WHERE outbound=(SELECT max(id) FROM social_outbound)').fetchone()[0]
        ctx.check('canonical delivery ID differs from derivative history slot',ids[-1]!=slot)
        db.commit()
    def query(args, key=None, ok=True):
        return op('history','chat-query',execution,who,json.dumps(args),*(['--operation',key] if key else []),ok=ok)
    with svc.running():
        outputs=[]
        mapping={fixture['identities']['person']:who,fixture['identities']['group_conversation']:group,
                 fixture['identities']['receipt']['operation_id']:receipt}
        for case in fixture['cases']:
            request=json.loads(json.dumps(case['request']))
            for name in ('target',):
                for k,v in request.get(name,{}).items():request[name][k]=mapping.get(v,v)
            if 'person_id' in request:request['person_id']=who
            if request.get('event_id')==fixture['identities']['center']:request['event_id']=ids[2]
            result=query(request);old=case['result']
            ctx.equal(case['name']+' original event content', [r['content'] for r in result['events']], [r['content'] for r in old['events']])
            ctx.equal(case['name']+' original neighbour content',[r['content'] for r in result['context']],[r['content'] for r in old['context']])
            for key in ('source','coverage','returned_count','order','has_more','start_at','end_at','before','after','center_status'):
                ctx.equal(case['name']+' '+key,result.get(key),old.get(key))
            ctx.equal(case['name']+' exact public result keys',set(result),set(old))
            for new,prior in zip(result['events'],old['events'],strict=True):
                ctx.equal(case['name']+' original event fields',set(new),set(prior))
                for key in ('event_kind','author_kind','direction','asr'):
                    ctx.equal(case['name']+' event '+key,new[key],prior[key])
                ctx.equal(case['name']+' canonical event ID',new['id'],ids[prior['id']-1])
                ctx.equal(case['name']+' Client and reply event ID',new['event_id'],new['id'])
            ctx.equal(case['name']+' literal fields',list(result['matched_fields'].values()),list(old['matched_fields'].values()))
            outputs.append({'name':case['name'],'request':request,'result':result})
        failed=query({'target':{'operation_id':failed_receipt}})
        ctx.equal('failed send invents no original event anchor',failed['receipt_anchor'],{'event_id':None,'conversation_id':None,'status':'unavailable'})
        ctx.equal('failed send retains original receipt status',failed['receipt_status'],'failed')
        query({'target':{'operation_id':str(uuid.uuid4())}},ok=False)
        key=str(uuid.uuid4());request={'view':'search','keyword':'electric','context_before':1,'context_after':1}
        saved=query(request,key)
        event('later','electric later',5)
        ctx.equal('replay retains original result after later input',query(request,key),saved)
        query({'view':'search','keyword':'other'},key,ok=False)
        op('privacy','erase','erase-history-center',who,json.dumps([ids[2]]))
        missing=query({'view':'around','event_id':ids[2],'before':1,'after':1})
        ctx.equal('erased canonical center unavailable without refilling neighbours',missing['events'],[])
        replay=query(request,key)
        ctx.check('original replay exposes no erased body','雷達站 electric' not in json.dumps(replay,ensure_ascii=False))
        op('person','set-enabled',who,'false')
        query({},ok=False)
        op('person','set-enabled',who,'true')
    with closing(sqlite3.connect(svc.data/'core.sqlite')) as db:
        db.execute('PRAGMA foreign_keys=ON')
        for index in range(230):
            db.execute("INSERT INTO events(source_scope,source_key,entry_id,account_id,raw_actor,actor_stage,bot_stage,observed_at,occurred_at,envelope,mentions) VALUES('fixture.bulk',?,?,?,?,?,?,1,1,?,'[]')",(str(index),group,human['account'],who,human['assignment'],bot['assignment'],json.dumps({'kind':'message','segments':[{'type':'text','text':'bulk '+str(index)}]})))
        db.commit()
    with svc.running():
        first_page=query({'view':'search','keyword':'bulk','limit':201,'order':'oldest'})
        ctx.equal('positive limit has no hidden 200 item cap',len(first_page['events']),201)
        cursor=first_page['next_cursor']
        op('privacy','erase','erase-page-cursor',who,json.dumps([cursor]))
        second_page=query({'view':'search','keyword':'bulk','limit':201,'order':'oldest','cursor':cursor})
        ctx.equal('deleted canonical cursor continues without refill or offset loss',len(second_page['events']),29)
        ctx.equal('last page is exhausted',second_page['has_more'],False)
        ctx.check('numeric page survives deletion without duplicates',not {e['id'] for e in first_page['events']}&{e['id'] for e in second_page['events']})
    from tests.harness.compact import Owner, reply, call
    from tests.peers.chat.recorder import Recorder
    with Recorder() as http:
        live=Owner(ctx,'history-live',20);live.config(http)
        with live.svc.running():
            source=live.human('original live history content')
            original,answer=live.wire(http)
            declaration=next(t['function'] for t in original['tools'] if t['function']['name']=='query_chat_history')
            ctx.equal('Main publishes the exact original history declaration',declaration,fixture['declaration'])
            reply(answer,calls=[call('history-original','query_chat_history',{'view':'recent'})])
            continued,answer=live.wire(http)
            pairs=[r for r in continued['messages'] if r['role']=='tool']
            material=json.loads(pairs[-1]['content'])['result']
            ctx.equal('physical paid continuation retains original provider call ID',pairs[-1]['tool_call_id'],'history-original')
            ctx.equal('Main pairs canonical original event identity',material['events'][0]['event_id'],source)
            ctx.equal('Main pairs original source material',material['events'][0]['content'],'original live history content')
            ctx.check('history read registers actual publication dependencies',bool(live.rows('SELECT event FROM main_dependencies WHERE event=?',source)))
            reply(answer)
            live.until(lambda:live.rows("SELECT id FROM executions WHERE source=? AND state='completed'",source))
    built=subprocess.run([sys.executable,str(ctx.root/'ci/publish-contract.py'),'build','--out',str(ctx.output/'sdk-distributions'),'--verify-work',str(ctx.working/'history-sdk')],capture_output=True,text=True,timeout=120)
    ctx.write('history-sdk-build.json',{'exit_code':built.returncode,'stderr':built.stderr})
    ctx.equal('history candidate contains independently verified SDK packages',built.returncode,0)
    external=external_history(ctx,ctx.working/'history-sdk')
    binary=ctx.root/'target/debug'/('yuki-core.exe' if os.name=='nt' else 'yuki-core')
    return {'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
            'host_idl_sha256':hashlib.sha256((ctx.root/'sdk/rust/src/host-preview.json').read_bytes()).hexdigest(),
            'history_declaration_sha256':hashlib.sha256((ctx.root/'fixtures/history/v4-query-declaration.json').read_bytes()).hexdigest(),
            'database_schema_revision':json.loads((ctx.output/'sqlite-build.json').read_text())['schema_version'],
            'oracle_sha256':hashlib.sha256((ctx.root/'fixtures/history/v4-original-query.json').read_bytes()).hexdigest(),
            'cases':outputs,'canonical_ids':ids,'deleted_center':missing,'replay_after_erase':replay,
            'large_pages':[len(first_page['events']),len(second_page['events'])],
            'main_continuation':continued,'external_publication':external,'platform_backfill_calls':0}


def external_history(ctx, sdk):
    from tests.harness.compact import Owner
    def load(card):
        spec=importlib.util.spec_from_file_location(card,ctx.root/'tests/cases'/f'{card}.py')
        module=importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module
    notification,aux=load('CORE-058'),load('CORE-050')
    owner=Owner(ctx,'history-external',34)
    space=json.dumps(['qq','compact','700'],separators=(',',':'))
    with owner.svc.running():
        source=owner.human('original history request')
        execution=owner.op('ingress','claim',source)['execution']
        entry=owner.rows('SELECT entry_id FROM events WHERE id=?',source)[0][0]
    folder=ctx.working/'history-news-peer'
    notification.installed(ctx,owner,sdk,folder,'history-news',{entry:owner.bot['assignment']})
    peer=aux.Peer(owner,folder)
    with owner.svc.running():
        owner.until((folder/'ready.json').exists)
        published=peer.call('notification.publish',notification.request(entry,space,'original-news',text='',target={'target_type':'private','target_id':owner.actor,'conversation_id':entry}))
        def query(args,key=None):
            return owner.op('history','chat-query',execution,owner.actor,json.dumps(args),*(['--operation',key] if key else []))
        request={'view':'search','event_kind':'external_event','keyword':'Original update'}
        operation=str(uuid.uuid4())
        result=query(request,operation)
        ctx.equal('original external query returns actual publication ID',[r['event_id'] for r in result['events']],[published['source_event_id']])
        row=result['events'][0]
        ctx.equal('external query uses original summary rather than delivery text',row['content'],'Original update')
        ctx.equal('external query retains original publisher and kind',[row[k] for k in ('event_kind','source_plugin_id','external_source','external_event_type','author_kind','origin','direction')],['external_event','history-news','feed-fixture','subscription_update','system','plugin_background','external'])
        ctx.equal('external publisher never becomes a human',[row['person_id'],row['presence_id']],[None,None])
        ctx.equal('default message query excludes external publication',[r['event_id'] for r in query({})['events']],[source])
        owner.op('privacy','forget','forget-original-notification',owner.actor)
        replay=owner.svc.command('operator','history','chat-query',execution,owner.actor,json.dumps(request),'--operation',operation,accepted=False)
        ctx.equal('erased external body is removed from saved original row',owner.rows('SELECT body FROM history_public_rows WHERE execution=? AND operation=?',execution,operation),[(None,)])
        ctx.equal('offline original query starts no model or delivery',owner.rows('SELECT (SELECT count(*) FROM model_requests),(SELECT count(*) FROM social_outbound)'),[(0,0)])
        return {'publication':published,'original':result,'after_erasure':replay}
