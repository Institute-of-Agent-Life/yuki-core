"""Exercise opt-in foreground audio with the original QQ receiver and model ledger."""

import base64
from contextlib import closing
from hashlib import sha256
import io
import json
import os
import subprocess
import sys
import time
import wave

from tests.harness.compact import Owner, reply
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer


def run(ctx, sdk, install, peer_type):
    owner = Owner(ctx, 'voice', 58)
    with owner.svc.running():
        space = json.dumps(['qq', 'compact', '702'], separators=(',', ':'))
        entry = owner.op('entry', 'group', space)['entry']
        other = owner.op('account', 'open', 'qq', 'compact', '300', 1, '--start', 1)['assignment']
    config = owner.svc.data / 'yuki.toml'
    folder = ctx.working / 'main-voice-peer'
    audio = io.BytesIO()
    with wave.open(audio, 'wb') as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(8000)
        out.writeframes(bytes(160))
    raw = audio.getvalue()
    speech = '/ai new quoted voice content'
    decisions = []

    def packet(bot, native, segments, *, group=702, sequence=None):
        frame = {'post_type': 'message', 'message_type': 'group', 'self_id': bot, 'time': 100,
                 'message_id': native, 'user_id': 200, 'group_id': group, 'message': segments}
        if sequence is not None:
            frame['message_seq'] = sequence
        return frame

    def send_record(peer, native, key):
        peer.send(packet(100, native, [{'type': 'record', 'data': {'file': key}}], sequence=native))
        request = peer.receive()
        ctx.equal('QQ conversion uses original receiver and recording', (request['action'], request['params']),
                  ('get_record', {'file': key, 'out_format': 'mp3'}))
        peer.send({'echo': request['echo'], 'status': 'ok', 'retcode': 0,
                   'data': {'base64': base64.b64encode(raw).decode()}})

    def find(native):
        return owner.until(lambda: owner.rows('SELECT event_id FROM event_locators WHERE native_id=?', str(native)))[0][0]

    # Verify a shared group sequence from paired synthetic receiving sockets.
    # The fixture decision never claims a real QQ deployment.
    digest = 'sha256:' + sha256((ctx.root/'tests/peers/onebot/peer.py').read_bytes()).hexdigest()
    for i, (occurrence, session, bot, native, sequence) in enumerate([
        ('a', 'A', 100, 1, 91), ('a', 'B', 300, 19, 91), ('b', 'A', 100, 2, 92),
        ('b', 'B', 300, 20, 92), ('a', 'A2', 100, 1, 91), ('echo', 'B', 300, 21, 93),
    ]):
        source = ctx.working / f'main-voice-pair-{i}.json'
        frame = packet(bot, native, [{'type':'record','data':{'file':'fixture'}},
                                     {'type':'at','data':{'qq':'300'}}], sequence=sequence)
        if occurrence == 'echo':
            frame.update(post_type='message_sent', user_id=bot)
        source.write_text(json.dumps(frame))
        decisions.append({'occurrence':occurrence,'session':session,'file':source.name,
                          'sha256':sha256(source.read_bytes()).hexdigest()})
    capture = ctx.working / 'main-voice-pairs.json'
    capture.write_text(json.dumps({'kind':'target_capture','target_image_digest':digest,
                       'capture_image_digest':digest,'sequence_field':'message_seq','observations':decisions}))
    decision = subprocess.check_output([sys.executable, ctx.root/'baseline/qq-key/decision.py', capture])
    ctx.equal('paired QQ fixture admits a shared sequence', json.loads(decision)['group']['mode'], 'scoped_shared_key')
    (owner.svc.data/'voice-decision.json').write_bytes(decision)
    env = dict(os.environ, YUKI_TEST_QQ_A='fixture-voice-A', YUKI_TEST_QQ_B='fixture-voice-B')
    with Recorder() as main, Recorder() as upstream:
        owner.config(main, 'audio_processor="aux"\n', binding=False)
        install(ctx, owner, folder, sdk, upstream, entry)
        config.write_text(config.read_text().replace(
            '"forget.owner"', '"forget.owner","media.process","media.open"',
        ).replace('purposes=["review"]', 'purposes=["transcript"]\nmedia=["audio"]') + f'''
[qq]
namespace="compact"
designated_ingress="100"
[qq.groups]
"702"="open"
"703"="open"
[[qq.bots]]
self_id="100"
stage="{owner.bot['assignment']}"
token_env="YUKI_TEST_QQ_A"
[[qq.bots]]
self_id="300"
stage="{other}"
token_env="YUKI_TEST_QQ_B"
[qq.shared_decision]
file="voice-decision.json"
sha256="{sha256(decision).hexdigest()}"
target_image_digest="{digest}"
''')
        with owner.svc.running(environment=env):
            owner.until((folder/'ready.json').exists)
            peer = peer_type(owner, folder)
            with closing(Peer(owner.svc.address,'100',env['YUKI_TEST_QQ_A'])) as a, \
                 closing(Peer(owner.svc.address,'300',env['YUKI_TEST_QQ_B'])) as b:
                send_record(a, 101, 'original-silk-record')
                source = find(101)
                first_call = owner.until(lambda:list(folder.glob('call-*.json')))[0]
                original = json.loads(first_call.read_bytes())['params']
                key = peer.submit('llm.complete', {'origin':{'handle':original['handle']},'call_id':'source',
                    'route':'jev','purpose':'transcript','text':'fixture transcript','selected_media':True})
                provider, answer = owner.wire(upstream)
                encoded = provider['messages'][-1]['content'][0]['input_audio']['data']
                ctx.equal('auxiliary model receives the proved recording bytes', base64.b64decode(encoded), raw)
                a.send(packet(100, 102, [{'type':'text','data':{'text':'independent text'}}],group=703))
                text_source = find(102)
                text_model, q = owner.wire(main)
                ctx.check('another Main lane completes text while ASR is held',
                          'independent text' in json.dumps(text_model) and speech not in json.dumps(text_model))
                reply(q, 'text complete')
                owner.finished(text_source)
                reply(answer, speech)
                receipt = peer.receive(key)
                (folder/('finish-'+original['handle']+'.json')).write_text(
                    json.dumps({'ok':True,'data':{'text':speech}}))
                audio_model, q = owner.wire(main)
                shown = '\n'.join(m.get('content','') for m in audio_model['messages'] if isinstance(m.get('content'),str))
                ctx.check('first Main request carries the original transcript as source data',
                          '语音转写（原附件内容）' in shown and speech in shown and f'"source_event":{source}' in shown)
                reply(q, 'voice complete')
                owner.finished(source)
                ctx.equal('spoken text did not invoke reset or a plugin command', owner.rows(
                    'SELECT (SELECT count(*) FROM main_controls)+(SELECT count(*) FROM host_command_events)'), [(0,)])
                b.send(packet(300, 901, [{'type':'record','data':{'file':'other-receiver-key'}}], sequence=101))
                ctx.equal('two receiving locators resolve to one original audio event', find(901), source)
                a.send(packet(100, 103, [{'type':'reply','data':{'id':101}},
                      {'type':'text','data':{'text':'refer to the earlier recording'}}], sequence=103))
                quoted = find(103)
                follow, q = owner.wire(main)
                ctx.check('later history supplies the saved transcript without buying it again',
                          speech in json.dumps(follow, ensure_ascii=False))
                reply(q, 'quoted source complete')
                owner.finished(quoted)
                ctx.check('quoted and second Bot input sent no additional auxiliary HTTP', upstream.wires.empty())
                ctx.equal('all audio charges stay on the original Main root', owner.rows(
                    "SELECT root FROM model_requests WHERE id LIKE 'plugin-aux:%'"), [(receipt['root'],)])
                ctx.equal('only one Host processor call was opened',
                          owner.rows("SELECT count(*) FROM host_calls WHERE method='media.process'"), [(1,)])
                started = time.monotonic()
                send_record(a, 104, 'failed-record')
                failed = find(104)
                pending = owner.until(lambda: next((
                    json.loads(p.read_bytes())['params'] for p in folder.glob('call-*.json')
                    if p != first_call), None))
                request = peer.submit('llm.complete', {'origin':{'handle':pending['handle']},'call_id':'failed',
                    'route':'jev','purpose':'transcript','text':'fixture transcript','selected_media':True})
                _, denied = owner.wire(upstream)
                failed_model, q = owner.wire(main, timeout=12)
                shown = '\n'.join(m.get('content','') for m in failed_model['messages'] if isinstance(m.get('content'),str))
                ctx.check('bounded audio preparation marks only its original segment not ready',
                          f'"source_event":{failed},"state":"not_ready"' in shown)
                denied.put(({'error':'fixture unavailable'}, 0, 503))
                no_transcript = peer.receive(request)
                ctx.equal('failed provider reports the original rejection', no_transcript['state'], 'rejected')
                (folder/('finish-'+pending['handle']+'.json')).write_text(
                    json.dumps({'ok':False,'error':'transcript unavailable'}))
                reply(q, 'received metadata only')
                owner.finished(failed)
                ctx.equal('unavailable audio creates no transcript', owner.rows(
                    'SELECT text FROM history_transcripts WHERE source=?', failed), [])
                elapsed = round(time.monotonic()-started, 3)
                duplicate = find(901)
        config.write_text(config.read_text().replace('root_slots=8', 'root_slots=1'), encoding='utf-8')
        with owner.svc.running(environment=env), closing(Peer(owner.svc.address, '100', env['YUKI_TEST_QQ_A'])) as a:
            a.send(packet(100, 105, [{'type':'record','data':{'file':'single-slot-record'}}], sequence=105))
            limited = find(105)
            last, q = owner.wire(main)
            shown = '\n'.join(m.get('content','') for m in last['messages'] if isinstance(m.get('content'),str))
            ctx.check('one remaining model slot is reserved for the foreground',
                      f'"source_event":{limited},"state":"not_ready"' in shown)
            reply(q, 'received metadata within the configured budget')
            owner.finished(limited)
            ctx.equal('single-slot Main does not open a media processor call', owner.rows(
                "SELECT count(*) FROM host_calls WHERE method='media.process'"), [(2,)])
            ctx.check('single-slot Main sends no auxiliary HTTP', upstream.wires.empty())
        return {'source':source,'other_receiver':duplicate,'quote':quoted,
                'provider':receipt,'transcript_in_main':True,'paid_calls':0,
                'unavailable':{'source':failed,'provider':no_transcript,'elapsed_seconds':elapsed},
                'single_slot':{'source':limited,'foreground_preserved':True}}
