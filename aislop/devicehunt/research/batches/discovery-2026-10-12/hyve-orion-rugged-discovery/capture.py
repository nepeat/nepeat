"""Root-owned finite saved-href capture; maximum three HTTP exchanges."""
from pathlib import Path
import datetime as dt,hashlib,json,re,subprocess,time
from urllib.robotparser import RobotFileParser
from urllib.parse import urlsplit
HERE=Path(__file__).resolve().parent;ROOT=HERE.parents[3];UA='devicehunt-public-research/1.0';sha=lambda b:hashlib.sha256(b).hexdigest()
def main():
 plan=json.loads((HERE/'plan.json').read_text());assert sha((ROOT/plan['basis']).read_bytes())==plan['basis_sha256'];assert not(HERE/'requests.jsonl').exists();records=[]
 def get(url,name,policy):
  assert len(records)<3 and not any(r['status']==429 for r in records)
  assert urlsplit(url).netloc=='cdn.prod.website-files.com' and urlsplit(url).scheme=='https'
  out=HERE/name;header=HERE/(name+'.headers');assert not out.exists() and not header.exists()
  argv=['curl','--silent','--show-error','--max-time','45','--retry','0','--user-agent',UA,'--dump-header',str(header),'--output',str(out),'--write-out','%{http_code}',url]
  start=dt.datetime.now(dt.timezone.utc).isoformat();p=subprocess.run(argv,capture_output=True,text=True);end=dt.datetime.now(dt.timezone.utc).isoformat();body=out.read_bytes() if out.exists() else b'';headers=header.read_bytes() if header.exists() else b''
  status=int(p.stdout) if re.fullmatch(r'\d{3}',p.stdout) else 0
  r={'url':url,'started_at_utc':start,'finished_at_utc':end,'status':status,'curl_returncode':p.returncode,'stderr':p.stderr,'argv':argv,'body_path':name,'body_bytes':len(body),'body_sha256':sha(body),'header_path':header.name,'header_sha256':sha(headers),'policy':policy,'redirects_followed':False,'retries':0};records.append(r)
  with(HERE/'requests.jsonl').open('a') as f:f.write(json.dumps(r)+'\n')
  return r,body
 r,body=get('https://cdn.prod.website-files.com/robots.txt','robots.txt',None);outcomes=[]
 if r['curl_returncode'] or r['status'] not in (200,404):outcomes.append({'stopped':'policy unavailable','status':r['status']})
 else:
  robots=RobotFileParser();robots.parse(body.decode('utf-8',errors='replace').splitlines() if r['status']==200 else []);delay=robots.crawl_delay(UA) or 0
  for url,name in zip(plan['urls'],['hyve-orion-rubin.pdf','hyve-mil-spec-rugged.pdf']):
   if any(x['status']==429 for x in records):outcomes.append({'url':url,'stopped':'whole-batch throttle'});break
   allowed=robots.can_fetch(UA,url);policy={'url':'https://cdn.prod.website-files.com/robots.txt','status':r['status'],'sha256':sha(body),'user_agent':UA,'exact_path_allowed':allowed,'crawl_delay_seconds':delay}
   if not allowed or delay>60:outcomes.append({'url':url,'stopped':'policy or delay needs checkpoint','policy':policy});continue
   if delay:time.sleep(delay)
   result,pdf=get(url,name,policy);outcomes.append({'url':url,'status':result['status'],'captured_pdf':result['status']==200 and result['curl_returncode']==0 and pdf.startswith(b'%PDF-'),'body_path':name})
 with(HERE/'capture-result.json').open('x') as f:json.dump({'requests':len(records),'outcomes':outcomes},f,indent=2);f.write('\n')
 print(json.dumps({'requests':len(records),'outcomes':outcomes}))
if __name__=='__main__':main()
