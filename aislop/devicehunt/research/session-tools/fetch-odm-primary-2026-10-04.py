import json,hashlib,datetime,urllib.request,urllib.error
from pathlib import Path
root=Path('/Users/nep/nocloud/git/nepeat/aislop/fcc_shenanigans/data/raw/odm-primary-2026-10-04');root.mkdir(exist_ok=True)
urls={
'inventec-about':'https://www.inventec.com/en/about',
'inventec-products':'https://www.inventec.com/en/products',
'wiwynn-products':'https://www.wiwynn.com/products',
'wiwynn-company':'https://www.wiwynn.com/about-wiwynn/company-info',
'hyve-hercules':'https://ir.tdsynnex.com/news/news-details/2023/Hyve-Solutions-Announces-Ultra-High-Density-Mass-Storage-Platform/default.aspx',
'synnex-merger':'https://www.sec.gov/Archives/edgar/data/1177394/000119312521264521/d213822dex991.htm',
'foxconn-ingrasys':'https://www.foxconn.com/en-us/press-center/press-releases/latest-news/1172'
}
report=root/'fetch-report.json';entries=json.loads(report.read_text()) if report.exists() else [];seen={r['url'] for r in entries}
for name,url in urls.items():
 if url in seen:continue
 entry={'url':url,'name':name,'checked_at':datetime.datetime.now(datetime.timezone.utc).isoformat()}
 try:
  req=urllib.request.Request(url,headers={'User-Agent':'DeviceHunt/0.1 public hardware research'})
  with urllib.request.urlopen(req,timeout=30) as r:
   raw=r.read(10_000_001)
   if len(raw)>10_000_000:raise ValueError('size exceeds limit')
   if 'html' not in r.headers.get('Content-Type',''):raise ValueError('not HTML')
   filename=name+'.html';(root/filename).write_bytes(raw)
   entry.update(status=r.status,path=filename,sha256=hashlib.sha256(raw).hexdigest(),bytes=len(raw),final_url=r.url)
 except (urllib.error.URLError,TimeoutError,ValueError) as e:entry['error']=str(e)
 entries.append(entry);report.write_text(json.dumps(entries,indent=2)+'\n');print(json.dumps(entry),flush=True)
