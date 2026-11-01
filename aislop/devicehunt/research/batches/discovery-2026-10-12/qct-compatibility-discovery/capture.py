"""Finite two-request public HTML discovery; no redirects or retries."""
from pathlib import Path
import datetime as dt,hashlib,json,re,subprocess,time
from urllib.robotparser import RobotFileParser
HERE=Path(__file__).resolve().parent;ROOT=HERE.parents[3];UA='devicehunt-public-research/1.0'
sha=lambda b:hashlib.sha256(b).hexdigest()
def main():
 plan=json.loads((HERE/'plan.json').read_text());assert sha((ROOT/plan['basis']).read_bytes())==plan['basis_sha256']
 assert not (HERE/'requests.jsonl').exists()
 def get(url,name,policy):
  out=HERE/name;headers=HERE/(name+'.headers');assert not out.exists() and not headers.exists()
  argv=['curl','--silent','--show-error','--max-time','45','--retry','0','--user-agent',UA,'--dump-header',str(headers),'--output',str(out),'--write-out','%{http_code}',url]
  started=dt.datetime.now(dt.timezone.utc).isoformat();p=subprocess.run(argv,capture_output=True,text=True);ended=dt.datetime.now(dt.timezone.utc).isoformat()
  body=out.read_bytes() if out.exists() else b'';head=headers.read_bytes() if headers.exists() else b'';status=int(p.stdout) if re.fullmatch(r'\d{3}',p.stdout) else 0
  login=bool(re.search(rb'<input[^>]+type=["\']?password|<title>[^<]*(?:sign in|log in|login)',body,re.I))
  r={'url':url,'started_at_utc':started,'finished_at_utc':ended,'status':status,'curl_returncode':p.returncode,'stderr':p.stderr,'body_path':name,'body_sha256':sha(body),'body_bytes':len(body),'headers_path':headers.name,'headers_sha256':sha(head),'argv':argv,'policy':policy,'login_marker':login,'redirects_followed':False,'retries':0}
  with (HERE/'requests.jsonl').open('a') as f:f.write(json.dumps(r)+'\n')
  return r,body
 r,body=get('https://www.qct.io/robots.txt','robots.txt',None)
 if r['status']!=200 or r['curl_returncode'] or r['login_marker']:
  result={'stopped':'policy unavailable','request':r}
 else:
  robots=RobotFileParser();robots.parse(body.decode('utf-8').splitlines());allowed=robots.can_fetch(UA,plan['url']);delay=robots.crawl_delay(UA) or 0
  policy={'robots_sha256':sha(body),'user_agent':UA,'exact_url':plan['url'],'allowed':allowed,'delay_seconds':delay}
  if not allowed or delay>60:result={'stopped':'policy disallows or delay needs checkpoint','policy':policy}
  else:
   if delay:time.sleep(delay)
   r,body=get(plan['url'],'compatibility.html',policy)
   result={'status':'captured' if r['status']==200 and not r['curl_returncode'] and not r['login_marker'] else 'stopped without retry or redirect','request':r,'policy':policy}
 with (HERE/'capture-result.json').open('x') as f:json.dump(result,f,indent=2);f.write('\n')
 print(json.dumps({'status':result.get('status',result.get('stopped')),'http':result.get('request',{}).get('status')}))
if __name__=='__main__':main()
