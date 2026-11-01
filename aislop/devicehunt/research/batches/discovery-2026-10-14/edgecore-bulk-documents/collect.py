"""Finite explicit public Edge-Core datasheet collection; no auto redirect/retry."""
from pathlib import Path
from html.parser import HTMLParser
from urllib.request import Request,build_opener,HTTPRedirectHandler
from urllib.error import HTTPError
from urllib.parse import urlsplit,urljoin,parse_qs
from urllib.robotparser import RobotFileParser
import datetime,hashlib,json,re,time

HERE=Path(__file__).resolve().parent
UA='devicehunt-public-research/1.0'
HOST='www.edge-core.com'
CAP=28


def sha(path):
 with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()

def save(name,data):
 with (HERE/name).open('x') as f:json.dump(data,f,indent=2,ensure_ascii=False);f.write('\n')

def endpoint(url):
 p=urlsplit(url);return p.scheme.lower()+'://'+p.netloc.lower()+p.path


class Stop(Exception):pass
class NoRedirect(HTTPRedirectHandler):
 def redirect_request(self,*a,**k):return None


class Landing(HTMLParser):
 def __init__(self,body):
  super().__init__(convert_charrefs=True);self.links=[];self.heading=[];self.in_heading=False;self.feed(body)
 def handle_starttag(self,t,a):
  d=dict(a)
  if t=='h1' and 'entry-title' in d.get('class','').split():self.in_heading=True
  if t=='a' and 'wpdm-download-link' in d.get('class','').split():self.links.append(d.get('href',''))
 def handle_data(self,s):
  if self.in_heading:self.heading.append(s)
 def handle_endtag(self,t):
  if t=='h1':self.in_heading=False


class Collector:
 def __init__(self,plan):
  self.plan=plan;self.entries=[];self.robot=None;self.policy_hash=None;self.delay=0;self.last_request=None
  old=json.loads((HERE/'prior-url-inventory.json').read_text())
  self.prior_urls={r['url'] for r in old['edgecore_urls']}
  self.prior_keys={r['comparison_key'] for r in old['edgecore_urls']}
  pdfs=json.loads((HERE/'prior-pdf-inventory.json').read_text())
  self.prior_hashes={r['sha256'] for r in pdfs['local_pdfs']}|{r['sha256'] for r in pdfs['catalog_reference_documents']}
  self.current_hashes=set()

 def fetch(self,url,name,purpose,basis):
  if any(r.get('status') in (402,403,404,429) or r.get('state')=='error' for r in self.entries):raise Stop('source already stopped on prior failure')
  p=urlsplit(url)
  if p.scheme!='https' or p.netloc!=HOST or p.username or p.password or p.fragment:raise Stop('out-of-scope URL')
  if len(self.entries)>=CAP:raise Stop('28-attempt cap reached')
  if url in {r['url'] for r in self.entries}:raise Stop('repeated batch URL forbidden')
  if purpose!='policy' and (url in self.prior_urls or endpoint(url) in self.prior_keys):raise Stop('historical URL deduplication/exclusion')
  if self.robot is not None and not self.robot.can_fetch(UA,url):raise Stop('robots exact-path denial')
  if purpose!='policy' and self.robot is None:raise Stop('fresh policy required')
  if self.last_request is not None and self.delay:
   time.sleep(max(0,self.last_request+self.delay-time.monotonic()))
  entry=dict(sequence=len(self.entries)+1,url=url,method='GET',purpose=purpose,basis=basis,body_path=name,started_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),request_headers={'User-Agent':UA,'Accept':'*/*'},automatic_redirects=False,timeout_seconds=30,maximum_body_bytes=20_000_000,policy_sha256=self.policy_hash,exact_path_allowed=True if self.robot else None)
  with (HERE/'requests.jsonl').open('a') as f:f.write(json.dumps(dict(entry,state='reserved'))+'\n')
  self.entries.append(entry);body=b''
  try:
   self.last_request=time.monotonic()
   try:response=build_opener(NoRedirect()).open(Request(url,headers=entry['request_headers']),timeout=30)
   except HTTPError as exc:response=exc
   with response:
    entry.update(status=response.status,response_url=response.geturl(),headers={k:v for k,v in response.headers.items() if k.lower() not in ('cookie','set-cookie')})
    body=response.read(20_000_001)
   with (HERE/name).open('xb') as f:f.write(body)
   entry.update(state='completed',bytes=len(body),sha256=sha(HERE/name))
   if len(body)>20_000_000:raise Stop('response exceeds body cap')
  except Exception as exc:
   entry.update(state='error',error=type(exc).__name__+': '+str(exc))
   raise Stop(entry['error']) from exc
  finally:
   entry['finished_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
   save(name+'.receipt.json',entry)
   print(json.dumps({k:entry.get(k) for k in ['sequence','purpose','status','bytes','error']}),flush=True)
  if entry['status'] in (402,403,404,429):raise Stop('source stopped on HTTP '+str(entry['status']))
  if entry['status'] not in (200,301,302,303,307,308):raise Stop('unexpected HTTP '+str(entry['status']))
  return entry,body

 def collect(self):
  policy,body=self.fetch('https://'+HOST+'/robots.txt','robots.txt','policy',dict(selection_plan_sha256=sha(HERE/'selection-plan.json')))
  if policy['status']!=200:raise Stop('policy was not successful200')
  self.policy_hash=policy['sha256'];self.robot=RobotFileParser('https://'+HOST+'/robots.txt');self.robot.parse(body.decode('utf-8').splitlines())
  self.delay=self.robot.crawl_delay(UA) or 0
  rate=self.robot.request_rate(UA)
  if rate:self.delay=max(self.delay,rate.seconds/rate.requests)
  if self.delay>60:raise Stop('published pacing exceeds bounded batch plan')
  save('policy-check.json',dict(policy_sha256=self.policy_hash,agent=UA,crawl_delay_seconds=self.robot.crawl_delay(UA),effective_spacing_seconds=self.delay,planned_landings=[dict(url=s['landing_url'],allowed=self.robot.can_fetch(UA,s['landing_url'])) for s in self.plan['selections']]))
  for selected in self.plan['selections']:
   n=selected['selection_number'];result=dict(selection_number=n,slug=selected['slug'],printed_document_heading=selected['printed_document_heading'],outcome='started')
   self.results.append(result)
   if CAP-len(self.entries)<3:raise Stop('insufficient budget for another complete three-exchange route')
   landing,body=self.fetch(selected['landing_url'],f'{n:02d}-landing.html','landing',dict(source_rows=selected['source_rows']))
   if landing['status']!=200:raise Stop('landing redirected; no alternate landing route')
   if body.startswith(b'%PDF-'):
    final,content=landing,body
   else:
    parsed=Landing(body.decode('utf-8'))
    heading=' '.join(''.join(parsed.heading).split())
    if heading!=selected['printed_document_heading']:raise Stop('fresh landing heading differs from planned full printed heading')
    links=list(dict.fromkeys(urljoin(selected['landing_url'],u) for u in parsed.links if u))
    expected_ids={v for row in selected['source_rows'] for v in row['wpdmdl_values']}
    if len(links)!=1 or set(parse_qs(urlsplit(links[0]).query).get('wpdmdl',[]))!=expected_ids:raise Stop('ambiguous or changed published document ID')
    if endpoint(links[0])!=selected['landing_url']:raise Stop('fresh download endpoint differs from selected landing')
    published=dict(selection_number=n,landing_path=landing['body_path'],landing_sha256=landing['sha256'],fresh_heading=heading,published_download_url=links[0],expected_wpdmdl_values=sorted(expected_ids),stale_graph_tokens_used=False)
    save(f'{n:02d}-published-link.json',published)
    final,content=self.fetch(links[0],f'{n:02d}-download.response','published_download',published)
    if final['status'] in (301,302,303,307,308):
     loc=next((v for k,v in final['headers'].items() if k.lower()=='location'),None)
     if not loc:raise Stop('redirect missing Location')
     target=urljoin(final['url'],loc);p=urlsplit(target)
     if p.scheme!='https' or p.netloc!=HOST or not p.path.startswith('/wp-content/uploads/') or not p.path.lower().endswith('.pdf') or p.query or p.fragment:raise Stop('unexpected redirect target; no alternate route')
     if target in self.prior_urls or endpoint(target) in self.prior_keys:
      result.update(outcome='duplicate_existing_or_excluded_pdf_url',published_pdf_url=target);continue
     final,content=self.fetch(target,f'{n:02d}-pdf.response','published_same_origin_redirect',dict(redirect_receipt=final['body_path']+'.receipt.json',redirect_sha256=final['sha256'],location_raw=loc))
     if final['status']!=200:raise Stop('second redirect forbidden')
   if not content.startswith(b'%PDF-'):
    result.update(outcome='html_or_other_body_only',response_path=final['body_path'],response_sha256=final['sha256'],response_url=final['url'],content_type=next((v for k,v in final['headers'].items() if k.lower()=='content-type'),None));continue
   digest=hashlib.sha256(content).hexdigest()
   if digest in self.prior_hashes or digest in self.current_hashes:
    result.update(outcome='duplicate_pdf_hash',sha256=digest,response_path=final['body_path'],response_url=final['url']);continue
   name=selected['slug']+'.pdf'
   with (HERE/name).open('xb') as f:f.write(content)
   self.current_hashes.add(digest)
   result.update(outcome='new_pdf',pdf=name,sha256=digest,bytes=len(content),response_path=final['body_path'],response_url=final['url'],source_role='documentation_publisher_only')
   print(json.dumps(dict(resource=n,outcome=result['outcome'],bytes=len(content))),flush=True)

 def run(self):
  if (HERE/'requests.jsonl').exists():raise Stop('batch already started; receipts immutable, no automatic resume/retry')
  self.results=[];stop=None
  try:self.collect()
  except Stop as exc:stop=str(exc)
  finally:
   if self.results and self.results[-1]['outcome']=='started':self.results[-1]['outcome']='source_stopped';self.results[-1]['reason']=stop
   save('collection-results.json',dict(selection_plan_sha256=sha(HERE/'selection-plan.json'),outcome='stopped' if stop else 'finite_selection_complete',stop_reason=stop,conservative_attempts=len(self.entries),actual_http_responses=sum('status' in r for r in self.entries),results=self.results))
  print('STOP: '+stop if stop else 'finite selection complete',flush=True)


if __name__=='__main__':
 plan=json.loads((HERE/'selection-plan.json').read_text())
 assert len(plan['selections'])<=12 and plan['maximum_attempts']==CAP
 Collector(plan).run()
