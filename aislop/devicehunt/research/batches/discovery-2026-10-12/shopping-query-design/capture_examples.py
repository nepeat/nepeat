"""Eight readonly examples; generate URL text only, never open searches."""
from pathlib import Path
import contextlib,datetime,hashlib,io,json
from urllib.parse import quote_plus
from devicehunt.catalog import Catalog
from devicehunt.__main__ import main,shopping_links
from devicehunt.listings import is_software_reference
ROOT=Path.cwd();OUT=ROOT/'research/batches/discovery-2026-10-12/shopping-query-design'
if (OUT/'examples-before-and-proposal.json').exists():
 raise SystemExit('Saved before outputs already exist; choose a new capture artifact')
IDS=[('legal_m6_ieit','0401e1a26671f300df5b'),('legal_m6_inspur','3296f362a273e5dde5ac'),('legal_a7_iei','32598b15a60784f4cfd2'),('legal_a7_ieit','aeba47c106c5d5b17c40'),('software_wedge','b764ab4c2c47be5a1677'),('owned_7760','fa989f63133b3fb248d8'),('owned_cs8200','77acd4d1fd66d5464b25'),('ordinary_sd6200','d1daa8e8c3ad705b1118')]
KINDS=('fcc_id','part_number','manufacturer_model','model_search','alias','onie_platform')
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1<<20),b''):h.update(b)
 return h.hexdigest()
def proposed(detail):
 if is_software_reference(detail.get('brand'),detail.get('kind')):return []
 candidates=[detail.get('title'),detail.get('model')]+[i['value'] for i in detail['identifiers'] if i['kind'] in KINDS]
 queries={q for q in candidates if isinstance(q,str) and q.strip()}
 return [{'query':q,'ebay':'https://www.ebay.com/sch/i.html?_nkw='+quote_plus(q),
          'web':'https://www.google.com/search?q='+quote_plus('"'+q+'" used surplus')} for q in sorted(queries)]
p=ROOT/'data/devices.sqlite';started=datetime.datetime.now(datetime.timezone.utc).isoformat();before=sha(p);examples=[]
c=Catalog(p,read_only=True)
try:
 for label,did in IDS:
  d=c.detail(did);old=shopping_links(d);new=proposed(d);stdout=io.StringIO();stderr=io.StringIO()
  with contextlib.redirect_stdout(stdout),contextlib.redirect_stderr(stderr):rc=main(['--db',str(p),'shopping',did])
  assert rc==0 and stderr.getvalue()==''
  examples.append({'label':label,'observed_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
   'device':{k:d[k] for k in ('id','brand','model','revision','title','kind')},
   'software_reference':is_software_reference(d['brand'],d['kind']),
   'source_records':[{k:r[k] for k in ('id','source_id','source_key','source_url')} for r in d['records']],
   'all_explicit_identifiers':d['identifiers'],'legacy_allowed_identifier_kinds':list(KINDS),
   'before_query_rows':old,'cli_argv':['python3','-m','devicehunt','--db',str(p),'shopping',did],
   'cli_returncode':rc,'cli_stdout':stdout.getvalue(),'cli_stderr':stderr.getvalue(),
   'proposal_query_rows':new,'model_query_added':not any(r['query']==d['model'] for r in old) and any(r['query']==d['model'] for r in new)})
finally:c.db.close()
after=sha(p)
artifact={'status':'design simulation only; no production changes','started_utc':started,'finished_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
 'database_sha256_before':before,'database_sha256_after':after,'concurrent_baseline_changed':before!=after,
 'example_count':len(examples),'network_calls':0,'examples':examples,
 'read_input_hashes':{s:sha(ROOT/s) for s in ['AGENTS.md','PROGRESS.md','research/batches/discovery-2026-10-11/review/buyer-audit.md','devicehunt/__main__.py','devicehunt/listings.py','devicehunt/report.html','tests/report.test.cjs','tests/test_readonly_catalog_commands.py','tests/test_listing_software_refs.py']}}
(OUT/'examples-before-and-proposal.json').write_text(json.dumps(artifact,indent=2,ensure_ascii=False)+'\n')
for e in examples:print(e['label'],repr(e['device']['model']),'before',[r['query'] for r in e['before_query_rows']],'proposed',[r['query'] for r in e['proposal_query_rows']])
print('catalog hash observed',before,after)
