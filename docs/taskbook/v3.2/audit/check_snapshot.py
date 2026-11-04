#!/usr/bin/env python3
"""Read a ZIP without extraction. Verify payload *and modes* against an exact Git root tree.

Gitlinks / export-ignore / CRLF-rewritten or permission-losing archives need an actual
Git checkout instead. No semantic review is inferred from verified bytes.
"""
import argparse, hashlib, json, re, stat, zipfile
from pathlib import Path, PurePosixPath
ROOT='c1885cc75e3273aea6f4ed4e994b762b0cdfba6e'
def oid(kind,b): return hashlib.sha1(kind+b' '+str(len(b)).encode()+b'\0'+b).hexdigest()
def tree_id(entries):
    root={}
    for path,(mode,h) in entries.items():
        node=root
        for part in path.split('/')[:-1]:node=node.setdefault(part,{})
        name=path.rsplit('/',1)[-1]
        if name in node:raise ValueError('path collision: '+path)
        node[name]=(mode,h)
    def reduce(node):
        records=[]
        for k,v in node.items():
            name=k.encode();folder=isinstance(v,dict);m,h=('40000',reduce(v)) if folder else v
            records.append((name+(b'/' if folder or m=='40000' else b''),m.encode()+b' '+name+b'\0'+bytes.fromhex(h)))
        return oid(b'tree',b''.join(val for _,val in sorted(records)))
    return reduce(root)
def check_archive(archive,expected):
    entries={};docs=[]
    with zipfile.ZipFile(archive) as z:
        for info in z.infolist():
            n=info.filename.rstrip('/'); parts=PurePosixPath(n).parts
            if not n or not parts or n.startswith('/') or '\\' in n or '..' in parts or str(PurePosixPath(n))!=n:raise ValueError('unexpected member: '+n)
            if info.is_dir():continue
            if n in entries:raise ValueError('duplicate '+n)
            mode=info.external_attr>>16; m='120000' if stat.S_ISLNK(mode) else ('100755' if mode & 0o111 else '100644')
            entries[n]=(m,oid(b'blob',z.read(info)))
    candidates=[entries]; prefixes={p.partition('/')[0] for p in entries}
    if len(prefixes)==1 and all('/' in p for p in entries):candidates.append({p.partition('/')[2]:v for p,v in entries.items()})
    for c in candidates:
        if tree_id(c)==expected:
            return {'tree':expected,'verified_blob_count':len(c),'markdown':sorted(p for p in c if p.lower().endswith(('.md','.markdown','.mdown'))),'semantic_review_claimed':False}
    raise ValueError('ZIP tree mismatch; do not use as the full baseline: '+str([tree_id(c) for c in candidates]))
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('zip',type=Path);p.add_argument('--expected-tree',default=ROOT);a=p.parse_args();print(json.dumps(check_archive(a.zip,a.expected_tree),indent=2,ensure_ascii=False))
