"""Read one actual Windows EXT-015 binary against its pinned native Client9 candidate."""
import argparse, hashlib, json, os, socket, sys, time, uuid, zipfile
from contextlib import closing
from http.client import HTTPConnection
from pathlib import Path

def digest(data): return hashlib.sha256(data).hexdigest()

def local_address():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1',0))
        return '127.0.0.1:'+str(sock.getsockname()[1])

def main():
    p=argparse.ArgumentParser(description=__doc__)
    for opt in ['host-snapshot','candidate','extension','out']:p.add_argument('--'+opt,required=True,type=Path)
    a=p.parse_args();source=a.host_snapshot.resolve();out=a.out.resolve();out.mkdir(exist_ok=False)
    frozen=json.loads((source/'.yuki-source.json').read_bytes())
    assert frozen['commit']=='d8bf54c1174741a23fdfad0d99b516a2bb584218'
    assert digest(a.candidate.read_bytes())=='6872f69f4792809ab8cb3721f4f231764fa5ff2338d06df88da6b73be52f6168'
    ext=a.extension.read_bytes();assert digest(ext)=='5b890f5e8244a82738633682a1e7a49f94e3fa576602fd57c503a564aa0a2cad'
    w=out/'run';w.mkdir();proof=out/'proof';proof.mkdir()
    for name,expected in frozen['files'].items():
        if name.startswith(('tests/','migrations/')):
            raw=(source/name).read_bytes();assert digest(raw)==expected
            dest=w/name;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(raw)
    with zipfile.ZipFile(a.candidate) as z:
        c=json.loads(z.read('candidate.json'));exe=z.read('bin/yuki-core.exe');assert digest(exe)==c['members']['bin/yuki-core.exe']['sha256'] and c['source_tree_sha256']==frozen['tree_sha256']
    binary=w/'target/debug/yuki-core.exe';binary.parent.mkdir(parents=True);binary.write_bytes(exe)
    plugin=w/'yuki-client.exe';plugin.write_bytes(ext)
    sys.dont_write_bytecode=True;sys.path.insert(0,str(w))
    from tests.harness import Context
    from tests.harness.compact import Owner,call,reply
    from tests.harness.client_stream import Stream
    from tests.peers.chat.recorder import Recorder
    from tests.peers.onebot.peer import Peer
    ctx=Context(w,proof,{'seed':0,'clock':0});ctx.working=w/'cases';ctx.working.mkdir()
    o=Owner(ctx,'client-extension',42)
    address=local_address();edge=local_address()
    while edge==address:edge=local_address()
    env=dict(os.environ,YUKI_CLIENT_CORE=address,YUKI_CLIENT_LISTEN=edge,YUKI_FIXTURE_QQ='isolated-client-stream')
    def http(method,path,body=None,token=None,extra=None):
        with closing(HTTPConnection(edge,timeout=12)) as s:
            hdr=dict(extra or {});data=None
            if body is not None:data=json.dumps(body).encode();hdr['Content-Type']='application/json'
            if token:hdr['Authorization']='Bearer '+token
            s.request(method,path,body=data,headers=hdr);r=s.getresponse();v=json.loads(r.read());return r.status,v.get('data',v)
    def find(v,entry):return next(x for x in v['conversations'] if x['conversation_id']==entry)
    stages=[];edge_streams={}
    def step(name):
        stages.append(name);print(name,file=sys.stderr,flush=True)
    with Recorder() as model:
        o.config(model);cfg=o.svc.data/'yuki.toml'
        body='\n'.join(l for l in cfg.read_text().splitlines() if not l.startswith('binding_endpoint=')).replace('"127.0.0.1:0"',json.dumps(address))
        cfg.write_text(body+'\n[[plugins]]\nid="client"\nnamespace="client"\nprogram='+json.dumps(str(plugin))+'\ngrants=[]\nenv=["YUKI_CLIENT_CORE","YUKI_CLIENT_LISTEN"]\n[qq]\nnamespace="compact"\ndesignated_ingress="100"\n[[qq.bots]]\nself_id="100"\nstage='+json.dumps(o.bot['assignment'])+'\ntoken_env="YUKI_FIXTURE_QQ"\n')
        with o.svc.running(environment=env):
            o.until(lambda:o.rows("SELECT revision FROM host_installations WHERE plugin='client' AND online"))
            step('original plugin initialized')
            code=o.op('personal','issue',o.actor,str(uuid.uuid4()),'chat')['code']
            status,d=http('POST','/client/v1/pair',{'code':code,'device_label':'original-client'})
            assert status==200 and d['token'];token=d['token']
            qq=Peer(address,'100',env['YUKI_FIXTURE_QQ'])
            try:
                o.until(lambda:o.op('qq','status','100')['online'])
                event=o.human('Test original delivery through the client stream');_,q=o.wire(model)
                entry=o.rows('SELECT entry_id FROM events WHERE id=?',event)[0][0]
                selected,_=http('GET','/client/v1/me',token=token,extra={'X-Yuki-Conversation':entry})
                assert selected==503
                step('pair and explicit selector boundary')
                with closing(Stream(address,d)) as s:
                    k,sync=s.frame();assert k=='sync' and find(sync,entry)['latest_event_id']==event
                    step('direct control sees the original pre-ACK sync')
                    def probe(label,extra=None):
                        start=time.monotonic()
                        phase='HTTP headers'
                        try:
                            with closing(Stream(edge,d,extra=extra)) as via:
                                phase='first sync'
                                k,sync=via.frame();assert k=='sync'
                                edge_streams[label]={'state':'sync','sequence':via.seq,'cursor':find(sync,entry)['latest_event_id']}
                        except (TimeoutError,ConnectionError) as e:
                            edge_streams[label]={'state':'unavailable','phase':phase,'error':type(e).__name__}
                        edge_streams[label]['elapsed_ms']=round(1000*(time.monotonic()-start))
                        step(label+' '+edge_streams[label]['state'])
                    probe('original_extension_initial')
                    reply(q,calls=[call('original-send','send',{'text':'immutable original client receipt'})])
                    msg=qq.receive();assert msg['action']=='send_private_msg' and msg['params']['user_id']==200
                    assert not o.rows('SELECT event FROM personal_live_deliveries WHERE entry=? AND event>?',entry,event)
                    qq.send({'status':'ok','retcode':0,'data':{'message_id':55331},'echo':msg['echo']})
                    wire,q=o.wire(model);result=json.loads(next(m['content'] for m in wire['messages'] if m.get('tool_call_id')=='original-send'))
                    assert result['state']=='confirmed'
                    cursor=o.until(lambda:o.rows('SELECT event FROM personal_live_deliveries WHERE entry=? AND event>?',entry,event))[0][0]
                    s.until(lambda k,v:(k=='sync' and find(v,entry)['latest_event_id']==cursor) or (k=='conversation' and v['conversation_id']==entry and v['latest_event_id']==cursor));seq=s.seq
                    step('direct control sees committed cursor')
                reply(q);o.finished(event)
                with closing(Stream(address,d,extra={'Last-Event-ID':str(seq)})) as s:
                    k,sync=s.frame();assert k=='sync' and s.seq==1 and find(sync,entry)['latest_event_id']==cursor
                probe('original_extension_reconnect',{'Last-Event-ID':str(seq)})
                status,events=http('GET','/client/v1/conversations/'+entry+'/events?after='+str(event),token=token);assert status==200 and [(x['event_id'],x['direction']) for x in events['items']]==[(cursor,'outbound')]
                assert o.rows('SELECT count(*) FROM personal_live_deliveries WHERE entry=? AND event>?',entry,event)==[(1,)]
                o.op('personal','revoke',d['device_id']);assert http('GET','/client/v1/me',token=token)[0]==401
                step('reconnect and revocation verified')
            finally:qq.close()
    rec={'card':'CORE-033','consumer':'EXT-015','source_commit':c['source_commit'],'tree':c['source_tree_sha256'],'core_sha256':digest(a.candidate.read_bytes()),'host_binary_sha256':digest(exe),'extension_sha256':digest(ext),'sdk_versions':c['sdk_versions'],'platform':sys.platform,'live_qq':False,'paid_calls':0,'published':False,'initial_cursor':event,'outbound_cursor':cursor,'observed_sequence':seq,'reconnected_sequence':1,'physical_sends':1,'retrieved_outbounds':1,'explicit_selection_http_status':selected,'revoked_me_status':401,'extension_sse':edge_streams,'direct_control_sse':'sync-post-ack-reconnect-passed','extension_sse_passed':all(x['state']=='sync' for x in edge_streams.values()),'stages':stages}
    f=proof/'real-client-stream.json';f.write_bytes((json.dumps(rec,indent=2)+'\n').encode());print(json.dumps({'proof':str(f),'sha256':digest(f.read_bytes())}))

if __name__=='__main__':main()
