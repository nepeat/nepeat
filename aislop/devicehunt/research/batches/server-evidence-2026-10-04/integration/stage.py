"""Historical stage-only helper. Do not rerun against accepted/shared state."""
import hashlib,json,shutil,sqlite3
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt.reference_documents import prepare
ROOT=Path.cwd(); B=ROOT/'research/batches/server-evidence-2026-10-04'; O=B/'integration'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def dump(p,obj):p.write_text(json.dumps(obj,indent=2,ensure_ascii=False)+'\n')
def fresh(p,data):
 assert not p.exists(),str(p)
 p.write_bytes(data)
a=json.loads((B/'acceptance.json').read_text())
for group in ('files','code_sha256'):
 for name,h in a[group].items():assert sha(ROOT/name)==h,name
before=json.loads((B/'before.json').read_text())
assert sha(ROOT/'data/devices.sqlite')==before['database_sha256']
assert sha(ROOT/'data/import-manifest.json')==before['manifest_sha256']
oldbundle=ROOT/'data/raw/reference-documents-with-vendor-discovery-2026-10-04.json'
old=json.loads(oldbundle.read_text());assert len(old['documents'])==45
oldmanifest=json.loads((ROOT/'data/import-manifest.json').read_text())
for name,h in oldmanifest['files'].items():assert sha(ROOT/'data'/name)==h,name
for dst,src in [('data/devices-before-server-evidence-2026-10-04.sqlite','data/devices.sqlite'),('data/import-manifest-before-server-evidence-2026-10-04.json','data/import-manifest.json')]:fresh(ROOT/dst,(ROOT/src).read_bytes())
merged={'sources':[],'devices':[]}
for n in ['oracle/candidate-curated.json','ingrasys/candidate-curated.json','ingrasys/portfolio-proposal.json']:
 d=json.loads((B/n).read_text());merged['sources']+=d['sources'];merged['devices']+=d['devices']
assert len(merged['sources'])==3 and len(merged['devices'])==15
mergedp=ROOT/'data/server-evidence-facts-2026-10-04.json';fresh(mergedp,(json.dumps(merged,indent=2,ensure_ascii=False)+'\n').encode())
for source in merged['sources']:
 p=mergedp.parent/source['snapshot']; assert p.is_file()
newentry=json.loads((B/'ingrasys/reference-pdf-proposal.json').read_text())['documents'][0]
raw=ROOT/'data/raw'
for key,name in [('pdf','server-second-span-eof-derivative.pdf'),('text','server-catalog-pages-text.json')]:
 original=raw/'server-evidence-ingrasys-2026-10-04'/name
 assert sha(original)==newentry[key]['sha256']
 fresh(raw/newentry[key]['path'],original.read_bytes())
prov=raw/'ingrasys-servers-reconstructed-2026-10-04-provenance.json';fresh(prov,(B/'ingrasys/derivative-provenance.json').read_bytes())
new=dict(old);new['documents']=old['documents']+[newentry]
bundle=raw/'reference-documents-with-server-evidence-2026-10-04.json';fresh(bundle,(json.dumps(new,indent=2,ensure_ascii=False)+'\n').encode())
assert new['documents'][:45]==old['documents']
prepare(bundle)
candidate=ROOT/'data/candidate-with-server-evidence-2026-10-04.sqlite';fresh(candidate,(ROOT/'data/devices.sqlite').read_bytes())
c=Catalog(candidate)
result={'reference':import_source(c,'reference-documents',bundle),'curated':import_source(c,'curated',mergedp)}
c.db.close()
con=sqlite3.connect(candidate.as_uri()+'?mode=ro',uri=True)
counts={t:con.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['devices','records','sources','facts','documents','document_pages']}
assert counts=={'devices':4090,'records':5317,'sources':287,'facts':44975,'documents':57,'document_pages':3661},counts
manifest=json.loads(json.dumps(oldmanifest))
matched=[j for j in manifest['imports'] if j['source']=='reference-documents'];assert len(matched)==1
assert matched[0]['path']==str(oldbundle.relative_to(ROOT/'data'))
matched[0]['path']=str(bundle.relative_to(ROOT/'data'))
manifest['imports'].append({'source':'curated','path':str(mergedp.relative_to(ROOT/'data'))})
# Keep every historical hashed input. Register reconstruction recipe and every raw dependency.
register=[mergedp,bundle,prov,raw/newentry['pdf']['path'],raw/newentry['text']['path']]
register += [ROOT/n for n in a['files'] if n.startswith('data/')]
for p in register:manifest['files'][str(p.relative_to(ROOT/'data'))]=sha(p)
expected=manifest['expected'];expected['devices']=4090;expected['records']=5317;expected['documents']=57;expected['document_pages']=3661
expected['sources']=dict(con.execute('SELECT id,records_seen FROM sources'))
assert len(expected['sources'])==287
assert all(manifest['files'][k]==v for k,v in oldmanifest['files'].items())
dump(ROOT/'data/import-manifest.json',manifest)
provenance={str(p.relative_to(ROOT)):sha(p) for p in register}
dump(O/'stage-result.json',{'phase':'staged; no promotion','accepted_freeze_sha256':sha(B/'acceptance.json'),'inputs_provenance':provenance,'candidate':str(candidate),'candidate_sha256':sha(candidate),'live_sha256':sha(ROOT/'data/devices.sqlite'),'manifest_sha256':sha(ROOT/'data/import-manifest.json'),'counts':counts,'import_results':result,'manifest_jobs':len(manifest['imports']),'manifest_inputs':len(manifest['files']),'old_manifest_inputs_preserved':len(oldmanifest['files']),'complete_bundle_old_entries_preserved':45,'complete_bundle_entries':46,'reused_test_result':json.loads((B/'engineering/test-results.json').read_text())})
print(json.dumps({'counts':counts,'jobs':len(manifest['imports']),'inputs':len(manifest['files']),'candidate_sha256':sha(candidate)},indent=2))
