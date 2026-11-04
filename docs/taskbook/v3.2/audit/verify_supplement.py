#!/usr/bin/env python3
"""Recompute the continuation audit using the issued Markdown, read ledger and authentic historical CI bytes. No network or product execution."""
from pathlib import Path
import argparse,ast,csv,hashlib,io,json,re,zipfile,collections
from xml.etree import ElementTree as ET
from check_snapshot import tree_id,oid
R=Path(__file__).resolve().parents[1]; sh=lambda b:hashlib.sha256(b).hexdigest()
SHA='329ca7c487414d2f54ddd98ff4727446ed2446cd'; ORIG='YuanYeYouTao/Yuki'
D={'plugin-development':'1ea6f2a03a5a19922e7f824101390bd014eed9f3','emoji-system':'a7a439d322896ae1ba0e3b7ac8e015a9c845349a','speech':'551db205783b4f1342a0eb3f1dc3b3b4d20cf50c','deployment':'983753a3ffe1742ac3d01e791bf8b8facbe5f61f','refactor':'ef32ea8f204741f85f259d110881d400e29ee1af'}
ARCH={'unit':'a62edfcca7c2e8bbcbbcd1886b90d9f2207c8f83575c86d448923c8888473a17','headed':'c8f8c924f43fb32629fe00feb5a3824344dd7ec38f793641b32c55bb5f95ae69','native':'c1fcaf4bcefa4b26d4f5245110d42f8bc2e7c6ac37bce69e8c2e83c50acb6cf6'}
def req(v,msg):
 if not v:raise ValueError(msg)
def run(out):
 out.mkdir(exist_ok=True,parents=True);rows=list(csv.DictReader((R/'audit/results/coverage.tsv').open(encoding='utf-8'),delimiter='\t'));report={'source_commit':SHA,'product_tests_executed_in_this_pass':False,'whole_repository_review':False,'subtrees':{},'upstream_ci':{},'real_source_locations':{}};members=[];sources=[]
 md=[r for r in rows if r['repository']==ORIG and r['path'].lower().endswith('.md')];fore={r['path']:r for r in md};req(len(fore)==len(md),'paths duplicated')
 for dr,h in D.items():
  prefix='docs/'+dr+'/';rs=[r for r in md if r['path'].startswith(prefix)];entries={r['path'][len(prefix):]:('100644',r['git_blob_if_known']) for r in rs};actual=tree_id(entries);req(actual==h and all(r['read_status'].endswith('全文') for r in rs),dr+' tree/ledger mismatch: '+actual);report['subtrees'][prefix]={'git_tree':actual,'files':len(rs),'full_read_ledger':len(rs)}
 top=list(csv.DictReader((R/'audit/results/docs-top-level.tsv').open(),delimiter='\t')); top_tree=tree_id({r['name']:(r['mode'],r['git_object']) for r in top});req(top_tree=='24c855d73fba9e43ed79f2b91a97b23e60611b46','docs/ direct enumeration');upgrade=[r for r in top if r['name'].startswith('upgrade-') and r['name'].endswith('.md')];req(all(fore['docs/'+r['name']]['git_blob_if_known']==r['git_object'] and fore['docs/'+r['name']]['read_status'].endswith('全文') for r in upgrade),'root upgrade coverage');report['complete_docs_root_upgrade_guides']={'direct_docs_tree':top_tree,'files':len(upgrade),'paths':[r['name'] for r in upgrade]}
 groups=collections.defaultdict(list)
 for r in rows:
  if r['git_blob_if_known']:groups[r['git_blob_if_known']].append(r['repository']+':'+r['path'])
 report['known_identical_git_blobs']={h:paths for h,paths in groups.items() if len(paths)>1};report['known_markdown_status']=dict(collections.Counter(r['read_status'] for r in md));report['total_known_markdown']=len(md)
 for name,digest in ARCH.items():
  path=R/f'audit/evidence/upstream-{name}.zip';req(sh(path.read_bytes())==digest,name+' digest');
  with zipfile.ZipFile(path) as z:
   req(z.testzip() is None and len(z.namelist())==len(set(z.namelist())),name+' CRC/member names');files=z.namelist();testcases=[];caseinfo=[]
   for n in files:
    b=z.read(n);node='';purpose='fixture or ancillary output (not a standalone pass)'
    if n.endswith('.xml'):
     testcases+=ET.fromstring(b).findall('.//testcase');purpose='原 CI JUnit，整組各案例狀態'
    elif n.endswith('evidence.json'):
     d=json.loads(b);node=d.get('nodeid','');purpose='具名測試的原始執行產物';caseinfo.append(node)
     if node.endswith('[paired-unknown]') and 'normal_request' in d:
      call=d['original_call']; key=call['id'];result=d['original_result'];msgs=d['normal_request']['messages'];pair=[(i,m) for i,m in enumerate(msgs) if m.get('tool_call_id')==key];req(len(pair)==1 and pair[0][1]['content']==result and call in msgs[pair[0][0]-1]['tool_calls'] and d['invoked']==[key],'lost/duplicated original pair');report['compact_example']={'original_member':n,'nodeid':node,'original_call_id':key,'byte_equal_original_result_in_next_normal_request':True,'original_dispatches_in_fixture':len(d['invoked']),'known_result_status':json.loads(result)['data']['status'],'fixture_source_revision':d['source_revision'],'note':'unknown refers to this test variant, not proof that the dispatched operation had an unknown effect'}
    elif n=='monty-isolation.json':
     d=json.loads(b);w=d['worker'];req(d['verdict']=='passed' and not w['procfs_exposed'] and '(enforce)' in w['apparmor_profile'] and all(ns['host']!=ns['worker'] for ns in w['namespaces'].values()) and len(w['namespaces'])==6,'isolation regression');report['upstream_native_isolation']={k:d[k] for k in ['platform','worker_sha256','launcher_sha256','verdict']};report['upstream_native_isolation'].update(namespaces=sorted(w['namespaces']),enforcement=w['apparmor_profile'],procfs_exposed=w['procfs_exposed']);purpose='受限原生 worker 的實際隔離探針'
    elif '/source/' in n or '/source-files/' in n or n.endswith('.py'):purpose='案例所保存的來源快照，非本輪產品執行'
    elif n.endswith('.json'):purpose='同一上游案例的輸出或儲存物件，按同目錄 evidence.json 定位'
    members.append((name,n,len(b),sh(b),purpose,node))
   sk=sum(t.find('skipped') is not None for t in testcases);fail=sum(t.find('failure') is not None for t in testcases);err=sum(t.find('error') is not None for t in testcases);report['upstream_ci'][name]=dict(archive_sha256=digest,members=len(files),junit_cases=len(testcases),skipped=sk,failures=fail,errors=err,passed_without_skip=len(testcases)-sk-fail-err,case_artifacts=len(caseinfo));
   if name=='unit':
    selected=('test_failed_rollback_keeps_owner_until_physical_close_even_under_repeated_cancel','test_unconfirmed_physical_close_stops_admission_without_masking_original','test_emoji_only_admission_preserves_unknown_commit_and_original_intent','test_unmatched_group_voice_does_not_buy_asr_or_enqueue_memory','test_wait_source_generation_change_during_asr_does_not_publish');found={t.get('name'):t.get('classname') for t in testcases if t.get('name') in selected and all(t.find(k) is None for k in ('skipped','failure','error'))};req(set(found)==set(selected),'selected JUnit cases');report['named_original_passing_tests']=found
 req('compact_example' in report,'compact example missing')
 with (out/'r2-ci-members.tsv').open('w',encoding='utf8',newline='') as f:w=csv.writer(f,delimiter='\t',lineterminator='\n');w.writerow(['suite','member','bytes','sha256','role','case_if_available']);w.writerows(members)
 with zipfile.ZipFile(R/'audit/evidence/upstream-wheel.zip') as dist,zipfile.ZipFile(io.BytesIO(dist.read(dist.namelist()[0]))) as wheel:
  for suffix,fn in [('social/emoji_intents.py','admit'),('plugin_host/notification_manifest.py','manifest_from_request'),('plugin_host/configuration_service.py','save'),('plugin_host/direct_command_router.py','match'),('http_client.py','request')]:
   matches=[]
   for name in [p for p in wheel.namelist() if p.endswith('/'+suffix)]:
    b=wheel.read(name);tree=ast.parse(b);lines=b.splitlines(keepends=True)
    for node in ast.walk(tree):
     if isinstance(node,(ast.FunctionDef,ast.AsyncFunctionDef)) and node.name==fn:
      matches.append({'member':name,'function':fn,'lines':[node.lineno,node.end_lineno],'source_sha256':sh(b),'span_sha256':sh(b''.join(lines[node.lineno-1:node.end_lineno])),'permalink_at_behavior_baseline':'https://github.com/'+ORIG+'/blob/'+SHA+'/src/'+name+'#L'+str(node.lineno)+'-L'+str(node.end_lineno)})
   req(matches,'missing evidence: '+suffix+':'+fn);report['real_source_locations'][suffix+':'+fn]=matches
 cards={};m=(R/'跨倉庫依賴與驗收矩陣.md').read_text();rids={}
 for f in R.glob('*提交級任務書.md'):
  for chunk in re.split(r'(?=^<a id="(?:core|ext)-\d{3}">)',f.read_text(),flags=re.M)[1:]:
   i=re.search(r'^## ((?:CORE|EXT)-\d{3})　',chunk,re.M)[1];cards[i]=chunk
   for rid in re.findall(r'^\*\*續核用例 (R\d+)｜',chunk,re.M):
    req(rid not in rids,rid+' duplicate');mat=re.search(r'^\| '+rid+r' \|[^\n]*\[('+i+r')\][^\n]*`(artifacts/[^`]+)`',m,re.M);req(mat and mat[2] in chunk,rid+' cross-artifact mismatch');rids[rid]=[i,mat[2]]
 req(len(rids)==23 and set(rids)=={f'R{i:02d}' for i in range(1,24)},'supplement missing');report['r2_cases']=dict(sorted(rids.items()));req(not set(re.findall(r'(?:CORE|EXT)-\d{3}',' '.join(r['task_ids'] for r in rows)))-cards.keys(),'coverage unknown task');
 report['coverage_rows_with_valid_task_ids']=len(rows);report['status']='PASS';return report
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--out',required=True,type=Path);a=p.parse_args();r=run(a.out);(a.out/'r2-source-proof.json').write_text(json.dumps(r,indent=2,ensure_ascii=False)+'\n',encoding='utf8');print(json.dumps({k:r[k] for k in ['status','total_known_markdown','subtrees','upstream_ci','compact_example']},ensure_ascii=False,indent=2))
