"""Observe a committed Social Outbound over actual Client9 HTTP; a hint carries no chat body."""
import hashlib
import json
import time
from contextlib import closing
from urllib.request import Request, urlopen

class Stream:
    def __init__(self, address, device, query='', extra=None):
        self.response=urlopen(Request('http://'+address+'/client/v1/stream'+query,headers={'Authorization':'Bearer '+device['token'],**(extra or {})}),timeout=10)
        self.seq=0
        assert self.response.headers.get_content_type()=='text/event-stream'
    def close(self):
        self.response.close()
    def frame(self):
        frame={}
        while True:
            line=self.response.readline()
            if not line:return None
            line=line.decode().strip()
            if not line:
                if not frame:continue
                num=int(frame['id']);assert num==self.seq+1;self.seq=num
                return frame['event'],json.loads(frame['data'])
            if not line.startswith(':'):
                name,value=line.split(':',1);frame[name]=value.strip()
    def until(self,predicate):
        until=time.monotonic()+10
        while time.monotonic()<until:
            item=self.frame()
            if item is None:raise AssertionError('stream closed before expected hint')
            if predicate(*item):return item
        raise AssertionError('expected stream projection did not arrive')

def observe(ctx,o,wire,pair,device,foreign,work,event,ack):
    fixture=ctx.root/'fixtures/personal/client-stream-v4.json'
    def entry(snapshot,id):return next(r for r in snapshot['conversations'] if r['conversation_id']==id)
    def http(path,who,status=200):
        actual,data=wire(o.svc.address,path,who['token']);ctx.equal('SSE consumer '+path+' status',actual,status);return data.get('data',data)
    qq=o.rows('SELECT entry_id FROM events WHERE id=?',event)[0][0]
    newcomer=pair(o.actor)
    ctx.equal('invalid works mode rejected',wire(o.svc.address,'/client/v1/stream?works=typo',device['token'])[0],422)
    with closing(Stream(o.svc.address,foreign)) as others:
        kind,snapshot=others.frame();ctx.check('foreign stream cannot see original QQ or Work',kind=='sync' and all(x['conversation_id']!=qq for x in snapshot['conversations']) and all(x['work_id']!=work for x in snapshot['works']))
    o.op('person','set-admin',o.actor,'true')
    with closing(Stream(o.svc.address,device)) as admin:
        character=admin.frame()[1]['character']
        ctx.equal('original superuser receives only SOUL revisions',set(character['soul']),{'revision','active_revision'})
    o.op('person','set-admin',o.actor,'false')
    with closing(Stream(o.svc.address,device,'?works=active')) as stream:
        kind,before=stream.frame();prior=entry(before,qq)['latest_event_id']
        ctx.check('normal device cannot observe privileged SOUL revisions','soul' not in before.get('character',{}))
        ctx.equal('initial subscription sees committed human event, not pending upload',[kind,stream.seq,prior],['sync',1,event])
        ctx.check('original active Work appears in initial revision projection',any(r['work_id']==work and r['steps_token'] for r in before['works']))
        activity=next(r for r in before['activity'] if r['conversation_id']==qq)
        ctx.check('activity reflects actual executing task without model or tool payloads',activity['evidence']=='executing' and activity['sources'] and all(set(r)=={'origin','started_at','kind'} for r in activity['sources']))
        before_count=o.rows('SELECT count(*) FROM personal_live_deliveries WHERE entry=?',qq)[0][0]
        ack()
        out=o.until(lambda:o.rows('SELECT event FROM personal_live_deliveries WHERE entry=? AND event>? ORDER BY event',qq,prior))[0][0]
        def committed(k,v):
            if k=='sync':return entry(v,qq)['latest_event_id']==out
            return k=='conversation' and v['conversation_id']==qq and v['latest_event_id']==out
        kind,posted=stream.until(committed);ctx.check('post-commit frame exposes the durable outbound cursor',committed(kind,posted))
        last=stream.seq
    with closing(Stream(o.svc.address,device,extra={'Last-Event-ID':str(last)})) as resumed:
        kind,sync=resumed.frame();ctx.equal('reconnect opens a new sequence from durable sync',[kind,resumed.seq,entry(sync,qq)['latest_event_id']],['sync',1,out])
        page=http('/client/v1/conversations/'+qq+'/events?after='+str(prior),device)
        ctx.equal('reconnect event cursor retrieves exactly the original outbound',[(i['event_id'],i['direction']) for i in page['items']],[(out,'outbound')])
    with closing(Stream(o.svc.address,newcomer)) as closing_stream:
        assert closing_stream.frame()[0]=='sync'
        o.op('personal','revoke',newcomer['device_id'])
        stop=time.monotonic()+8
        while time.monotonic()<stop:
            frame=closing_stream.frame()
            if frame is None:break
        else:raise AssertionError('revoked SSE did not close')
    http('/client/v1/stream',newcomer,401)
    ctx.equal('subscribers and reconnects did not redispatch or duplicate delivery',o.rows('SELECT count(*) FROM personal_live_deliveries WHERE entry=?',qq),[(before_count+1,)])
    return {'source_commit':json.loads(fixture.read_bytes())['source_commit'],'original_oracle_sha256':hashlib.sha256(fixture.read_bytes()).hexdigest(),'conversation':qq,'before_ack_cursor':prior,'committed_cursor':out,'resume_from':prior,'reconnect_seq':1,'post_commit_delivery_count':1,'revoked_stream_closed':True,'real_extension_consumption':False}
