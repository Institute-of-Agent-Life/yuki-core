#!/usr/bin/env python3
"""Outer-join an immutable target Git inventory with the semantic reading ledger.

A text extraction, a matching file name, or a new file is never a new semantic signoff.
Inputs and outputs are local. No remote services and no product execution.
"""
from pathlib import Path
import argparse,csv,json
import collections
BASE='329ca7c487414d2f54ddd98ff4727446ed2446cd'; ORIG='YuanYeYouTao/Yuki'
def tsv(p):
 with p.open(encoding='utf-8',newline='') as f:return list(csv.DictReader(f,delimiter='\t'))
def reconcile(tree,ledger,out):
 info=json.loads((tree/'inventory.json').read_text(encoding='utf8'))
 if info['commit']!=BASE:raise ValueError('供給的 inventory 不是任務書指定的 SHA；請另開版本。')
 rows={r['path']:r for r in tsv(tree/'docs-tree.tsv')}; old={r['path']:r for r in tsv(ledger) if r['repository']==ORIG and r['path'].lower().endswith(('.md','.mdown','.markdown','.rst','.adoc'))};out.mkdir(exist_ok=True,parents=True);stats=collections.Counter()
 with (out/'coverage-outer-join.tsv').open('w',encoding='utf8',newline='') as f:
  w=csv.writer(f,delimiter='\t',lineterminator='\n');w.writerow(('path','source_commit','git_blob','equal_blob_other_path','status','type','task_ids','decision','conflict'))
  for p in sorted(rows.keys()|old.keys()):
   r=rows.get(p,{});o=old.get(p,{});status=o.get('read_status') or '待閱讀（僅已擷取來源）';conflict=''
   if not r:status='底稿引用但未見於此樹';conflict='核路徑或移除，不以錯檔代替'
   elif o.get('source_commit',BASE)!=BASE or o.get('git_blob_if_known') not in ('',None,r['git_blob']):status='來源不一致，重新閱讀';conflict='原簽核不得沿用'
   elif r['body_status'].startswith('decode failed'):status='無法解碼，待復核';conflict='核對編碼/原件'
   w.writerow((p,BASE,r.get('git_blob',''),r.get('duplicate_of',''),status,o.get('type','待判讀'),o.get('task_ids',''),o.get('adjudication_and_reason','待據本文與源碼裁決，未宣告完成'),conflict));stats[status]+=1
 result={'sha':BASE,'tree':info['tree'],'rows':sum(stats.values()),'by_status':dict(stats),'new_automatic_semantic_signoffs':0};(out/'coverage-reconcile.json').write_text(json.dumps(result,indent=2,ensure_ascii=False)+'\n',encoding='utf8');return result
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('tree',type=Path);p.add_argument('--ledger',type=Path,default=Path(__file__).parent/'results/coverage.tsv');p.add_argument('--out',type=Path,required=True);a=p.parse_args();print(json.dumps(reconcile(a.tree,a.ledger,a.out),ensure_ascii=False,indent=2))
