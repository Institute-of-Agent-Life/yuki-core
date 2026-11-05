"""Offline source precedence, scoped refusal and interrupted SQL retirement.

All SQL writes use disposable stopped copies. The fixed v4 source and its retained
CI artifacts are read-only; no gateway, plugin or provider is launched.
"""
import hashlib,json,os,sqlite3,subprocess,sys,time,tomllib
from pathlib import Path
from contextlib import closing
from tests.peers.chat.recorder import Recorder


def sha(raw):return hashlib.sha256(raw).hexdigest()
def revision(raw,secret):return int(sha(hashlib.sha256(raw).digest()+hashlib.sha256(secret).digest())[:13],16)+1

def run(ctx):
    binary=ctx.root/'target/debug'/('yuki-core.exe' if os.name=='nt' else 'yuki-core')
    fixtures=ctx.root/'tests/fixtures/import/model-v4'
    provenance=json.loads((fixtures/'provenance.json').read_text())
    for name,digest in provenance['members'].items():ctx.equal('retained legacy input '+name,sha((fixtures/name).read_bytes()),digest)
    root=ctx.working/'model-import';root.mkdir()
    def make(name,env=None,overrides=(),extra='',dotenv=''):
        directory=root/name;directory.mkdir();out=root/(name+'-bundle')
        source='''# operator comment retained
schema_version=3
[profiles.main]
provider="fake"
protocol="chat_completions"
base_url="http://127.0.0.1:1"
model="fixture-model"
timeout_seconds=5
max_retries=0
default_temperature=0.21
default_max_output_tokens=301
thinking_mode="disabled"
capabilities=["tools","structured_output"]
operator_extension="preserve private original fields"
[profiles.main.headers]
x-private="fixture-header-canary"
[routes]
chat_agent="main"
plugin_agent_session="main"
memory_extraction="main"
memory_self_reflection="main"
memory_dream="main"
utility_structured="main"
'''+extra
        profile=directory/'selected.toml';profile.write_text(source)
        secret=directory/'model_profiles.secrets.json';secret.write_text(json.dumps({'version':1,'keys':{'RETAINED_KEY':'synthetic-original-sidecar'}}));secret.chmod(0o600)
        environment=directory/'explicit-environment.json';environment.write_text(json.dumps(env or {}));environment.chmod(0o600)
        envfile=directory/'.env';envfile.write_text(dotenv);envfile.chmod(0o600)
        database=directory/'source.db'
        with closing(sqlite3.connect(database)) as db:
            db.executescript('''CREATE TABLE alembic_version(version_num TEXT);INSERT INTO alembic_version VALUES('0156');
CREATE TABLE runtime_config_overrides(id INTEGER PRIMARY KEY,version INTEGER,config_key TEXT,scope_type TEXT,value_json TEXT,apply_mode TEXT,canonical_person_id TEXT,canonical_space_id TEXT);
CREATE TABLE original_effects(id INTEGER PRIMARY KEY,state TEXT,body TEXT);INSERT INTO original_effects VALUES(1,'unknown','must-never-dispatch');''')
            for index,row in enumerate(overrides,1):
                key,value,*rest=row;scope=rest[0] if rest else 'global'
                mode='hot' if key in ['llm.temperature','llm.max_output_tokens','llm.thinking_enabled'] else 'restart_required'
                db.execute('INSERT INTO runtime_config_overrides VALUES(?,1,?,?,?,?,?,?)',(index,key,scope,json.dumps(value),mode,None,'original-group' if scope=='group' else None))
            db.commit()
        command=[str(binary),'--data',str(out),'import-models','--profile',str(profile),'--database',str(database),'--environment',str(environment),'--dotenv',str(envfile),'--revision',str(revision(profile.read_bytes(),secret.read_bytes())),'--stopped-copy']
        return {'directory':directory,'out':out,'profile':profile,'secret':secret,'env':environment,'dotenv':envfile,'db':database,'command':command}
    def rows(f):
        with closing(sqlite3.connect(f['db'])) as db:return db.execute('SELECT id,version,config_key,scope_type,value_json FROM runtime_config_overrides ORDER BY id').fetchall()
    def effect_rows(f):
        with closing(sqlite3.connect(f['db'])) as db:return db.execute('SELECT state,body FROM original_effects').fetchall()
    def source_bytes(f):return {k:(f[k].read_bytes(),f[k].stat().st_mode&0o777) for k in ['profile','secret','env','dotenv']}
    def run_import(f,ok=True,resume=False,environment=None):
        r=subprocess.run(f['command']+(['--resume'] if resume else []),capture_output=True,text=True,env=environment,timeout=20)
        if (r.returncode==0)!=ok:ctx.write('import-failure.json',{'exit_code':r.returncode,'stdout':r.stdout,'stderr':r.stderr})
        ctx.equal('native import '+f['directory'].name+(' resume' if resume else '')+' accepted',r.returncode==0,ok)
        ctx.check('receipts contain no secret value',all(s not in r.stdout+r.stderr for s in ['synthetic-original-sidecar','fixture-header-canary','synthetic-tavily','synthetic-vision']))
        return json.loads(r.stdout) if r.stdout.strip() else {'error':r.stderr.strip()}
    proof=[]
    for supplied,stored in [('defaults','absent'),('dotenv','null'),('process','global'),('process','absent')]:
        env={'LLM_TEMPERATURE':'0.37','LLM_MAX_OUTPUT_TOKENS':'411','LLM_THINKING_ENABLED':'true'}
        overrides=[] if stored=='absent' else [(k,None if stored=='null' else v) for k,v in [('llm.temperature',0.49),('llm.max_output_tokens',771),('llm.thinking_enabled',False)]]
        f=make(supplied+'-'+stored,env if supplied=='process' else {},overrides,dotenv='\n'.join(k+'='+v for k,v in env.items()) if supplied=='dotenv' else '')
        before=source_bytes(f);result=run_import(f);ctx.write('sqlite-build.json',result['sqlite_build']);document=tomllib.loads((f['out']/'model_profiles.toml').read_text());selected=document['profiles'][document['routes']['chat_agent']]
        old=json.loads((fixtures/('explicit-'+supplied+'-'+stored+'.json')).read_text())['selected']
        for key in ['default_temperature','default_max_output_tokens']:
            ctx.equal('original '+supplied+' '+stored+' '+key,selected[key],old[key])
        thinking=selected.get('thinking_enabled',False if selected.get('thinking_mode')=='disabled' else None)
        ctx.equal('original explicit false/null/default thinking',thinking,old['thinking_enabled'])
        ctx.equal('only explicitly consumed legacy environment fields',result['consumed_env_fields'],[] if supplied=='defaults' else sorted(env))
        ctx.equal('one shared agent profile, unchanged memory route',document['routes']['plugin_agent_session'],document['routes']['chat_agent'])
        ctx.equal('non-agent purpose retains original profile',document['routes']['memory_dream'],'main')
        ctx.equal('no split without a true effective difference',len(document['profiles']),1 if supplied=='defaults' else 2)
        ctx.equal('private provider headers survive profile split',selected['headers'],{'x-private':'fixture-header-canary'})
        ctx.equal('unknown private profile fields survive split',selected['operator_extension'],'preserve private original fields')
        ctx.equal('source TOML, sidecar, env and modes stay unchanged',source_bytes(f),before)
        ctx.equal('only original legacy parameter rows retired',rows(f),[])
        ctx.check('per-purpose parameters actually reconcile',len(result['effective_parameters'])==6 and all(v['equal'] for v in result['effective_parameters'].values()))
        written={p.name:p.read_bytes() for p in f['out'].iterdir()}
        repeated=run_import(f,resume=True);ctx.equal('original import result replays',repeated,result)
        ctx.equal('replay does not rewrite bundle',written,{p.name:p.read_bytes() for p in f['out'].iterdir()})
        proof.append(result)
    for mode in ['disabled','native','tavily','both']:
        f=make('search-'+mode,{'WEB_MODE':mode,'TAVILY_API_KEY':'synthetic-tavily','VISION_ENABLED':False,'VISION_BASE_URL':'http://127.0.0.1:1','VISION_API_KEY':'synthetic-vision','ASR_BASE_URL':'','ASR_API_KEY':'','ASR_ENABLED':True})
        before=source_bytes(f);result=run_import(f);doc=tomllib.loads((f['out']/'model_profiles.toml').read_text())
        ctx.equal('key never implicitly enables search '+mode,doc['search']['mode'],mode)
        ctx.equal('disabled vision stays disabled',doc['vision']['enabled'],False)
        ctx.equal('ASR inherits one complete Qwen credential pair',doc['asr']['base_url'],doc['vision']['base_url'])
        secrets=json.loads((f['out']/'model_profiles.secrets.json').read_text())['keys']
        ctx.equal('ASR retains original full credential pair',secrets[doc['asr']['api_key_env']],'synthetic-vision')
        ctx.check('public TOML contains aliases only','synthetic-tavily' not in (f['out']/'model_profiles.toml').read_text())
        ctx.equal('read-only source permissions retained',source_bytes(f),before);proof.append(result)
    f=make('existing-sections',{'WEB_MODE':'both','TAVILY_API_KEY':'synthetic-tavily','VISION_ENABLED':True},extra='\n[search]\nmode="disabled"\n[vision]\nenabled=false\n')
    run_import(f);doc=tomllib.loads((f['out']/'model_profiles.toml').read_text())
    ctx.equal('present disabled sections never replaced by environment',[doc['search']['mode'],doc['vision']['enabled']],['disabled',False])
    for name,overrides in [('scoped',[('llm.temperature',0.7,'group')]),('invalid',[('llm.max_output_tokens',0)])]:
        f=make(name,overrides=overrides);before=source_bytes(f);old=rows(f);run_import(f,ok=False)
        ctx.check('refusal creates no partial bundle',not f['out'].exists());ctx.equal('refusal preserves original rows',rows(f),old);ctx.equal('refusal preserves files',source_bytes(f),before)
    f=make('null-scoped',overrides=[('llm.temperature',None,'group')]);run_import(f)
    ctx.equal('original nonnullable NULL scoped record is not a global override',rows(f),[])
    f=make('stale',overrides=[('llm.max_output_tokens',777)]);f['command'][-2]='0';run_import(f,ok=False)
    ctx.check('stale revision writes nothing',not f['out'].exists())
    fault_proof=[]
    for phase,change in [('files-published','none'),('files-published','version'),('files-published','destination'),('sql-retired','none')]:
        f=make('crash-'+phase+'-'+change,overrides=[('llm.max_output_tokens',777)])
        barrier=root/(f['directory'].name+'-barrier');barrier.mkdir()
        if phase=='sql-retired':(barrier/'files-published.release').write_text('release')
        process=subprocess.Popen(f['command'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env={**os.environ,'YUKI_TEST_MODEL_IMPORT_BARRIER':str(barrier)})
        deadline=time.monotonic()+20
        while not (barrier/phase).exists() and process.poll() is None and time.monotonic()<deadline:time.sleep(0.01)
        if not (barrier/phase).exists():
            process.kill();stdout,stderr=process.communicate();ctx.write('barrier-failure.json',{'stdout':stdout,'stderr':stderr})
        ctx.check('real importer reached '+phase,(barrier/phase).exists())
        process.kill();process.communicate(timeout=5)
        ctx.equal('physical kill preserves correct SQL boundary',len(rows(f)),1 if phase=='files-published' else 0)
        if change=='version':
            with closing(sqlite3.connect(f['db'])) as db:db.execute('UPDATE runtime_config_overrides SET version=2');db.commit()
        if change=='destination':
            with (f['out']/'model_profiles.toml').open('a') as stream:stream.write('\n# user changed destination\n')
        before={p.name:p.read_bytes() for p in f['out'].iterdir()}
        resumed=run_import(f,resume=True,ok=change=='none')
        if change=='version':
            ctx.equal('changed original row reports known partial',resumed['error_category'],'legacy_changed')
            ctx.equal('changed version never retired',rows(f)[0][1],2)
        if change!='none':ctx.equal('conflict never overwrites published bundle',before,{p.name:p.read_bytes() for p in f['out'].iterdir()})
        else:ctx.equal('cold readback retires each original once',rows(f),[])
        fault_proof.append({'phase':phase,'change':change,'receipt':resumed})
    f=make('inspect-only',overrides=[('llm.max_output_tokens',777)])
    before=source_bytes(f);old=rows(f);index=f['command'].index('--revision');del f['command'][index:index+2];f['command']+=['--inspect'];info=run_import(f)
    ctx.equal('offline inventory returns original bundle revision',info['revision'],revision(f['profile'].read_bytes(),f['secret'].read_bytes()))
    ctx.equal('offline inventory preserves original rows',rows(f),old);ctx.equal('inventory preserves source bytes and modes',source_bytes(f),before)
    ctx.check('inventory writes no destination',not f['out'].exists())
    f=make('application-active',overrides=[('llm.max_output_tokens',777)])
    lock_path=Path(str(f['db'])+'.application.lock')
    with lock_path.open('w+b') as lock:
        lock.write(b'0');lock.flush();lock.seek(0)
        if os.name=='nt':
            import msvcrt
            msvcrt.locking(lock.fileno(),msvcrt.LK_NBLCK,1)
        else:
            import fcntl
            fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        try:run_import(f,ok=False)
        finally:
            lock.seek(0)
            if os.name=='nt':msvcrt.locking(lock.fileno(),msvcrt.LK_UNLCK,1)
            else:fcntl.flock(lock,fcntl.LOCK_UN)
    ctx.check('actual legacy Application lock refuses before any bundle write',not f['out'].exists())
    f=make('sql-rollback',overrides=[('llm.temperature',0.6),('llm.max_output_tokens',777)])
    with closing(sqlite3.connect(f['db'])) as db:
        db.execute("CREATE TRIGGER reject_second BEFORE DELETE ON runtime_config_overrides WHEN old.id=2 BEGIN SELECT RAISE(ABORT,'injected SQL failure'); END");db.commit()
    before=rows(f);failed=run_import(f,ok=False)
    ctx.equal('SQL failure reports published file with unconfirmed retirement',[failed['partial'],failed['error_category']],[True,'legacy_retirement_unconfirmed'])
    ctx.equal('SQL failure rolls back every earlier delete',rows(f),before)
    with closing(sqlite3.connect(f['db'])) as db:db.execute('DROP TRIGGER reject_second');db.commit()
    run_import(f,resume=True);ctx.equal('readback resumes only original rows after SQL recovery',rows(f),[])
    f=make('quiesced-wal',overrides=[('llm.temperature',0.6)])
    with closing(sqlite3.connect(f['db'])) as keeper:
        keeper.execute('PRAGMA journal_mode=WAL');keeper.execute("UPDATE runtime_config_overrides SET value_json='0.73'");keeper.commit()
        wal=Path(str(f['db'])+'-wal');ctx.check('stopped fixture has actual uncheckpointed WAL',wal.stat().st_size>0)
        original_wal=sha(wal.read_bytes());result=run_import(f)
        members=result['database_snapshot']['files']
        ctx.check('inventory binds the actual original WAL bytes',any(v['path'].endswith('-wal') and v['sha256']==original_wal for v in members))
        doc=tomllib.loads((f['out']/'model_profiles.toml').read_text())
        ctx.equal('WAL-only source parameter is migrated',doc['profiles'][doc['routes']['chat_agent']]['default_temperature'],0.73)
    with Recorder() as http:
        f=make('no-paid-network',{'VISION_ENABLED':True,'VISION_BASE_URL':http.url,'VISION_API_KEY':'synthetic-vision'})
        run_import(f)
        ctx.equal('offline importer preserves unknown original effect',effect_rows(f),[('unknown','must-never-dispatch')])
    build=subprocess.run([sys.executable,str(ctx.root/'ci/publish-contract.py'),'build','--out',str(ctx.output/'sdk-distributions'),'--verify-work',str(ctx.working/'sdk-install')],capture_output=True,text=True,timeout=120)
    ctx.equal('candidate SDK distributions install and verify',build.returncode,0)
    return {'host_idl_sha256':sha((ctx.root/'sdk/rust/src/host-preview.json').read_bytes()),'baseline_commit':provenance['baseline_commit'],'retained_ci_commit':provenance['ci_commit'],'source_archive_sha256':provenance['archive_sha256'],'cases':proof,'faults':fault_proof,'paid_requests':0,'gateway_started':False,'plugins_started':False,'binary_sha256':sha(binary.read_bytes())}
