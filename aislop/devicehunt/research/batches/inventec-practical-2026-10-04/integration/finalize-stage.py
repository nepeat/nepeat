import hashlib,json,sqlite3
from pathlib import Path
R=Path.cwd();B=R/'research/batches/inventec-practical-2026-10-04';O=B/'integration'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def load(p):return json.loads(p.read_text())
a=load(B/'acceptance.json');pf=load(O/'preflight.json');before=load(B/'before.json');proof=load(O/'candidate-verification.json')
assert len(proof['duplicate_sensitive_43_table_comparison'])==43
for g in ['files','code_sha256']:
 for n,h in a[g].items():assert sha(R/n)==h,n
for n,h in pf['standard_exports'].items():assert sha(R/n)==h,n
assert sha(R/'data/devices.sqlite')==before['database_sha256'];assert sha(R/'data/import-manifest-before-inventec-practical-2026-10-04.json')==before['manifest_sha256']
m=load(R/'data/import-manifest.json');old=pf['old_manifest'];assert len(m['imports'])==120
for n,h in old['files'].items():assert m['files'][n]==h and sha(R/'data'/n)==h
for i,j in enumerate(old['imports']):
 k=m['imports'][i]
 if j['source']=='reference-documents':assert k==dict(j,path='raw/reference-documents-with-inventec-practical-2026-10-04.json')
 else:assert k==j
merged=load(R/'data/inventec-practical-facts-2026-10-04.json');ds=[];ss=[]
for n in a['curated_inputs']:
 p=R/n;d=load(p);ds+=d['devices']
 for s in d['sources']:
  dst=[x for x in merged['sources'] if x['id']==s['id']][0];assert {k:v for k,v in s.items() if k!='snapshot'}=={k:v for k,v in dst.items() if k!='snapshot'};assert (p.parent/s['snapshot']).resolve()==(R/'data'/dst['snapshot']).resolve();ss.append(s['id'])
assert ds==merged['devices']
from collections import Counter
oc=sqlite3.connect((R/'data/devices-before-inventec-practical-2026-10-04.sqlite').as_uri()+'?mode=ro',uri=True);nc=sqlite3.connect((R/'data/candidate-with-inventec-practical-2026-10-04.sqlite').as_uri()+'?mode=ro',uri=True)
affold=Counter(oc.execute('SELECT * FROM affiliations'));affnew=Counter(nc.execute('SELECT * FROM affiliations'));expectedaff=Counter()
for d in merged['devices']:
 rid=hashlib.sha256(json.dumps([d['source'],d['key']]).encode()).hexdigest()[:24]
 for company,role in d['affiliations']:expectedaff[(rid,company,'Inventec',role)]+=1
assert affnew-affold==expectedaff and not affold-affnew
(O/'new-affiliations-proof.json').write_text(json.dumps({'new_rows':list(map(list,expectedaff.elements())),'count':sum(expectedaff.values()),'exact_accepted_roles':True},indent=2)+'\n')
prior=load(R/'research/batches/server-evidence-2026-10-04/integration/final-freeze.json')
priorcodes=prior.get('code_sha256',prior.get('code_freeze',{}))
# prior source acceptance records the same application/test freeze used by its successful428suite
prioraccept=load(R/'research/batches/server-evidence-2026-10-04/acceptance.json');assert all(a['code_sha256'][k]==v for k,v in prioraccept['code_sha256'].items());assert len(prioraccept['code_sha256'])==162 and set(a['code_sha256'])-set(prioraccept['code_sha256'])=={'scripts/compare_rebuild.py','scripts/refresh_exports.py','scripts/refresh_collection_reports.py'}
tr=load(R/'research/batches/server-evidence-2026-10-04/engineering/test-results.json');assert next(x for x in tr if x['name']=='full-suite')['returncode']==0
assert 'Ran 47 tests' in (O/'tests.log').read_text() and '\nOK\n' in (O/'tests.log').read_text()
paths=[R/'data/devices.sqlite',R/'data/import-manifest.json',R/'data/candidate-with-inventec-practical-2026-10-04.sqlite',R/'data/rebuilt-with-inventec-practical-2026-10-04.sqlite',R/'data/inventec-practical-facts-2026-10-04.json',R/'data/raw/reference-documents-with-inventec-practical-2026-10-04.json']+[p for p in O.iterdir() if p.is_file() and p.name not in ['staging-ready.json','final-hashes.json']]
hashes={str(p.relative_to(R)):sha(p) for p in paths};(O/'final-hashes.json').write_text(json.dumps(hashes,indent=2)+'\n')
st={'phase':'staging ready; not promoted; standard exports unchanged','counts':load(O/'stage-result.json')['counts'],'jobs':120,'hashed_inputs':len(m['files']),'old_inputs_preserved':2191,'old_jobs_preserved':119,'old_bundle_entries_preserved':46,'complete_entries':47,'accepted_sources_and_code_unchanged':True,'curated_transform_only_snapshot_rebase':True,'tests':{'affected':47,'full428_reused_on_equal162application_test_hashes_plus3current_script_hashes':True},'proof':'candidate-verification.json','cli':'cli-verification.json','hashes':'final-hashes.json','live_sha256':sha(R/'data/devices.sqlite'),'candidate_sha256':sha(R/'data/candidate-with-inventec-practical-2026-10-04.sqlite'),'manifest_sha256':sha(R/'data/import-manifest.json'),'standard_exports_preserved':len(pf['standard_exports']),'next_steps':['independent staging review and root acceptance','separately authorized live promotion','standard exports refresh and read-only live verification','root documentation update and independent documentation review']}
(O/'staging-ready.json').write_text(json.dumps(st,indent=2)+'\n');print(json.dumps(st,indent=2))
