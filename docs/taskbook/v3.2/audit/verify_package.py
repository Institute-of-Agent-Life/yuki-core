#!/usr/bin/env python3
"""Verify raw ZIP members against the canonical self-excluding TSV and optionally originals."""
import argparse,hashlib,json,zipfile
from collections import Counter
from pathlib import Path,PurePosixPath
MAN='audit/results/delivery-sha256.tsv'
def manifest(items):
 return ('path\tbytes\tsha256\n'+''.join(f'{n}\t{len(b)}\t{hashlib.sha256(b).hexdigest()}\n' for n,b in sorted(items.items()) if n!=MAN)).encode()
def verify(path,root=None):
 with zipfile.ZipFile(path) as z:
  names=z.namelist();assert len(set(names))==len(names),'duplicate member';assert z.testzip() is None,'CRC failure'
  for n in names:assert n and str(PurePosixPath(n))==n and not PurePosixPath(n).is_absolute() and '..' not in PurePosixPath(n).parts and '\\' not in n and not n.endswith('/'),('unsafe path',n)
  items={n:z.read(n) for n in names};assert items[MAN]==manifest(items),'bad manifest or unlisted files'
  if root:
   local={p.relative_to(root).as_posix():p for p in root.rglob('*') if p.is_file() and '__pycache__' not in p.parts};assert set(local)==set(items),sorted(set(local)^set(items));
   for n,b in items.items():assert b==local[n].read_bytes(),('mismatch',n)
 return {'zip_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'zip_bytes':path.stat().st_size,'members':len(names),'crc':'PASS','manifest':'PASS','compared_each_with_local_file':bool(root),'per_member':{n:hashlib.sha256(b).hexdigest() for n,b in sorted(items.items())}}
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('archive',type=Path);p.add_argument('--root',type=Path);a=p.parse_args();print(json.dumps(verify(a.archive,a.root),ensure_ascii=False,indent=2))
