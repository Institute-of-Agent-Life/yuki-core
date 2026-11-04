#!/usr/bin/env python3
"""Offline checks of the four-contract review. No product code is imported or executed."""
from __future__ import annotations
import argparse, csv, hashlib, io, json, re, zipfile
from collections import Counter
from pathlib import Path
from xml.etree import ElementTree as ET
from verify_plan import check as check_plan

ROOT = Path(__file__).resolve().parents[1]
BASE_SHA = '8e73bdf30fdf12ce56d0ae3aa1b96cc1015f078afb666cd92251fdf9754fc1a3'
BEHAVIOR = '329ca7c487414d2f54ddd98ff4727446ed2446cd'
CARD_FILES = ('核心倉庫-提交級任務書.md','擴展倉庫-提交級任務書.md')
DOC = ('Yuki-Rust重寫總任務書-v3.2.md',*CARD_FILES,'跨倉庫依賴與驗收矩陣.md')
EXPECTED_DOCS = {'main-agent-runtime':'9aa35f64377342d84131a8c299dc945202a0342b','tool-kernel':'0ea863ef1d2834922a4f84a58f3c096f53aec2d8','model-providers':'84b0440877d90cced157ca8c91a6d9974dc62abb','client-api':'399a3116d88dcc89d5412d22f8ff9e18b1d64a01'}

def require(ok,msg):
    if not ok: raise ValueError(msg)
def digest(data): return hashlib.sha256(data).hexdigest()
def parse_cards(read):
    result = {}
    for name in CARD_FILES:
        for item in re.split(r'(?=^<a id="(?:core|ext)-\d{3}"></a>)',read(name),flags=re.M)[1:]:
            i = re.search(r'^## ((?:CORE|EXT)-\d{3})　',item,re.M)[1]
            require(i not in result,i+' duplicate card'); result[i] = item
    return result

def deps(cards):
    return {(p,i) for i,t in cards.items() for p in re.findall(r'\[((?:CORE|EXT)-\d{3})\]',re.search(r'^\*\*先決：\*\* (.*)$',t,re.M)[1])}

def inherited(cards):
    return {(p,i,n) for i,t in cards.items() for l in re.findall(r'^\*\*沿途已取得的輸入（不另排關卡）：\*\* (.*)$',t,re.M) for p,n in re.findall(r'\[((?:CORE|EXT)-\d{3})\]\([^)]+\) `([^`]+)`',l)}

def ancestors(edges,node):
    seen=set(); todo=[node]
    while todo:
        n=todo.pop(); new={p for p,i in edges if i==n}-seen; seen.update(new);todo.extend(new)
    return seen

def run(root:Path, base:Path|None=None):
    read=lambda n:(root/n).read_text(encoding='utf8')
    plan=check_plan(root); records=json.loads(read('audit/results/core-doc-crosswalk.json'))
    require(records['behavior_commit']==BEHAVIOR and records['input_sha256']==BASE_SHA,'wrong input pin')
    docs={Path(r['path']).stem:r['git_blob'] for r in records['documents']};require(docs==EXPECTED_DOCS,'reviewed file identities differ')
    ledger={r['path']:r for r in csv.DictReader(io.StringIO(read('audit/results/coverage.tsv')),delimiter='\t')}
    for doc in records['documents']:
        r=ledger[doc['path']];require(r['git_blob_if_known']==doc['git_blob'] and r['read_status'].endswith('全文') and r['source_commit']==BEHAVIOR,'ledger '+doc['path'])
    require(all('v3.2 核心文檔補核版' in read(f).splitlines()[0] for f in DOC),'wrong document version')
    cards=parse_cards(read);matrix=read(DOC[-1]).split('<a id="h-matrix"></a>')[1]; coverage=read('全量文檔覆蓋與裁決表.md').split('<a id="core-review"></a>')[1]
    historical={}; wheel_members={}; archroot=root/'audit/evidence'
    for zpath in sorted(archroot.glob('upstream-*.zip')):
        with zipfile.ZipFile(zpath) as z:
            if zpath.name=='upstream-wheel.zip':
                with zipfile.ZipFile(io.BytesIO(z.read(z.namelist()[0]))) as w:
                    wheel_members={k:w.read(k) for k in w.namelist() if k.startswith('yuki/') and k.endswith('.py')}
            for member in z.namelist():
                if not member.endswith('.xml'):continue
                data=z.read(member)
                try: xml=ET.fromstring(data)
                except ET.ParseError: continue
                for t in xml.iter('testcase'):
                    key=(zpath.name,member,t.get('classname'),t.get('name'))
                    historical.setdefault(key,[]).append(tuple(k.tag for k in t if k.tag in ('error','failure','skipped')))
    cases=records['cases'];need={f'H{i:02}' for i in range(1,31)}
    require(len(cases)==len(need) and {c['case'] for c in cases}==need,'H inventory mismatch')
    require(set(re.findall(r'^\*\*(H\d\d)｜','\n'.join(cards.values()),re.M))==need,'unmapped card cases')
    checked=set(); ranges=set();cases_by_group=Counter(); owners=set(); oldnodes=set()
    for c in cases:
        h,i=c['case'],c['task'];card=cards[i]; command=f'python ci/check.py {i} --out artifacts/{i}/01/'; art=c['artifact'];anchor=h.lower(); own=[k for k,s in cards.items() if f'**{h}｜' in s];require(own==[i],'wrong owner '+h)
        require(c['command']==command and command in card and f'`{art}`' in card and c['selector']=='cases.'+h and c['selector'] in card,'missing command/output '+h)
        require(c['acceptance'] in card and c['title'] in card and c['acceptance'] in coverage,'case prose diverged '+h)
        ln=[l for l in matrix.splitlines() if l.startswith('| ['+h+' ')];require(len(ln)==1 and all(x in ln[0] for x in (command,art,c['contract'],c['decision'],i)),'matrix '+h)
        require('<a id="'+anchor+'"></a>' in coverage,'missing evidence anchor '+h)
        for e in c['implementation']:
            raw=wheel_members[e['file'].removeprefix('src/')];a,b=e['lines'];lines=raw.splitlines(keepends=True)
            require(1<=a<=b<=len(lines) and digest(raw)==e['sha256'] and digest(b''.join(lines[a-1:b]))==e['span_sha256'],'wheel source '+h+' '+e['file'])
            require(e['url'].endswith('/'+BEHAVIOR+'/'+e['file']+f'#L{a}-L{b}'),'permalink '+h);checked.add(e['file']);ranges.add((e['file'],a,b))
        t=c['historical'];key=(t['archive'],t['member'],t['classname'],t['test']);require(() in historical.get(key,[]),'old CI testcase absent or not passed: '+h);oldnodes.add(key)
        require(c['consumer']['url'] in coverage and t['url'] in coverage,'consumer/test link missing '+h)
        cases_by_group[c['group']]+=1;owners.add(i)
    edges=deps(cards); inputs=inherited(cards); b0=ancestors(edges,'CORE-044');delta=None
    if base is not None:
        require(digest(base.read_bytes())==BASE_SHA,'wrong continuation ZIP')
        with zipfile.ZipFile(base) as z:
            before=parse_cards(lambda n:z.read(n).decode());require(set(cards)==set(before),'input tasks lost/added')
            for f in (root/'audit/evidence').glob('*.zip'):require(f.read_bytes()==z.read('audit/evidence/'+f.name),'old evidence modified '+f.name)
        old=deps(before);prev=inherited(before);require(edges-old=={('EXT-016','EXT-018')} and old-edges=={('EXT-016','EXT-052')},'unexpected prerequisite delta')
        require(inputs-prev=={('CORE-013','CORE-016','social-parts.json'),('EXT-016','EXT-052','client-resync.json')} and not prev-inputs,'unexpected inherited material delta')
        require(b0==ancestors(old,'CORE-044'),'changed B0 closure')
        delta={'ids_unchanged':True,'B0_ancestor_set_unchanged':True,'direct_added':sorted(edges-old),'direct_removed':sorted(old-edges),'inherited_added':sorted(inputs-prev),'historical_archives_byte_equal':True}
    return {'status':'PASS','user_supplied_base_verified':bool(base),'source_commit':BEHAVIOR,'reviewed_documents':docs,'mapped_cases':len(cases),'cases_by_document':dict(cases_by_group),'case_owner_cards':len(owners),'wheel_source_files':len(checked),'distinct_checked_code_ranges':len(ranges),'historical_junit_nodes_located':len(oldnodes),'local_graph_check':plan,'baseline_difference':delta,'evidence_limits':{'source_documents':'semantic reading via GitHub; Git blobs recorded, originals not rehashed by this script','github_and_vendor_links':'selected external references read; only local links/anchors exhaustively checked','junit':'historical observations; no product tests rerun','live_provider_probes':False,'desktop_reexport_or_ui_e2e':False,'new_rust_e2e':False,'repository_writes':False,'deployment':False}}

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--root',type=Path,default=ROOT);parser.add_argument('--base',type=Path);args=parser.parse_args();print(json.dumps(run(args.root,args.base),ensure_ascii=False,indent=2))
