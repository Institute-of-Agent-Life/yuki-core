"""Real HTTP credentials and installed SDK callbacks before any Work exists."""
import hashlib,json,sqlite3,sys,uuid
from contextlib import closing
from tests.harness.compact import Owner

METHODS=['device.check','device.close','device.connect']

def run(ctx,sdk,Plugin,wire):
    o=Owner(ctx,'connector-auth',45);secrets=[];peers={}
    config=o.svc.data/'yuki.toml'
    for name,grants in [('connector-peer',['device.connections','personal.auth','personal.work','native.events']),('other-peer',['device.connections']),('no-grant',['personal.auth'])]:
        folder=ctx.working/name;folder.mkdir();peers[name]=Plugin(o,folder)
        with config.open('a') as f:f.write('\n[[plugins]]\nid='+json.dumps(name)+'\nnamespace='+json.dumps(name)+'\nprogram='+json.dumps(sys.executable)+'\nargs='+json.dumps([str(ctx.root/'tests/peers/personal/consumer.py'),str(folder),str(sdk/'python-install')])+'\ngrants='+json.dumps(grants)+'\n')
    p=peers['connector-peer'];other=peers['other-peer']
    def http(path,token=None,body=None,expected=200):
        status,value=wire(o.svc.address,path,token,body)
        ctx.equal('connector HTTP '+path+' expected status',status,expected)
        return value.get('data',value)
    def pair(purpose='connector',owner=None):
        code=o.op('personal','issue',owner or o.actor,str(uuid.uuid4()),purpose)['code'];secrets.append(code)
        value=http('/agents/v1/pair' if purpose=='connector' else '/client/v1/pair',body={'code':code,'device_label':'isolated connector identity'})
        secrets.append(value['token']);return value
    def proof(device,plugin='connector-peer',expected=200):
        value=http('/core/v1/entry/exchange',device['token'],{'purpose':'connector','plugin':plugin},expected)
        if expected==200:secrets.append(value['proof']);ctx.equal('connection proof has a distinct scope',value['scope'],'connector');return value['proof']
    def connect(device):
        value=p.rpc('device.connect',{'proof':proof(device)});secrets.append(value['handle']);return value
    def check(value,ok=True):return p.rpc('device.check',{'handle':value['handle']},ok)
    def inject(sql,*args):
        # Isolated expiry or current-installation fault; no secret or device identity is forged.
        with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:db.execute(sql,args);db.commit()
    with o.svc.running():
        for peer in peers.values():peer.ready()
        d=pair();d2=pair()
        ctx.equal('connector pairs create no chat entry',o.rows("SELECT count(*) FROM accounts WHERE platform='app'"),[(0,)])
        chat=pair('chat')
        proof(chat,expected=401)
        http('/core/v1/entry/exchange',d['token'],{'purpose':'chat','plugin':'connector-peer'},422)
        proof(d,'no-grant',503)
        ticket=proof(d)
        other.rpc('device.connect',{'proof':ticket},False)
        p.rpc('device.auth',{'proof':ticket},False)
        p.rpc('device.connect',{'proof':ticket,'device_id':d2['device_id']},False)
        first=p.rpc('device.connect',{'proof':ticket});secrets.append(first['handle'])
        p.rpc('device.connect',{'proof':ticket},False)
        ctx.equal('host returns stored device rather than hello claim',first['device_id'],d['device_id'])
        other.rpc('device.check',{'handle':first['handle']},False)
        p.rpc('work.get',{'handle':first['handle']},False)
        p.rpc('native.queryAck',{'handle':first['handle'],'operation':'guessed','event_key':'guessed'},False)
        before=o.rows('SELECT * FROM connector_connections ORDER BY hash')
        for _ in range(3):check(first)
        ctx.equal('credential rechecks never change connection records',o.rows('SELECT * FROM connector_connections ORDER BY hash'),before)
        grants=o.rows('SELECT grants FROM host_installations WHERE plugin=?','connector-peer')[0][0]
        inject('UPDATE host_installations SET grants=? WHERE plugin=?',json.dumps(['personal.auth','personal.work','native.events']),'connector-peer')
        check(first,False);proof(d,expected=503)
        inject('UPDATE host_installations SET grants=? WHERE plugin=?',grants,'connector-peer');check(first)
        current=connect(d)
        ctx.equal('reconnect keeps namespace and advances connection epoch',[current['device_id'],current['journal_namespace'],current['connection_epoch']],[first['device_id'],first['journal_namespace'],first['connection_epoch']+1])
        check(first,False);p.rpc('device.close',{'handle':first['handle']},False);check(current)
        ctx.equal('late old close cannot close current connection',o.rows('SELECT closed FROM connector_connections WHERE epoch=? AND device=?',(current['connection_epoch']),d['device_id']),[(0,)])
        second=connect(d2)
        ctx.check('two devices have separate stable journals',second['journal_namespace']!=current['journal_namespace'])
        chat_stage=o.rows('SELECT e.actor_stage FROM personal_devices d JOIN entries e ON e.id=d.entry WHERE d.id=?',chat['device_id'])[0][0]
        transition=o.rows('SELECT max(at)+1 FROM assignment_states WHERE stage=?',chat_stage)[0][0]
        o.op('account','enabled',chat_stage,'false',transition)
        check(current)
        b='00000000-0000-4000-8000-000000000055';o.op('person','create',b)
        o.op('account','transfer',o.user['assignment'],2,3,'--person',b)
        check(current)
        ctx.equal('account transfer does not reassign original device',o.rows('SELECT original_person FROM device_origins WHERE device_id=?',d['device_id']),[(o.actor,)])
        o.op('person','merge',o.actor,b);check(current)
        ctx.equal('merge preserves original device provenance',o.rows('SELECT original_person FROM device_origins WHERE device_id=?',d['device_id']),[(o.actor,)])
        before_revoke=proof(d)
        o.op('personal','revoke',d['device_id']);check(current,False);proof(d,expected=401);check(second)
        p.rpc('device.connect',{'proof':before_revoke},False)
        ctx.equal('revocation between proof and redemption rolls back consumption',o.rows('SELECT used FROM connector_proofs WHERE hash=?',hashlib.sha256(before_revoke.encode()).hexdigest()),[(None,)])
        o.op('person','set-enabled',o.actor,'false');check(second,False)
        o.op('person','set-enabled',o.actor,'true');o.op('person','set-enabled',b,'false');check(second,False)
        o.op('person','set-enabled',b,'true');check(second);check(current,False)
        c='00000000-0000-4000-8000-000000000066';o.op('person','create',c)
        headless=pair(owner=c);headless_connection=connect(headless);check(headless_connection)
        ctx.equal('accountless connector authentication creates no account',o.rows('SELECT count(*) FROM assignments WHERE original_person=?',c),[(0,)])
        expired=proof(headless);inject('UPDATE connector_proofs SET expires=0 WHERE hash=?',hashlib.sha256(expired.encode()).hexdigest())
        p.rpc('device.connect',{'proof':expired},False)
        inject('UPDATE connector_connections SET expires=0 WHERE hash=?',hashlib.sha256(headless_connection['handle'].encode()).hexdigest());check(headless_connection,False)
        closed=p.rpc('device.close',{'handle':second['handle']});ctx.equal('current close is explicit',closed['closed'],True)
        ctx.equal('same connection close replays without closing another',p.rpc('device.close',{'handle':second['handle']}),closed);check(second,False)
        live=connect(d2);unredeemed=proof(d2)
        ctx.equal('connection authorization creates no business execution',o.rows('SELECT (SELECT count(*) FROM events),(SELECT count(*) FROM work_nodes),(SELECT count(*) FROM model_requests),(SELECT count(*) FROM personal_receipts)'),[(0,0,0,0)])
    for peer in peers.values():(peer.folder/'ready.json').unlink(missing_ok=True)
    with o.svc.running():
        for peer in peers.values():peer.ready()
        check(live,False);p.rpc('device.connect',{'proof':unredeemed},False)
        fresh=connect(d2)
        ctx.equal('cold reconnect retains original device and journal',[fresh['device_id'],fresh['journal_namespace']],[live['device_id'],live['journal_namespace']])
        ctx.check('cold connection epoch moves forward',fresh['connection_epoch']>live['connection_epoch'])
        check(fresh)
        with closing(sqlite3.connect(o.svc.data/'core.sqlite')) as db:dump='\n'.join(db.iterdump())
        ctx.check('no raw codes tokens proofs or connection handles persisted',all(s not in dump for s in secrets))
    path=ctx.root/'fixtures/personal/connector-identity.json';original=json.loads(path.read_bytes())
    ctx.equal('fixed original connector identity tests passed',original['pytest_exit'],0)
    return {'methods':METHODS,'original_oracle_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'device_id':d2['device_id'],'journal_namespace':fresh['journal_namespace'],'first_epoch':second['connection_epoch'],'cold_epoch':fresh['connection_epoch'],'business_events':0,'paid_calls':0}
