from pathlib import Path
import urllib.request,json,hashlib,datetime
from pypdf import PdfReader
raw=Path('data/raw'); results=[]
for name,url in [
 ('meta-minipack2-spec','https://www.opencompute.org/documents/minipack2-generic-ocp-specification-rev1-0-pdf'),
 ('meta-minipack2-ocp-presentation','https://146a55aca6f00848c565-a7635525d40ac1c70300198708936b4e.ssl.cf1.rackcdn.com/images/a2a5e5b8730291f45f1d302eba4ce9886e6dc982.pdf')]:
 p=raw/(name+'.pdf');assert not p.exists()
 req=urllib.request.Request(url,headers={'User-Agent':'devicehunt/0.1 (local public hardware research)'})
 try:
  with urllib.request.urlopen(req,timeout=45) as response:
   body=response.read(20_000_001)
   assert len(body)<=20_000_000 and body.startswith(b'%PDF-')
   metadata=dict(url=url,final_url=response.url,status=response.status,bytes=len(body),sha256=hashlib.sha256(body).hexdigest(),fetched_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
  p.write_bytes(body)
  pages=[dict(page=n,text=page.extract_text() or '') for n,page in enumerate(PdfReader(p).pages,1)]
  text=raw/(name+'.json')
  text.write_text(json.dumps(dict(pdf_sha256=metadata['sha256'],extraction_method='pypdf embedded text; no OCR',pages=pages),ensure_ascii=False,indent=2)+'\n')
  metadata.update(pdf=p.name,text=text.name,page_count=len(pages))
 except Exception as exc:
  metadata=dict(url=url,error=str(exc))
 results.append(metadata)
(raw/'meta-minipack2-fetch-report.json').write_text(json.dumps(results,indent=2)+'\n')
print(json.dumps(results,indent=2))
