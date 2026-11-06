"""The selected codec uses the same operation/attempts as the model's original attachment read."""
import base64
import hashlib
import json
import os
import sqlite3
from contextlib import closing
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder


def run(ctx,sdk,installed,ready,png):
    observations={}
    for case,kind,visual in [('video','video',True),('pdf','file',False),('text','video',False),('late','video',True),('switch','video',True)]:
        o=Owner(ctx,'sel-'+case,49)
        folders={key:ctx.working/(key+case) for key in ['codec-a','codec-b']}
        for path in folders.values():path.mkdir()
        peer=folders['codec-a']
        frames=[png(px) for px in (b'\xfe\x01\x02',b'\x03\xfc\x04')]
        for i,frame in enumerate(frames):(peer/f'image-{i}.png').write_bytes(frame)
        raw=b'%PDF-1.4\n%private page\n' if kind=='file' else b'\0\0\0\x18ftypmp42\0unseen video packet'
        (o.svc.data/'media-inputs'/'media.bin').write_bytes(raw)
        def edit(stmt,*args):
            with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:db.execute(stmt,args);db.commit()
        with Recorder() as model:
            installed(ctx,o,folders,sdk,model,visual,processor='codec-a')
            with o.svc.running(environment={**os.environ,'YUKI_BROKER_QQ':'synthetic-media-receiver'}):
                ready(o,folders)
                inbound={'source':case,'account':o.user['account'],'bot_stage':o.bot['assignment'],'reply_bot':'100','observed':1,'occurred':1,'kind':'message','segments':[{'type':'text','text':'inspect selected attachment'},{'type':'media','kind':kind,'reference':'original-key'}]}
                eid=o.op('ingress','record',json.dumps(inbound),'--accept')['event']
                initial,q=o.wire(model)
                exec_id=o.rows('SELECT id FROM executions WHERE source=? ORDER BY rowid LIMIT 1',eid)[0][0]
                ctx.equal('paid original selected dispatch freezes configured installation',o.rows('SELECT plugin,length(deployment)>0 FROM main_processor_bindings WHERE execution=?',exec_id),[('codec-a',1)])
                cached=o.op('media','cache-file',exec_id,o.actor,eid,0,o.bot['assignment'],'original-key','media.bin')
                ctx.equal('selected input is actual original file',cached['digest'],hashlib.sha256(raw).hexdigest())
                ctx.check('initial Main has no frame or decoded PDF',all(base64.b64encode(img).decode() not in str(initial) for img in frames) and 'controlled selected PDF page' not in str(initial))
                if case=='switch':
                    edit("UPDATE core_meta SET value=json_set(value,'$.deployment','other-deployment') WHERE key='plugin-approval:codec-a'")
                if case=='late':(peer/'hold-selected').touch()
                args={'event_id':eid,'attachment_index':0,'question':'show original details'}
                reply(q,calls=[call('read-original','inspect_conversation_attachment',args)])
                if case=='late':
                    o.until(lambda:(peer/'selected-ready.json').exists())
                    edit("UPDATE media_sources SET expires=0 WHERE event=? AND slot=0",eid)
                    (peer/'hold-selected').unlink()
                body,q=o.wire(model)
                imgs=[b['image_url']['url'] for m in body['messages'] if isinstance(m.get('content'),list) for b in m['content'] if b.get('type')=='image_url']
                expected=['data:image/png;base64,'+base64.b64encode(f).decode() for f in frames] if case=='video' else []
                ctx.equal('same Main receives only authorized ordered original frames '+case,imgs,expected)
                called=(peer/'selected-ready.json').exists()
                ctx.equal('physical decoder ran only for selected eligible source '+case,called,case in ('video','late','pdf'))
                ctx.check('unselected installed peer has zero decoder calls',not (folders['codec-b']/'selected-ready.json').exists())
                receipt=next(m['content'] for m in body['messages'] if m.get('tool_call_id')=='read-original')
                if case=='pdf':ctx.check('original PDF text reaches same model','controlled selected PDF page' in receipt)
                if case=='late':ctx.check('expired selected handle hides returned private text','controlled selected frames' not in receipt and 'unavailable' in receipt)
                if case=='video':ctx.equal('foreign image remains unavailable',o.rows('SELECT status FROM host_media_presentations ORDER BY ordinal'),[('ready',),('ready',),('unavailable',)])
                if called:
                    proof=json.loads((peer/'selected-ready.json').read_bytes())
                    ctx.equal('same original logical tool operation and source',o.rows('SELECT o.operation_id,o.target,h.method,p.event,p.slot FROM operations o JOIN host_calls h ON (o.namespace,o.operation_id)=(h.execution,h.operation) JOIN main_processor_calls p USING(handle) WHERE o.namespace=?',exec_id),[(proof['operation'],'inspect_conversation_attachment','media.process',eid,0)])
                ctx.equal('one initial model call and one normal continuation, no extra vision request',o.rows('SELECT count(*) FROM model_attempts'),[(2,)])
                reply(q,'');o.finished(eid)
                observations[case]={'physical_processor_calls':len(o.rows("SELECT handle FROM host_calls WHERE method='media.process'")),'original_event':eid,'images':[hashlib.sha256(base64.b64decode(u.partition(',')[2])).hexdigest() for u in imgs],'model_attempts':2}
    oracle=(ctx.root/'fixtures/media/main-selected-v4.json').read_bytes()
    proof={'cases':observations,'original_oracle_sha256':hashlib.sha256(oracle).hexdigest(),'tool':'inspect_conversation_attachment'}
    ctx.write('main-selected-media.json',proof)
    return proof
