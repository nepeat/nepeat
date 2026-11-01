"""Bounded Red Hat hardware listing expansion: 9 literal saved-facet companies."""
from pathlib import Path
import datetime,hashlib,json,subprocess,time,urllib.parse,urllib.robotparser
OUT=Path(__file__).resolve().parent
ROOT=OUT.parents[3]
UA='devicehunt-research/1.0 (public hardware catalog research; polite, rate-limited)'
PARTNERS=['ASRock Industrial Computer Corp.','ASRockRack','Aivres Systems Inc.','Gigabyte','MiTAC Computing Technology Corp','Pegatron Corporation','TYAN Computer Corporation','Wistron','xFusion Digital Technologies Co. Ltd']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def url_for(partner):
 return 'https://catalog.redhat.com/api/solr?'+urllib.parse.urlencode([('type','search'),('redhat_client','ecosystem-catalog'),('q','*:*'),('wt','json'),('rows','200'),('start','0'),('sort','sortTitle asc'),('fq','language:(en)'),('fq','documentKind:CertifiedHardware'),('fq','partnerName:'+json.dumps(partner))])
def fetch(url,name):
 p=OUT/name;h=OUT/(name+'.headers');assert not p.exists() and not h.exists()
 r=subprocess.run(['curl','--silent','--show-error','--max-time','45','--max-filesize','10000000','--user-agent',UA,'--dump-header',str(h),'--output',str(p),'--write-out','%{http_code}',url],capture_output=True,text=True)
 row={'url':url,'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'status':r.stdout,'exit_code':r.returncode,'error':r.stderr,'path':name,'sha256':sha(p) if p.exists() else None,'bytes':p.stat().st_size if p.exists() else 0}
 with (OUT/'requests.jsonl').open('a') as f:f.write(json.dumps(row)+'\n')
 print(json.dumps(row),flush=True)
 if r.returncode or r.stdout!='200':raise SystemExit('stop on unsuccessful response; no retry')
 return row
if __name__=='__main__':
 original=ROOT/'data/raw/discovery-2026-10-11-redhat-cert/api/facet-partnerName-hardware.json'
 j=json.loads(original.read_text());v=j['facet_counts']['facet_fields']['partnerName'];counts=dict(zip(v[::2],v[1::2]))
 assert all(p in counts and counts[p]<200 for p in PARTNERS)
 plan={'source_basis':str(original.relative_to(ROOT)),'sha256':sha(original),'partners':PARTNERS,'historical_counts':{p:counts[p] for p in PARTNERS},'max_http':10,'per_query_rows_cap':200,'pagination':'none; current larger result sets explicitly incomplete','retries':0}
 with (OUT/'plan.json').open('x') as f:json.dump(plan,f,indent=2);f.write('\n')
 fetch('https://catalog.redhat.com/robots.txt','robots.txt')
 rp=urllib.robotparser.RobotFileParser();rp.parse((OUT/'robots.txt').read_text().splitlines());delay=max(2,rp.crawl_delay(UA) or 0)
 if delay>30:raise SystemExit('large crawl delay checkpoint')
 for i,partner in enumerate(PARTNERS):
  url=url_for(partner)
  if not rp.can_fetch(UA,url):raise SystemExit('policy disallows query; no alternate')
  time.sleep(delay);fetch(url,f'partner-{i+1:02}.json')
  data=json.loads((OUT/f'partner-{i+1:02}.json').read_text());docs=data['response']['docs']
  assert data['response']['start']==0 and all(d['partnerName']==partner for d in docs)
  print(json.dumps({'partner':partner,'numFound':data['response']['numFound'],'captured':len(docs),'categories':sorted(set(d['certified_category'] for d in docs))}),flush=True)
