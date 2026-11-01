import json,datetime,hashlib
from pathlib import Path
from urllib.request import Request,urlopen
from urllib.error import HTTPError,URLError
root=Path.cwd()/'data/raw/cubscout-fcc';root.mkdir(parents=True,exist_ok=True)
report=[]
for name,url,kind in [
 ('index.html','https://fcc.report/FCC-ID/2AVPL-CUBSCOUT','html'),
 ('5661528.pdf','https://fcc.report/FCC-ID/2AVPL-CUBSCOUT/5661528.pdf','pdf'),
 ('5657033.pdf','https://fcc.report/FCC-ID/2AVPL-CUBSCOUT/5657033.pdf','pdf')]:
 entry=dict(path=name,url=url,checked_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
 try:
  with urlopen(Request(url,headers={'User-Agent':'DeviceHunt/0.1 public equipment research'}),timeout=25) as response:
   if response.geturl()!=url:raise ValueError('Unexpected redirect: '+response.geturl())
   body=response.read(20_000_001)
   if len(body)>20_000_000:raise ValueError('Response exceeds size bound')
   if kind=='pdf' and not body.startswith(b'%PDF-'):raise ValueError('Not PDF bytes')
   if kind=='html' and 'text/html' not in response.headers.get('Content-Type',''):raise ValueError('Not HTML')
  (root/name).write_bytes(body)
  entry.update(status='captured',sha256=hashlib.sha256(body).hexdigest(),bytes=len(body))
 except (HTTPError,URLError,TimeoutError,ValueError) as exc:
  entry.update(status='unavailable',error=str(exc))
 report.append(entry);(root/'fetch-report.json').write_text(json.dumps(report,indent=2)+'\n')
 print(json.dumps(entry),flush=True)
