"""Render all six immutable source pages with existing Poppler; no network."""
from pathlib import Path
import hashlib,json,subprocess
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
PDF=HERE.parent/'odm-source-expansion/amd-instinct-catalog.pdf'
TOOLS=Path('/nix/store/wb8486c5l68f1shsbc217b7m4hbgrw3s-poppler-utils-26.06.0/bin')
def sha(p):
 with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
assert sha(PDF)=='8436689a7a45614e4b4078e2bd13ad6aba0403d0407d5d4f79f4b557203c2d5c'
assert not (HERE/'page-1.png').exists()
argv=[str(TOOLS/'pdftoppm'),'-f','1','-l','6','-r','140','-png',str(PDF),str(HERE/'page')]
v=subprocess.run([str(TOOLS/'pdftoppm'),'-v'],capture_output=True,text=True,check=True)
r=subprocess.run(argv,capture_output=True,text=True,check=True)
xmlargv=[str(TOOLS/'pdftotext'),'-bbox-layout','-enc','UTF-8',str(PDF),str(HERE/'pages-bbox.xhtml')]
x=subprocess.run(xmlargv,capture_output=True,text=True,check=True)
images=[dict(page=i,path=f'page-{i}.png',sha256=sha(HERE/f'page-{i}.png')) for i in range(1,7)]
result=dict(source_pdf=str(PDF.relative_to(ROOT)),pdf_sha256=sha(PDF),pages=6,render=dict(argv=argv,returncode=r.returncode,stderr=r.stderr,tool_version=v.stdout+v.stderr,tool_sha256=sha(TOOLS/'pdftoppm'),dpi=140,images=images),bbox=dict(argv=xmlargv,returncode=x.returncode,stderr=x.stderr,path='pages-bbox.xhtml',sha256=sha(HERE/'pages-bbox.xhtml')))
with (HERE/'render-provenance.json').open('x') as f:json.dump(result,f,indent=2);f.write('\n')
print(json.dumps(images,indent=2))
