#!/usr/bin/env python3
"""Adversarial tests of the document verifier. Only changes temporary Markdown copies."""
import json,re,shutil,tempfile
from pathlib import Path
from verify_plan import check
R=Path(__file__).resolve().parents[1];C='核心倉庫-提交級任務書.md';X='跨倉庫依賴與驗收矩陣.md';M='Yuki-Rust重寫總任務書-v3.2.md'
def modcard(text,task,field,fn):
 k='## '+task+'　';a,b=text.split(k,1);p=b.split('<a id=',1);s=p[0];pat=r'^(\*\*'+re.escape(field)+r'：\*\* )(.*)$';s,n=re.subn(pat,lambda m:m[1]+fn(m[2]),s,count=1,flags=re.M);assert n;suf='<a id='+p[1] if len(p)>1 else '';return a+k+s+suf
link=lambda i:f'[{i}](核心倉庫-提交級任務書.md#{i.lower()})'
def replace1(s,a,b):
 assert a in s,(a,b);return s.replace(a,b,1)
def run():
 assert check(R)['status']=='PASS';cases={
 '缺失前置':(C,lambda s:modcard(s,'CORE-002','先決',lambda l:l+' '+link('CORE-999'))),
 '重複卡號':(C,lambda s:replace1(s,'## CORE-002　','## CORE-001　')),
 '跨倉 owner 錯':(C,lambda s:replace1(s,'## CORE-002　','## EXT-002　')),
 '依賴循環':(C,lambda s:modcard(s,'CORE-001','先決',lambda _:link('CORE-002'))),
 '下游不對稱':(C,lambda s:modcard(s,'CORE-001','直接交給',lambda _:'無。')),
 '假借沿途原件':(C,lambda s:re.sub(r'(\*\*沿途已取得的輸入（不另排關卡）：\*\* )',r'\1'+link('CORE-047')+' `fake.json`；',s,count=1)),
 '對應產物被改名':(C,lambda s:modcard(s,'CORE-001','完成與界線',lambda v:v.replace('/01/','/01/wrong-',1))),
 '同卡無測試指令':(C,lambda s:replace1(s,'python ci/check.py CORE-001 --out','python ci/check.py UNKNOWN --out')),
 '矩陣錯波':(X,lambda s:replace1(s,'| 1 | [','| 99 | [')),
 '新增不存在錨點':(M,lambda s:s+'\n[broken](#anchor-no-such-thing)\n'),
 '錯誤版本連結':(M,lambda s:s+'\n[old](Yuki-Rust重寫總任務書-v3.1.md)\n'),
 '錯置B0排除條件':(X,lambda s:s.split('| 目標 | 可以不完成的任務',1)[0]+'| 目標 | 可以不完成的任務'+re.sub(r'(^\| \[CORE-044\]\([^)]+\) \| )',lambda m:m[1]+'[EXT-067](擴展倉庫-提交級任務書.md#ext-067)、',s.split('| 目標 | 可以不完成的任務',1)[1],count=1,flags=re.M)),
 };report={}
 for name,(fn,mutation) in cases.items():
  with tempfile.TemporaryDirectory() as t:
   root=Path(t);shutil.copytree(R/'audit',root/'audit',ignore=shutil.ignore_patterns('__pycache__'));[shutil.copy(p,root/p.name) for p in R.glob('*.md')];p=root/fn;v=p.read_text(encoding='utf-8');new=mutation(v);assert new!=v,name;p.write_text(new,encoding='utf-8');
   try:check(root)
   except (ValueError,KeyError,IndexError,TypeError) as err:report[name]=str(err)
   else:raise AssertionError('corrupted plan accepted: '+name)
 return {'normal':'PASS','corruptions_rejected':len(report),'rejections':report,'product_tests_run':False}
if __name__=='__main__':print(json.dumps(run(),ensure_ascii=False,indent=2))
