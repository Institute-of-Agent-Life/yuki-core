"""Client9 resolves selected scopes using an actual device, writer, and existing ACP threads."""
import hashlib,json,sqlite3,uuid
from contextlib import closing
from tests.harness.compact import Owner

def run(ctx,wire,sdk,Plugin):
    o=Owner(ctx,'entry-selection',46);b=str(uuid.uuid4());c=str(uuid.uuid4());key=lambda:str(uuid.uuid4())
    fixture=ctx.root/'fixtures/personal/entry-selection.json';vectors={v['name']:v['result'] for v in json.loads(fixture.read_bytes())['scenarios']};matched=[]
    def http(path,device=None,body=None,headers=None,expected=200):
        status,doc=wire(o.svc.address,path,device and device['token'],body,selected=headers)
        ctx.equal('selected Client9 '+path+' HTTP status',status,expected)
        return doc.get('data',doc)
    def pair(person):
        code=o.op('personal','issue',person,key(),'chat')['code'];return http('/client/v1/pair',body={'code':code,'device_label':'selection fixture'})
    def counts():return o.rows('SELECT (SELECT count(*) FROM events),(SELECT count(*) FROM personal_receipts),(SELECT count(*) FROM personal_codes),(SELECT count(*) FROM model_requests)')[0]
    def edit(sql,*args):
        with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:db.execute(sql,args);db.commit()
    with o.svc.running():
        for person in [b,c]:o.op('person','create',person)
        da=pair(o.actor);db=pair(b);dc=pair(c)
        ma=http('/client/v1/me',da);mb=http('/client/v1/me',db);mc=http('/client/v1/me',dc)
        acp=o.op('account','open','acp','personal','client-b',1,'--person',b,'--start',1)['assignment'];bot=o.op('account','open','acp','personal','yuki-acp',1,'--start',1)['assignment']
        threads=[key(),key()];sessions=[o.op('entry','private',acp,bot,'--topic',t)['entry'] for t in threads]
        labels={o.actor:'A',b:'B',ma['binding_id']:'A.app',mb['binding_id']:'B.app',ma['conversation_id']:'A.home',mb['conversation_id']:'B.home',acp:'B.acp',sessions[0]:'B.thread',threads[0]:'thread',None:None}
        bh={'X-Yuki-Entry':mb['binding_id']};ch={'X-Yuki-Conversation':sessions[0]};ch2={'X-Yuki-Conversation':sessions[1]}
        def original(name,headers=None):
            want=vectors[name];status=200 if 'error' not in want else {'unauthenticated':401,'not_found':404}[want['error']]
            value=http('/client/v1/me',da,headers=headers,expected=status)
            projection={'error':value['problem']['code']} if status!=200 else {k:(labels[value[k]] if k!='platform' else value[k]) for k in want}
            ctx.equal('fixed original selection '+name,projection,want);matched.append(name)
        original('default-before');original('foreign-app-before',bh);original('foreign-acp-before',ch)
        o.op('person','merge',o.actor,b)
        original('default-after');original('selected-app',bh);original('selected-acp',ch);original('matched-acp',{**ch,'X-Yuki-Entry':acp});original('mismatched-binding',{**ch,**bh});original('app-as-acp',{'X-Yuki-Conversation':mb['conversation_id']});original('acp-binding-alone',{'X-Yuki-Entry':acp});original('missing-binding',{'X-Yuki-Entry':'missing'});original('empty-binding',{'X-Yuki-Entry':''})
        op=key();body={'request_id':op,'text':'scoped App submission'}
        before=counts();denied_before=before
        for headers,status in [({'X-Yuki-Entry':mc['binding_id']},401),({'X-Yuki-Conversation':'missing'},404),({**bh,**ch},404)]:http('/client/v1/messages',da,body,headers,status)
        ctx.equal('explicit invalid writes never reach the writer effects',counts(),before)
        from http.client import HTTPConnection
        with closing(HTTPConnection(o.svc.address,timeout=8)) as conn:
            raw=json.dumps(body).encode();conn.putrequest('POST','/client/v1/messages')
            for k,v in [('Authorization','Bearer '+da['token']),('Content-Type','application/json'),('Content-Length',str(len(raw))),('X-Yuki-Entry',ma['binding_id']),('X-Yuki-Entry',mb['binding_id'])]:conn.putheader(k,v)
            conn.endheaders(raw);response=conn.getresponse();response.read()
            ctx.equal('duplicate selection headers rejected',response.status,422)
        ctx.equal('ambiguous HTTP headers produce no effects',counts(),before)
        denied_after=counts()
        accepted=http('/client/v1/messages',da,body,bh);event=accepted['event_id']
        selected_projection=http('/client/v1/me',da,headers=bh)
        example={k:selected_projection[k] for k in ('source_person_id','person_id','request_person_id','request_scope','binding_id','conversation_id','platform')}
        example.update({'event_id':event,'visible_via':['A selecting B','B default'],'invisible_via':['A default']})
        ctx.equal('writer uses selected original binding and request person',o.rows('SELECT entry_id,raw_actor,actor_stage FROM events WHERE id=?',event),[(mb['conversation_id'],b,mb['binding_id'])])
        ctx.equal('selected message is visible in its original namespace',http('/client/v1/requests/'+op,da,headers=bh)['event_id'],event)
        ctx.equal('default original namespace cannot see selected receipt',http('/client/v1/requests/'+op,da)['found'],False)
        ctx.check('other original device replays selection',http('/client/v1/messages',db,body)['replay'])
        http('/client/v1/messages',da,{**body,'text':'changed payload'},bh,409)
        for hdr,entry in [(ch,sessions[0]),(ch2,sessions[1])]:
            msg=http('/client/v1/messages',da,{**body,'text':entry},hdr);ctx.equal('ACP message stays on selected thread',msg['conversation_id'],entry)
            ctx.equal('ACP source is original binding and topic',o.rows('SELECT e.raw_actor,e.entry_id,v.topic FROM events e JOIN entries v ON v.id=e.entry_id WHERE e.id=?',msg['event_id']),[(b,entry,threads[sessions.index(entry)])])
            ctx.equal('ACP receipt is independent of same UUID in other threads',http('/client/v1/requests/'+op,db,headers=hdr)['event_id'],msg['event_id'])
        ctx.equal('one UUID produced exactly three valid isolated message effects',o.rows('SELECT count(*) FROM personal_receipts WHERE operation=?',op),[(3,)])
        # Reader 46 used the App entry id as scope. Retain that row and its original result.
        edit('UPDATE personal_receipts SET scope=? WHERE owner=? AND scope=? AND operation=?',mb['conversation_id'],b,b,op)
        ctx.check('reader 46 App receipt replays through explicit matching person',http('/client/v1/messages',da,body,bh)['replay'])
        scope_id=key();request={'request_id':scope_id,'purpose':'connector'}
        codes=[http('/client/v1/link-codes',da,request,h) for h in [None,bh,ch,ch2]]
        ctx.equal('same code UUID remains isolated by original person or ACP conversation',len({v['code_id'] for v in codes}),4)
        replay=http('/client/v1/link-codes',db,request);ctx.check('second device replays original B code without disclosing raw code',replay['code_id']==codes[1]['code_id'] and replay['code'] is None)
        http('/client/v1/link-codes',da,{**request,'purpose':'chat'},bh,409)
        ctx.equal('code binds selected original stage',o.rows('SELECT owner,source_stage FROM personal_codes WHERE id=?',codes[1]['code_id']),[(b,mb['binding_id'])])
        receipt=http('/client/v1/requests/'+scope_id,da,headers=bh);ctx.equal('code receipt is original, secret-free and committed',[receipt['operation_kind'],receipt['status'],receipt['target'],set(receipt['result'])],['client.link_code','succeeded',codes[1]['code_id'],{'code_id','purpose','expires_at'}])
        http('/client/v1/messages',db,{'request_id':scope_id,'text':'change operation class'},expected=409)
        http('/client/v1/link-codes',db,{'request_id':op,'purpose':'chat'},expected=409)
        old_op=key();old=http('/client/v1/link-codes',da,{'request_id':old_op,'purpose':'chat'})
        # Actual reader-46 shape: device issuer, raw UUID and no selected binding; migration does not invent it.
        edit('UPDATE personal_codes SET operation=?,source_stage=NULL WHERE id=?',old_op,old['code_id'])
        recovered={label:http('/client/v1/requests/'+old_op,da,headers=headers)['found'] for label,headers in [('original_source',None),('merged_root',bh),('acp',ch)]}
        ctx.equal('legacy default receipt isolation matches original NULL fallback',recovered,json.loads(fixture.read_bytes())['legacy_null_receipt'])
        legacy=http('/client/v1/link-codes',da,{'request_id':old_op,'purpose':'chat'});ctx.check('raw-UUID old code replays without returning a secret',legacy['replay'] and legacy['code_id']==old['code_id'] and legacy['code'] is None)
        o.op('account','enabled',acp,'false',2);before=counts()
        ctx.equal('original paused ACP session remains readable',http('/client/v1/me',da,headers=ch)['conversation_id'],sessions[0]);http('/client/v1/messages',da,{'request_id':key(),'text':'paused'},ch,409)
        ctx.equal('paused original ACP write leaves no receipt',counts(),before)
        o.op('person','set-enabled',o.actor,'false');http('/client/v1/me',da,headers=bh,expected=401);o.op('person','set-enabled',o.actor,'true')
        o.op('person','set-enabled',b,'false');http('/client/v1/me',da,headers=bh,expected=401);o.op('person','set-enabled',b,'true')
        stage=ma['binding_id'];at=o.rows('SELECT max(at)+1 FROM assignment_states WHERE stage=?',stage)[0][0]
        o.op('account','enabled',stage,'false',at);original('paused-default-app');original('paused-source-selected-app',bh)
        before=counts();http('/client/v1/messages',da,{'request_id':key(),'text':'paused default'},expected=409);ctx.equal('paused default does not create new effect',counts(),before)
        o.op('account','enabled',stage,'true',at+1)
        o.op('personal','revoke',da['device_id']);original('revoked',bh)
    with o.svc.running():
        ctx.equal('cold original B device still reads matched old App receipt',http('/client/v1/requests/'+op,db)['event_id'],event)
        http('/client/v1/me',da,headers=bh,expected=401)
    ctx.equal('all fixed original identity scenarios executed',sorted(matched),sorted(vectors))
    from tests.harness.selected_work import exercise
    work_evidence=exercise(ctx,wire,sdk,Plugin)
    return {'selected_app_receipt':example,'denied_counts':{'columns':['events','receipts','codes','model_requests'],'before':denied_before,'after':denied_after},'selected_work':work_evidence,'selected_headers':['X-Yuki-Entry','X-Yuki-Conversation'],'original_oracle_sha256':hashlib.sha256(fixture.read_bytes()).hexdigest(),'scenarios':matched,'app_request_scope':'original request person','acp_request_scope':'original conversation','invalid_selection_writes':0,'real_extension_consumption':False}
