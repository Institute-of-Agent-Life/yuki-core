#!/usr/bin/env python3
"""Verify the issued Markdown, not the generator's in-memory graph. Stdlib only."""
from pathlib import Path
from graphlib import TopologicalSorter
from urllib.parse import unquote,urlsplit
import argparse, json,re,sys,collections
ROOT=Path(__file__).resolve().parents[1]
P=r'(?:CORE|EXT)-[0-9]{3}'
def check(root):
 def require(v,msg):
  if not v:raise ValueError(msg)
 def ids(s):return re.findall(r'\[('+P+r')\]',s)
 def get(s,field):
  a=re.search(r'^\*\*'+field+'：\\*\\* (.*)',s,re.M);return a[1] if a else ''
 T={p.name:p.read_text(encoding='utf8') for p in root.glob('*.md')}; files=['核心倉庫-提交級任務書.md','擴展倉庫-提交級任務書.md']; D={}; I={}; Re={}; A={}; counter=0
 for f,g in zip(files,['CORE','EXT']):
  parts=re.split(r'(?=^<a id="(?:core|ext)-\d{3}"></a>)',T[f],flags=re.M);index=ids(re.search(r'^\*\*索引：.+$',parts[0],re.M)[0]); parsed=[]
  for s in parts[1:]:
   i=re.search(r'^## ('+P+r')　',s,re.M)[1];require(s.startswith('<a id="'+i.lower()+'"></a>'),i+' anchor/heading mismatch');require(i not in D and i.startswith(g),f'duplicate/wrong owner {i}');parsed.append(i)
   for k in ['先決','變更面','先設驗收','不變條件','重現','完成與界線','遷移／相容／回退','直接交給']:require(get(s,k),f'{i} lacks {k}')
   require('**實作與提交邊界：**' in s and s.index('**先設驗收')<s.index('**實作與提交邊界'),i+' missing red-first case');require(re.search(r'`(?:feat|test|fix|build|chore|perf|docs|ci|refactor)\([^`]+?\): [^`]+`',s),i+' commit');require(f'tests/cases/{i}.py' in s and f'python ci/check.py {i} --out artifacts/{i}/01/' in s,i+' runner');D[i]=set(ids(get(s,'先決')));Re[i]=set(ids(get(s,'直接交給')));inp=get(s,'沿途已取得的輸入（不另排關卡）');I[i]=dict(re.findall(r'\[('+P+r')\]\([^)]+\) `([^`]+)`',inp));require(len(ids(get(s,'先決')))==len(D[i]) and len(ids(get(s,'直接交給')))==len(Re[i]),i+' duplicate edge');require(set(ids(inp))==set(I[i]),i+' unnamed input'); A[i]=re.search(r'artifacts/'+i+r'/01/([^`]+)',get(s,'完成與界線'))[1]
  require(len(set(parsed))==len(parsed),'repeated card');require(parsed==index,f+' index');counter+=len(parsed)
 require(all(d<=D.keys() for d in D.values()),'missing prerequisite');ac={};wave={};order=list(TopologicalSorter(D).static_order());
 for i in order:
  par=D[i];up=set().union(*(ac[j] for j in par));ac[i]=par|up;wave[i]=max([wave[j] for j in par] or [0])+1;require(not(par&up),i+' transitive redundancy');require(set(I[i])<=up and all(A[j]==v for j,v in I[i].items()),i+' ghost input or wrong artifact');require({j for j in D if i in D[j]}==Re[i],i+' reverse edges')
 x=T['跨倉庫依賴與驗收矩陣.md']; mp=x.split('## 4　',1)[1].split('### 舊資料',1)[0];mapped=[]
 for line in mp.splitlines():
  if re.search(r'\[('+P+r')\]',line):
   it=ids(line);require(len(it)==1,'ambiguous capability row');i=it[0];mapped.append(i);cols=line.strip('|').split('|');require(len(cols)==6 and A[i] in cols[-1] and re.findall('E[0-9]{2}',cols[-2]) and re.findall('K[0-9]{2}',cols[-3]),i+' capability artifact/contract')
 require(sorted(mapped)==sorted(D),'capabilities omit/duplicate tasks'); wr=x.split('| 波次 |',1)[1].split('### 兩倉交接',1)[0];order_seen=[]
 for w,s in re.findall(r'^\| (\d+) \| (.*?) \|$',wr,re.M):
  for i in ids(s):require(wave[i]==int(w),i+' wave');order_seen.append(i)
 require(sorted(order_seen)==sorted(D),'wave coverage');cross=x.split('| 消費者 | 直接跨倉先決／原件 |',1)[1].split('## 6',1)[0];seen=set();counts=0
 for line in cross.splitlines():
  if not line.startswith('| ['):continue
  c=line.strip('|').split('|');i=ids(c[0])[0];require(i not in seen,'duplicate cross consumer');seen.add(i)
  for seg,ds in zip(c[1:],(D[i],I[i])):
   found=dict(re.findall(r'\[('+P+r')\]\([^)]+\) `([^`]+)`',seg));expected={j:A[j] for j in ds if j[:3]!=i[:3]};require(found==expected and set(ids(seg))==set(expected),i+' cross pins')
 require(seen=={i for i in D if any(j[:3]!=i[:3] for j in D[i]|I[i].keys())},'cross coverage');
 # These guard lines belong to the final matrix, not a separate manually maintained allow-list.
 attack=x.split('| 目標 | 可以不完成的任務',1)[1].split('本版驗證',1)[0]
 for ln in attack.splitlines():
  if ln.startswith('| ['):
   c=ln.strip('|').split('|');i=ids(c[0])[0];require(not(ac[i]&set(ids(c[1]))) and set(ids(c[2]))<=ac[i],i+' release gate attack');counts+=1
 require(set('EXT-004 EXT-028 EXT-067 CORE-039'.split())<=ac['CORE-044'],'B0 lost actual witnesses');req={'CORE-055','CORE-056','CORE-057','EXT-068','EXT-069','EXT-070','EXT-019','EXT-042','EXT-044','CORE-040'};require(not req&ac['CORE-044'],'B0 indirectly gated');require({'CORE-057','EXT-068','EXT-069','EXT-070'}<=ac['CORE-047'],'final release lacks new work');
 nlinks=0;external=0
 for f in files+['Yuki-Rust重寫總任務書-v3.2.md','跨倉庫依賴與驗收矩陣.md','全量文檔覆蓋與裁決表.md','audit/README.md']:
  text=(root/f).read_text();require(not re.search(r'\]\([^)]*重寫總任務書-v3\.1\.md',text),'stale link');
  for u in re.findall(r'\[[^\]\n]*\]\(([^()\s]+)\)',text):
   v=urlsplit(u)
   if v.scheme:external+=1;continue
   dest=(root/f).parent/unquote(v.path) if v.path else root/f;require(dest.is_file(),f+' missing '+u)
   if v.fragment:
    anchor=unquote(v.fragment);require('id="'+anchor+'"' in dest.read_text(),f+' missing anchor '+u)
   nlinks+=1
 return dict(tasks=len(D),core=sum(i.startswith('CORE') for i in D),extensions=sum(i.startswith('EXT') for i in D),direct_edges=sum(map(len,D.values())),cross_edges=sum(i[:3]!=j[:3] for i in D for j in D[i]),inherited_named_inputs=sum(map(len,I.values())),waves=max(wave.values()),B0_wave=wave['CORE-044'],B0_ancestors=len(ac['CORE-044']),capability_rows=len(mapped),adversarial_gate_rows=counts,local_file_anchor_links=nlinks,external_link_occurrences_not_network_tested=external,product_tests_run=False,status='PASS')
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--root',type=Path,default=ROOT);a=p.parse_args();print(json.dumps(check(a.root),ensure_ascii=False,indent=2))
