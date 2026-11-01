"""Finite public LVFS metadata capture. No firmware downloads or automatic retries."""
from pathlib import Path
import datetime,hashlib,json,subprocess,urllib.robotparser
OUT=Path(__file__).resolve().parent
AGENT='DevicehuntResearch/1.0'
def fetch(url,name):
 p=OUT/name;h=OUT/(name+'.headers');assert not p.exists() and not h.exists()
 started=datetime.datetime.now(datetime.timezone.utc).isoformat()
 r=subprocess.run(['curl','--silent','--show-error','--max-time','60','--max-filesize','50000000','--user-agent',AGENT,'--dump-header',str(h),'--output',str(p),'--write-out','%{http_code}',url],capture_output=True,text=True)
 result={'url':url,'started_utc':started,'status':r.stdout,'exit':r.returncode,'stderr':r.stderr,'file':name,'sha256':hashlib.sha256(p.read_bytes()).hexdigest() if p.exists() else None,'bytes':p.stat().st_size if p.exists() else 0}
 with (OUT/'requests.jsonl').open('a') as f:f.write(json.dumps(result)+'\n')
 print(json.dumps(result),flush=True)
 return result
if __name__=='__main__':
 policy=fetch('https://cdn.fwupd.org/robots.txt','robots.txt')
 if policy['exit'] or policy['status'] not in ('200','404'):raise SystemExit('stopped at policy')
 url='https://cdn.fwupd.org/downloads/firmware.xml.zst'
 if policy['status']=='200':
  rp=urllib.robotparser.RobotFileParser();rp.parse((OUT/'robots.txt').read_text().splitlines())
  if not rp.can_fetch(AGENT,url):raise SystemExit('robots disallow')
  delay=rp.crawl_delay(AGENT)
  if delay:raise SystemExit(f'policy delay {delay}; checkpoint before metadata')
 result=fetch(url,'firmware.xml.zst')
 if result['exit'] or result['status']!='200':raise SystemExit('metadata unavailable; no retry')
