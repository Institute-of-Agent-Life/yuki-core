"""Capture literal original artifact pages from a read-only fixed v4 checkout."""
import argparse,hashlib,json,os,subprocess,sys
from pathlib import Path

BASE='329ca7c487414d2f54ddd98ff4727446ed2446cd'

def main():
 p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--out',type=Path,required=True);args=p.parse_args()
 source=args.source.resolve();assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip()==BASE
 assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=source)
 sys.path[:0]=[str(source),str(source/'src')]
 from yuki.tool_results import artifacts as old
 base={'handle':'h','mode':'json','logical_root':'data','provider_id':'p','tool_name':'t'}
 rows=[]
 def capture(name,operation,value,path=(),offset=0,limit=20,budget=8000,query=''):
  kwargs=dict(path=path,offset=offset,limit=limit,base=base,max_characters=budget)
  if operation=='text':result=old._read_text_artifact('h',value,offset=offset,limit=limit,query=query,max_characters=budget)
  else:
   if operation=='search':kwargs['query']=query
   result=getattr(old,'_'+operation+'_json')(value,**kwargs)
  rows.append({'name':name,'operation':operation,'value':value,'raw_value':json.dumps(value,ensure_ascii=False),'path':path,'offset':offset,'limit':limit,'budget':budget,'query':query,'base':base,'result':result})
 value={'z':None,'A':['中\n',{'x':True}],'beta':'Straße 中😀 tail'}
 for op in ('inspect','get'):
  capture(op+'-object',op,value,limit=2)
  capture(op+'-array',op,value['A'],path=('A',),limit=1)
  capture(op+'-scalar',op,True,path=('flag',))
  capture(op+'-past-eof',op,value,offset=20)
 capture('casefold-key-order','get',{'ß':1,'SS':2,'ss':3,'a':4,'A':5,'İ':6,'i\u0307':7},offset=1,limit=4)
 for offset in (0,1,4):capture('unicode-'+str(offset),'get','中😀\n尾',offset=offset,limit=2)
 capture('whole-first-item','get',['first'*200,'later'],budget=400)
 capture('search-parent-dedup','search',{'item':{'b':'hit','a':'hit','nested':['hit','hit']}},query='hit')
 capture('search-complete-record','search',[{'message':'Straße','author':'甲'},{'message':'no','author':'乙'}],query='STRASSE')
 capture('search-oversized-record','search',[{'hit':'a'*1000}],query='hit',budget=600)
 capture('search-paged','search',[{'key':'hit'},{'key':'hit'},{'key':'hit'}],query='hit',offset=1,limit=1)
 capture('search-no-match','search',value,query='absent')
 capture('text-unicode','text','A中😀\n尾',offset=1,limit=3)
 capture('text-folded-offset','text','前ß x STRASSE end',query='strasse',limit=7)
 capture('text-no-match','text','前ß',query='absent')
 capture('text-budget','text','中😀\n'*100,limit=1000,budget=256)
 capture('get-budget','get','中😀\n'*100,limit=1000,budget=400)
 raw=source/'src/yuki/tool_results/artifacts.py'
 result={'source_commit':BASE,'source_path':str(raw.relative_to(source)),'source_sha256':hashlib.sha256(raw.read_bytes()).hexdigest(),'cases':rows}
 args.out.parent.mkdir(parents=True,exist_ok=True)
 with args.out.open('x',encoding='utf-8') as f:json.dump(result,f,ensure_ascii=False,indent=2);f.write('\n')
 print(json.dumps({'cases':len(rows),'sha256':hashlib.sha256(args.out.read_bytes()).hexdigest()}))
if __name__=='__main__':main()
