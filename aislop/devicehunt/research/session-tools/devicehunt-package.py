import hashlib,json,subprocess
from pathlib import Path
src=Path('/Users/nep/.codex/.chatgpt-projects/g-p-6ab8261241248191896f6d9090905253')
dst=Path('/Users/nep/nocloud/git/nepeat/aislop/fcc_shenanigans')
assert src.is_dir() and not dst.exists(), 'Refusing to overwrite destination'
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as s:
  for b in iter(lambda:s.read(4*1024*1024),b''):h.update(b)
 return h.hexdigest()
dst.mkdir()
# APFS copy-on-write clones preserve independent files without doubling snapshot space.
subprocess.run(['/bin/cp','-cRp',str(src)+'/.',str(dst)],check=True)
entries=[]
files=sorted(p for p in src.rglob('*') if p.is_file())
for n,p in enumerate(files,1):
 rel=p.relative_to(src);q=dst/rel
 assert not p.is_symlink() and q.is_file() and not q.is_symlink(),rel
 a=sha(p);b=sha(q);assert a==b,rel
 assert p.stat().st_ino!=q.stat().st_ino, 'Unexpected hard link: '+str(rel)
 entries.append(dict(path=str(rel),bytes=p.stat().st_size,sha256=a))
 if n%250==0:print('Verified',n,'/',len(files),flush=True)
report=dict(source=str(src),destination=str(dst),method='Independent APFS copy-on-write clones',
 copied_files=len(entries),logical_bytes=sum(x['bytes'] for x in entries),all_files_match=True,
 phase='Before intended destination AGENTS replacement and current cache-path relocation',files=entries)
(dst/'data/exports/handoff-copy-manifest.json').write_text(json.dumps(report,indent=2)+'\n')
# User explicitly requested local continuation instructions in this folder.
(dst/'AGENTS.md').write_bytes((dst/'docs/handoff-AGENTS.md').read_bytes())
print(json.dumps({k:v for k,v in report.items() if k!='files'},indent=2),flush=True)
