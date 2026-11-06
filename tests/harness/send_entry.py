"""Drive native Main/QQ through the canonical directory contract recorded from V4."""
import hashlib
import json
import os
import select
import sqlite3
from contextlib import closing
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer


def recall_after_reserve(owner, execution, original_event):
    # Interleave the withdrawal in the real reservation commit (no synthetic
    # second reservation while a physical request is awaiting its response).
    with closing(sqlite3.connect(owner.svc.data/'core.sqlite')) as lost:
        lost.execute("CREATE TABLE send_receipt_fault(execution TEXT, original_event INTEGER, reserved_logical TEXT)")
        lost.execute("INSERT INTO send_receipt_fault VALUES(?,?,NULL)",(execution,original_event))
        lost.executescript("""
        CREATE TRIGGER test_recall_between_reserve_and_dispatch AFTER INSERT ON model_requests
        WHEN NEW.activation=(SELECT 'main:'||execution FROM send_receipt_fault)
            AND (SELECT reserved_logical FROM send_receipt_fault) IS NULL
        BEGIN
          UPDATE social_outbound SET recalled=1,body='{}' WHERE id=(
              SELECT outbound FROM personal_deliveries WHERE event=(SELECT original_event FROM send_receipt_fault));
          UPDATE send_receipt_fault SET reserved_logical=NEW.id;
        END;
        """)
        lost.commit()


def run(ctx):
    owner = Owner(ctx, 'canonical-send', 42)
    fixture = json.loads((ctx.root/'fixtures/social/v4-send-target.json').read_bytes())
    group = json.dumps(['qq','compact','700'], separators=(',',':'))
    results = []
    proof = {}
    with Recorder() as model:
        owner.config(model)
        cfg = owner.svc.data/'yuki.toml'
        cfg.write_text('\n'.join(l for l in cfg.read_text().splitlines() if not l.startswith('binding_endpoint=')).replace('root_slots=8','root_slots=32') +
            f'\n[qq]\nnamespace="compact"\ndesignated_ingress="100"\n[qq.groups]\n"700"="open"\n"701"="open"\n[[qq.bots]]\nself_id="100"\nstage={json.dumps(owner.bot["assignment"])}\ntoken_env="SEND_ENTRY_QQ"\n')
        with owner.svc.running(environment={**os.environ,'SEND_ENTRY_QQ':'canonical-fixture'}):
            qq = Peer(owner.svc.address,'100','canonical-fixture')
            try:
                owner.until(lambda:owner.op('qq','status','100')['online'])
                private_event = owner.human('Known private contact'); _,answer = owner.wire(model)
                reply(answer); owner.finished(private_event)
                private = owner.rows('SELECT entry_id FROM events WHERE id=?',private_event)[0][0]
                group2 = json.dumps(['qq','compact','701'],separators=(',',':'))
                alt_event = owner.human('Original observation in the second group',group=group2)
                _,answer = owner.wire(model);reply(answer);owner.finished(alt_event)
                alt_entry = owner.rows('SELECT entry_id FROM events WHERE id=?',alt_event)[0][0]
                alt_space = owner.rows('SELECT id FROM contact_spaces WHERE space_key=?',group2)[0][0]
                event = owner.human('Use the original directory and send',group=group)
                wire,answer = owner.wire(model)
                public = owner.rows('SELECT entry_id FROM events WHERE id=?',event)[0][0]
                space = owner.rows('SELECT id FROM contact_spaces WHERE space_key=?',group)[0][0]
                unknown = 'ffffffff-ffff-4fff-8fff-ffffffffffff'
                actual = next(t['function']['parameters']['properties']['target'] for t in wire['tools'] if t['function']['name']=='send')
                ctx.equal('Main exposes the original canonical target declaration',actual,fixture['target_schema'])
                def invoke(key, target, action=None):
                    nonlocal answer
                    reply(answer,calls=[call(key,'send',{'text':key,'target':target})])
                    if action:
                        packet=qq.receive()
                        ctx.equal('native action for '+key,packet['action'],action)
                        dest='group_id' if action=='send_group_msg' else 'user_id'
                        ctx.equal('actual recipient for '+key,packet['params'][dest],(701 if key=='observed-group' else 700) if dest=='group_id' else 200)
                        qq.send({'status':'ok','retcode':0,'data':{'message_id':8600+len(results)},'echo':packet['echo']})
                    wire,answer=owner.wire(model)
                    observed=json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')==key))
                    if action:
                        data=observed['data']
                        expected=owner.rows("SELECT o.operation_id,d.event,p.entry,p.stage FROM model_tool_intents i JOIN operations o ON o.namespace=i.execution AND o.operation_id=i.operation_id JOIN social_plans p ON p.namespace=o.namespace AND p.operation_id=o.operation_id JOIN social_parts t ON t.namespace=p.namespace AND t.operation_id=p.operation_id JOIN social_outbound a ON a.namespace=t.namespace AND a.child=t.child JOIN personal_deliveries d ON d.outbound=a.id WHERE i.call_id=?", key)
                        ctx.equal('original model-visible ACK and internal event for '+key,(data['operation_id'],data['original_event_id'],data['original_conversation_id'],data['presence_id']),expected[0])
                        ctx.equal('actual delivered text, target and success on '+key,
                                  [observed['ok'],observed['mutation_committed'],observed['retryable'],observed['tool_name'],observed['provider_id'],data['status'],data['action'],data['event_id'],data['target'],data['delivered_text_available'],data['delivered_text']],
                                  [True,True,False,'send','core','succeeded','send_message',data['original_event_id'],{'kind':target['kind'],'id':target['target_id']},True,key])
                        proof[key]=data
                        state='confirmed'
                    else:
                        state=observed['state']
                        ctx.equal('paired refusal for '+key,state,'failed')
                        ctx.equal('refused selection has no physical send',select.select([qq.socket],[],[],0)[0],[])
                    results.append({'call':key,'target':target,'state':state,'action':action})
                invoke('canonical-group',{'kind':'space','target_id':space,'conversation_id':public,'binding_id':unknown},'send_group_msg')
                invoke('observed-private',{'kind':'person','target_id':owner.actor,'conversation_id':private,'binding_id':unknown},'send_private_msg')
                invoke('observed-binding',{'kind':'person','target_id':owner.actor,'binding_id':owner.user['assignment']},'send_private_msg')
                invoke('observed-group',{'kind':'space','target_id':alt_space,'conversation_id':alt_entry},'send_group_msg')
                invoke('mismatched-person',{'kind':'person','target_id':unknown,'conversation_id':private})
                invoke('mismatched-space',{'kind':'space','target_id':space,'conversation_id':private})
                invoke('wrong-binding',{'kind':'person','target_id':owner.actor,'binding_id':unknown})
                reply(answer);owner.finished(event)
                ctx.equal('receipt rows freeze physical destinations and original Main actor',owner.rows("SELECT entry,original_actor FROM social_outbound WHERE locator IN ('8600','8601','8602','8603') ORDER BY locator"),[(public,owner.actor),(private,owner.actor),(private,owner.actor),(alt_entry,owner.actor)])
                ctx.equal('destination authority is retained for erasure',owner.rows("SELECT count(*) FROM main_dependencies d JOIN executions x ON x.id=d.execution WHERE x.source=? AND d.event=?", event, private_event),[(1,)])
                parent=owner.rows("SELECT id FROM executions WHERE source=?",event)[0][0]
                owner.op('main','register','work','delegated-send',public,'--parent',parent)
                delegated=owner.op('main','trigger','delegated-send','bound',json.dumps({'text':'Exercise the original Work scope.'}))['execution']
                _,answer=owner.wire(model)
                invoke('work-cannot-expand-private',{'kind':'person','target_id':owner.actor,'conversation_id':private})
                invoke('work-cannot-expand-group',{'kind':'space','target_id':alt_space,'conversation_id':alt_entry})
                reply(answer);owner.until(lambda: owner.rows("SELECT id FROM executions WHERE id=? AND state='completed'",delegated))
                ctx.equal('delegated Work acquired no physical parts',owner.rows('SELECT count(*) FROM social_parts WHERE namespace=?',delegated),[(0,)])
                # After the provider has read a delivered line, a later paid request would
                # copy that old tool output. A withdrawn line must stop before dispatch.
                trigger=owner.human('Check an original text send after recall',group=group)
                _,answer=owner.wire(model)
                invoke('original-before-recall',{'kind':'space','target_id':space},'send_group_msg')
                invoke('later-distinct-send',{'kind':'space','target_id':space},'send_group_msg')
                captured=proof['original-before-recall'];later=proof['later-distinct-send']
                execution=owner.rows('SELECT id FROM executions WHERE source=?',trigger)[0][0]
                attempts=owner.rows('SELECT count(*) FROM model_attempts')[0][0]
                recall_after_reserve(owner,execution,captured['original_event_id'])
                reply(answer,calls=[call('after-privacy-change','time_get_current',{})])
                owner.until(lambda: owner.rows("SELECT diagnostic FROM main_turns JOIN executions x ON x.id=execution WHERE x.id=? AND x.state='held'", execution)==[('send_receipt_source_changed',)])
                original_logical=owner.rows("SELECT reserved_logical FROM send_receipt_fault")[0][0]
                ctx.check('normal admission reserved the logical request before the source was withdrawn',bool(original_logical))
                ctx.equal('no physical attempt can be acquired by that reservation',owner.rows("SELECT count(*) FROM model_attempts WHERE logical=?",original_logical),[(0,)])
                ctx.equal('old confirmed text cannot leave in a copied Chat body',owner.rows('SELECT count(*) FROM model_attempts'),[(attempts,)])
                ctx.equal('later ACK is still attached only to its own content',owner.rows("SELECT json_extract(body,'$.message[0].data.text') FROM personal_live_deliveries WHERE event=?",later['original_event_id']),[('later-distinct-send',)])
                ctx.equal('a held invocation did not redispatch the QQ send',select.select([qq.socket],[],[],0)[0],[])
            finally:
                qq.close()
    return {'source_commit':fixture['source_commit'],'oracle_sha256':hashlib.sha256((ctx.root/'fixtures/social/v4-send-target.json').read_bytes()).hexdigest(),'physical_calls':6,'model_send_receipt':{'schema':'v4-confirmed-single-text','original_anchor':captured['original_event_id'],'later_anchor':later['original_event_id'],'withdrawn_source_held':True,'reserved_dispatch_refused':True,'oracle_sha256':hashlib.sha256((ctx.root/'fixtures/social/v4-model-send-receipt.json').read_bytes()).hexdigest()},'zero_dispatch_refusals':5,'cases':results}
