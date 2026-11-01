"""Offline finite next-source selection; never fetch or infer catalog identities."""
import hashlib,importlib.util,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[4]
RAW=ROOT/'data/raw/discovery-2026-10-11-spec'
OUT=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('saved_spec_index_parser',ROOT/'research/batches/discovery-2026-10-11/spec/scripts/parse_idx.py')
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
rows=module.rows(str(RAW/'index/cpu2017_results_cpu2017_.html'))
have={p.name for group in ('cpu2017','wave8/cpu2017','wave9/cpu2017') for p in (RAW/group).glob('*.txt')}
matched=[r for r in rows if any(v in (r.get('test_sponsor','')+' '+r.get('hw_model','')).casefold() for v in ('inspur','ieit'))]
fetched={(r['test_sponsor'],r['hw_model']) for r in matched if Path(r['_links']['Text']).name in have}
remaining={}
for r in sorted(matched,key=lambda r:r['_links']['Text'],reverse=True):
 key=(r['test_sponsor'],r['hw_model'])
 if key not in fetched:remaining.setdefault(key,r)
assert len(remaining)==244,len(remaining)
cpu=sorted(remaining.values(),key=lambda r:r['_links']['Text'],reverse=True)[:12]
jbb=module.rows(str(RAW/'index/jbb2015_results_jbb2015_.html'))
wanted={'jbb2015-20160516-00095','jbb2015-20160321-00067','jbb2015-20160321-00066','jbb2015-20160321-00065','jbb2015-20160118-00041','jbb2015-20151123-00036'}
selected=[]
for r in jbb:
 links=r.get('_links',{})
 for label,link in links.items():
  if Path(link).stem in wanted and link.endswith('.html'):
   assert not (RAW/'wave10/jbb2015'/Path(link).name).exists()
   selected.append({'benchmark':'jbb2015','url':'https://www.spec.org'+link,'saved_index_row':r});break
assert len(selected)==6,len(selected)
for r in cpu:selected.append({'benchmark':'cpu2017','url':'https://www.spec.org'+r['_links']['Text'],'saved_index_row':r})
assert len({r['url'] for r in selected})==18
body={'status':'offline plan only; no requests or candidate','selection':'six known remaining oldest Inspur jbb HTML pages plus newest12 of244 remaining distinct sponsor/full-system CPU2017 names; raw index names are discovery only',
 'sources':selected,'dependencies':{str(p.relative_to(ROOT)):sha(p) for p in (RAW/'index/cpu2017_results_cpu2017_.html',RAW/'index/jbb2015_results_jbb2015_.html',Path(module.__file__),RAW/'wave10/robots/robots.txt')},
 'limits':{'http_cap_including_robots_redirects':20,'searches':0,'screenshots':0,'minimum_delay_seconds':10},
 'rules':['Refresh robots first and enforce current exact-path policy; no CGI endpoints.','Count every exchange including redirects within cap; no automatic redirects or retries.','Stop source on402/403/login/robots denial or404/timeout; no alternate route to denied or missing item. Stop run on429.','Published index link only; jbb HTML, never retry known missing jbb txt.','Read all saved request ledgers for prior failure overlap before acquisition.','No cloud instance rows, import, manufacturer/deployment inference or normalization of raw index identities.']}
with (OUT/'plan.json').open('x') as f:json.dump(body,f,indent=2,ensure_ascii=False);f.write('\n')
print(json.dumps({'remaining_cpu_names':len(remaining),'proposed_sources':len(selected),'names':[(r['benchmark'],r['saved_index_row'].get('hw_model'),r['url']) for r in selected]},indent=2))
