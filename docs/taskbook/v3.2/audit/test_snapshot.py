#!/usr/bin/env python3
"""Compare ZIP verification with real Git; reject altered bytes. Temporary offline fixture only."""
import tempfile,subprocess,zipfile,json,os
from pathlib import Path
from check_snapshot import check_archive
from inventory import inventory
with tempfile.TemporaryDirectory() as tmp:
 root=Path(tmp); repo=root/'repo';repo.mkdir();git=lambda *a:subprocess.check_output(['git','-C',str(repo),*a],stderr=subprocess.DEVNULL)
 git('init','-q');(repo/'檔.md').write_text('[proof](misc/proof.json)\n');(repo/'misc').mkdir();(repo/'misc/proof.json').write_bytes(b'{"value":0}\n');(repo/'script.sh').write_bytes(b'#!/bin/sh\nexit 0\n');(repo/'script.sh').chmod(0o755);os.symlink('script.sh',repo/'link');git('add','.');git('-c','user.email=a@example.test','-c','user.name=Fixture','-c','commit.gpgsign=false','commit','-qm','fixture');tree=git('rev-parse','HEAD^{tree}').decode().strip();one=root/'good.zip';git('archive','--format=zip','--prefix=fixture/',f'--output={one}','HEAD');result=check_archive(one,tree);bad=root/'bad.zip'
 with zipfile.ZipFile(one) as src, zipfile.ZipFile(bad,'w') as dst:
  for i in src.infolist():
   b=src.read(i);dst.writestr(i,b+b'alter' if i.filename.endswith('檔.md') else b)
 try:check_archive(bad,tree)
 except ValueError:rejected=True
 else:raise AssertionError('accepted altered archive')
 inv=inventory(repo,root/'inventory','HEAD');assert 'misc/proof.json' in (root/'inventory/evidence-tree.tsv').read_text();print(json.dumps({'fixture_only':True,'git_tree':tree,'zip_result':result,'tampered_zip_rejected':rejected,'cross_directory_attachment_found':True,'real_Yuki_tree_tested':False},ensure_ascii=False,indent=2))
