"""Original recall is a real Main tool, bound to the original QQ sender and ledger."""
import json
import hashlib
import os
import sqlite3
import select
from contextlib import closing
from tests.harness.compact import Owner,call,reply
from tests.harness.crash import kill_writer
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer as QQ


def run(ctx):
    o=Owner(ctx,'model-recall',42)
    group=json.dumps(['qq','compact','700'],separators=(',',':'))
    with o.svc.running(): alternate=o.op('account','open','qq','compact','101',1,'--start',1)
    with Recorder() as model:
        o.config(model)
        cfg=o.svc.data/'yuki.toml'
        text='\n'.join(line for line in cfg.read_text().splitlines() if not line.startswith('binding_endpoint='))
        text=text.replace('root_slots=8','root_slots=24')
        cfg.write_text(text+f'''\n[qq]\nnamespace="compact"\ndesignated_ingress="100"\n[qq.groups]\n"700"="open"\n[[qq.bots]]\nself_id="100"\nstage={json.dumps(o.bot['assignment'])}\ntoken_env="RECALL_QQ"\n[[qq.bots]]\nself_id="101"\nstage={json.dumps(alternate['assignment'])}\ntoken_env="RECALL_QQ_OTHER"\n''')
        env={**os.environ,'RECALL_QQ':'original-recall','RECALL_QQ_OTHER':'alternate-recall'}
        def connect(bot='100'):
            peer=QQ(o.svc.address,bot,'original-recall' if bot=='100' else 'alternate-recall');o.until(lambda:o.op('qq','status',bot)['online']);return peer
        def receipt(wire,key):return json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')==key))
        sent=[]
        with o.svc.running(environment=env):
            peer=connect()
            try:
                source=o.human('Send original messages for recall',group=group)
                _,q=o.wire(model)
                for i in range(4):
                    key=f'original-send-{i}';reply(q,calls=[call(key,'send',{'text':f'original message {i}'})])
                    physical=peer.receive();ctx.equal('original send reaches the native group',physical['action'],'send_group_msg')
                    peer.send({'status':'ok','retcode':0,'data':{'message_id':4000+i},'echo':physical['echo']})
                    wire,q=o.wire(model);ctx.equal('native send has a confirmed receipt',receipt(wire,key)['state'],'confirmed')
                    event=o.rows('SELECT d.event FROM personal_deliveries d JOIN social_outbound b ON b.id=d.outbound WHERE b.locator=?',str(4000+i))[0][0]
                    sent.append(event)
                reply(q);o.finished(source)
            finally:peer.close()
        # The original oracle changes this same persisted route with the owner stopped.
        with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:
            db.execute("INSERT INTO recovered_groups(namespace,native_group,stage,enabled,revision) VALUES('compact','700',?,0,1)",(alternate['assignment'],));db.commit()
        cfg.write_text(cfg.read_text().replace('"700"="open"','"700"="paused"'))
        observations=[]
        with o.svc.running(environment=env):
            peer=connect();other=connect('101')
            try:
                current=o.human('Recall the original messages from my authorized group')
                wire,q=o.wire(model)
                fixture=json.loads((ctx.root/'fixtures/social/v4-recall.json').read_text(encoding='utf-8'))
                ctx.equal('Main preserves the original recall declaration',next(t['function'] for t in wire['tools'] if t['function']['name']=='recall_own_message'),fixture['declarations'][0])
                def invoke(key,event,physical=False,response='ok'):
                    nonlocal q
                    reply(q,calls=[call(key,'recall_own_message',{'event_id':event})])
                    packet=None
                    if physical:
                        packet=peer.receive();ctx.equal('recall uses original native action',packet['action'],'delete_msg')
                        ctx.equal('recall preserves original locator',packet['params'],{'message_id':4000+sent.index(event)})
                        peer.send({'status':response,'retcode':0 if response=='ok' else 1,'data':{},'echo':packet['echo']})
                    wire,q=o.wire(model);result=receipt(wire,key);observations.append({'event':event,'wire':packet,'receipt':result});return result,packet
                result,_=invoke('confirmed-recall',sent[0],True)
                ctx.equal('paused and rerouted group recalls through the original sender',result['state'],'confirmed')
                ctx.equal('recall reports original Presence',result['result']['presence_id'],o.bot['assignment'])
                expected=dict(fixture['original_results'][0]['result'])
                expected.update(operation_id=result['id'],presence_id=o.bot['assignment'],target={'kind':'space','id':o.rows('SELECT id FROM contact_spaces WHERE space_key=?',group)[0][0]})
                ctx.equal('confirmed recall preserves every original result field',result['result'],expected)
                ctx.equal('confirmed native recall marks the original outbound',o.rows('SELECT recalled FROM social_outbound WHERE locator=?','4000'),[(1,)])
                result,_=invoke('retained-recall',sent[0]);ctx.equal('second call consumes confirmed receipt without a new packet',result['state'],'confirmed')
                result,_=invoke('unknown-recall',sent[1],True,'unrecognized')
                ctx.equal('unrecognized original receipt stays unknown',result['state'],'unknown')
                result,_=invoke('unknown-replay',sent[1]);ctx.equal('another model call cannot retry unknown recall',result['state'],'unknown')
                result,late=invoke('late-recall',sent[2],True,'unrecognized')
                ctx.equal('late recall first records unknown',result['state'],'unknown')
                peer.send({'status':'ok','retcode':0,'data':{},'echo':late['echo']})
                o.until(lambda:o.rows("SELECT o.operation_id FROM operations o JOIN model_tool_intents i ON i.execution=o.namespace AND i.operation_id=o.operation_id WHERE i.call_id='late-recall' AND o.state='confirmed'"))
                ctx.equal('late callback settles the same native part',o.rows('SELECT recalled FROM social_outbound WHERE locator=?','4002'),[(1,)])
                result,_=invoke('not-own-message',current);ctx.equal('human inbound cannot be recalled',result['state'],'failed')
                reply(q);o.finished(current)
                ctx.equal('each original part acquires at most one physical recall',o.rows('SELECT count(*) FROM social_recalls'),[(3,)])
                ctx.equal('replacement bot received no recall',select.select([other.socket],[],[],0)[0],[])
            finally:peer.close();other.close()
        with o.svc.running(environment=env):
            event=o.human('Read original unknown recall after restart')
            _,q=o.wire(model);reply(q,calls=[call('cold-unknown','recall_own_message',{'event_id':sent[1]})])
            wire,q=o.wire(model);cold=receipt(wire,'cold-unknown')
            ctx.equal('cold offline replay retains unknown',cold['state'],'unknown')
            ctx.equal('cold replay acquires no new physical intent',o.rows('SELECT count(*) FROM social_recalls'),[(3,)])
            reply(q);o.finished(event)
            other=connect('101')
            try:
                offline_event=o.human('Original sender is offline; replacement must not recall')
                _,q=o.wire(model);reply(q,calls=[call('offline-original','recall_own_message',{'event_id':sent[3]})])
                wire,q=o.wire(model);offline=receipt(wire,'offline-original')
                ctx.equal('offline original rejects even when replacement is online',offline['state'],'failed')
                ctx.equal('offline original acquires no recall',o.rows('SELECT count(*) FROM social_recalls'),[(3,)])
                ctx.equal('replacement socket stays empty',select.select([other.socket],[],[],0)[0],[])
                reply(q);o.finished(offline_event)
            finally:other.close()
            crash_exec=o.rows('SELECT id FROM executions WHERE source=?',offline_event)[0][0]
            o.op('effect','intent',crash_exec,'crash-recall',o.actor,'external','recall_own_message',json.dumps({'event_id':sent[3]}))
        kill_writer(ctx,o.svc.data,'model_recall_crash_child','after',{'YUKI_MODEL_RECALL_TEST':json.dumps([crash_exec,o.actor,'crash-recall',o.bot['assignment']])})
        ctx.equal('crash preserves committed original recall claim',o.rows("SELECT state FROM operations WHERE namespace=? AND operation_id='crash-recall'",crash_exec),[('dispatched',)])
        with o.svc.running(environment=env):
            ctx.equal('cold owner settles a committed claim as unknown',o.rows("SELECT state FROM operations WHERE namespace=? AND operation_id='crash-recall'",crash_exec),[('unknown',)])
            crash_event=o.human('Never retry the committed original recall after crash')
            _,q=o.wire(model);reply(q,calls=[call('after-crash','recall_own_message',{'event_id':sent[3]})])
            wire,q=o.wire(model);crashed=receipt(wire,'after-crash')
            ctx.equal('new model call preserves original crash uncertainty',crashed['state'],'unknown')
            ctx.equal('restart and new operation preserve four original keys',o.rows('SELECT count(*) FROM social_recalls'),[(4,)])
            reply(q);o.finished(crash_event)
            o.op('privacy','erase','erase-original-recalled-source',o.actor,json.dumps([source]))
            event=o.human('Check erased original event')
            _,q=o.wire(model);reply(q,calls=[call('erased-recall','recall_own_message',{'event_id':sent[2]})])
            wire,q=o.wire(model);erased=receipt(wire,'erased-recall')
            ctx.equal('erased original cannot be dispatched again',erased['state'],'failed')
            ctx.equal('erasure retains minimal original recall keys',o.rows('SELECT count(*) FROM social_recalls'),[(4,)])
            reply(q);o.finished(event)
    return {'original_events':sent,'calls':observations,'cold':cold,'offline':offline,'crash':crashed,'erased':erased,'original_sender':o.bot['assignment'],'physical_recalls':3,'committed_recall_claims':4,'recall_oracle_sha256':hashlib.sha256((ctx.root/'fixtures/social/v4-recall.json').read_bytes()).hexdigest(),'source_commit':'329ca7c487414d2f54ddd98ff4727446ed2446cd'}
