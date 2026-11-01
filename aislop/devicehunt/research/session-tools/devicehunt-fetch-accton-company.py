import json,datetime,hashlib
from pathlib import Path
from urllib.request import Request,urlopen
from urllib.error import HTTPError,URLError
root=Path.cwd()/'data/raw/fcc-company-accton';report=[]
for n in range(1,4):
 url=json.loads((root/('page-'+str(n)+'.json')).read_text())['page_url']
 entry=dict(page=n,url=url,checked_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
 try:
  with urlopen(Request(url,headers={'User-Agent':'DeviceHunt/0.1 public-index research'}),timeout=25) as r:
   if r.geturl()!=url or 'text/html' not in r.headers.get('Content-Type',''):raise ValueError('Unexpected redirect or content type')
   body=r.read(5_000_001)
   if len(body)>5_000_000:raise ValueError('HTML exceeds reviewed size limit')
  file=root/('page-'+str(n)+'.html');file.write_bytes(body)
  entry.update(status='captured',path=file.name,sha256=hashlib.sha256(body).hexdigest())
 except (HTTPError,URLError,TimeoutError,ValueError) as exc:
  entry.update(status='unavailable',error=str(exc))
 report.append(entry);(root/'fetch-report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(entry),flush=True)
 if entry['status']=='unavailable':break
