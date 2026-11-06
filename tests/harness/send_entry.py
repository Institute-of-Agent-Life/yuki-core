"""Drive native Main/QQ through the canonical directory contract recorded from V4."""
import hashlib
import json
import os
import select
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer


def run(ctx):
    owner = Owner(ctx, 'canonical-send', 42)
    fixture = json.loads((ctx.root/'fixtures/social/v4-send-target.json').read_bytes())
    group = json.dumps(['qq','compact','700'], separators=(',',':'))
    results = []
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
                    ctx.equal('paired send receipt for '+key,observed['state'],'confirmed' if action else 'failed')
                    if not action:
                        ctx.equal('refused selection has no physical send',select.select([qq.socket],[],[],0)[0],[])
                    results.append({'call':key,'target':target,'state':observed['state'],'action':action})
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
            finally:
                qq.close()
    return {'source_commit':fixture['source_commit'],'oracle_sha256':hashlib.sha256((ctx.root/'fixtures/social/v4-send-target.json').read_bytes()).hexdigest(),'physical_calls':4,'zero_dispatch_refusals':5,'cases':results}
