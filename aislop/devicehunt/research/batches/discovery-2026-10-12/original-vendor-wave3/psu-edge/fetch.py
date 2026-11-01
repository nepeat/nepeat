"""One explicit HTTP exchange per invocation; no automatic redirect/retry."""
import sys,json,hashlib,datetime,urllib.request,urllib.error
from pathlib import Path
ROOT=Path(__file__).resolve().parent
LEDGER=ROOT/'requests.json'
records=json.loads(LEDGER.read_text()) if LEDGER.exists() else []
if len(records)>=7:raise SystemExit('seven-request cap reached')
if any(x.get('status')==429 for x in records):raise SystemExit('throttling stop')
url,name,purpose=sys.argv[1:]
if any(x['requested_url']==url for x in records):raise SystemExit('duplicate URL forbidden')
origin=urllib.request.urlparse(url).netloc if hasattr(urllib.request,'urlparse') else __import__('urllib.parse',fromlist=['urlparse']).urlparse(url).netloc
if any(x.get('status') in (402,403) and x['origin']==origin for x in records):raise SystemExit('origin denied; stopped')
path=ROOT/name
if path.exists():raise SystemExit('output exists')
class NoRedirect(urllib.request.HTTPRedirectHandler):
 def redirect_request(self,req,fp,code,msg,headers,newurl):return None
entry={'sequence':len(records)+1,'requested_url':url,'method':'GET','origin':origin,'purpose':purpose,'started_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'automatic_redirects':False,'request_headers':{'User-Agent':'devicehunt-public-research/1.0','Accept':'*/*'},'timeout_seconds':30}
request=urllib.request.Request(url,headers=entry['request_headers'])
try:
 try: response=urllib.request.build_opener(NoRedirect()).open(request,timeout=30)
 except urllib.error.HTTPError as e:response=e
 with response:
  body=response.read();entry.update(status=response.status,final_url=response.geturl(),headers=dict(response.headers.items()),bytes=len(body),sha256=hashlib.sha256(body).hexdigest(),body_path=name)
  with path.open('xb') as f:f.write(body)
except Exception as e:entry.update(error=type(e).__name__+': '+str(e),status=None)
entry['finished_at_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat();records.append(entry)
LEDGER.write_text(json.dumps(records,indent=2)+'\n')
print(json.dumps({k:entry.get(k) for k in ('sequence','requested_url','status','bytes','sha256','error')}));print('location:',entry.get('headers',{}).get('Location'))
