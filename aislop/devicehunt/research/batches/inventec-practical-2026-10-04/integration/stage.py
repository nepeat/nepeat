import hashlib,json,sqlite3,os
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt.reference_documents import prepare
R=Path.cwd();B=R/'research/batches/inventec-practical-2026-10-04';O=B/'integration'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def dump(p,o):p.write_text(json.dumps(o,indent=2,ensure_ascii=False)+'\n')
def fresh(p,b):
 assert not p.exists(),str(p)
 p.write_bytes(b)
a=json.loads((B/'acceptance.json').read_text());assert sha(B/'acceptance.json')=='519caef3ae2e07e851f7a66151132690389b8b4c4a25614145aed93e6528f3c5'
for g in ['files','code_sha256']:
 for n,h in a[g].items():assert sha(R/n)==h,n
before=json.loads((B/'before.json').read_text());assert sha(R/'data/devices.sqlite')==before['database_sha256'];assert sha(R/'data/import-manifest.json')==before['manifest_sha256']
m=json.loads((R/'data/import-manifest.json').read_text());assert len(m['imports'])==119 and len(m['files'])==2191
for n,h in m['files'].items():assert sha(R/'data'/n)==h,n
old=json.loads((R/a['complete_old_bundle']).read_text());assert len(old['documents'])==46
dump(O/'preflight.json',{'files':a['files'],'code_sha256':a['code_sha256'],'standard_exports':{str(p.relative_to(R)):sha(p) for p in (R/'data/exports').rglob('*') if p.is_file()},'old_manifest':m})
fresh(R/'data/devices-before-inventec-practical-2026-10-04.sqlite',(R/'data/devices.sqlite').read_bytes());fresh(R/'data/import-manifest-before-inventec-practical-2026-10-04.json',(R/'data/import-manifest.json').read_bytes())
merged={'sources':[],'devices':[]};mp=R/'data/inventec-practical-facts-2026-10-04.json'
for n in a['curated_inputs']:
 p=R/n;d=json.loads(p.read_text())
 for s in d['sources']:
  orig=(p.parent/s['snapshot']).resolve();s['snapshot']=os.path.relpath(orig,mp.parent);assert (mp.parent/s['snapshot']).resolve()==orig
 merged['sources']+=d['sources'];merged['devices']+=d['devices']
fresh(mp,(json.dumps(merged,indent=2,ensure_ascii=False)+'\n').encode())
e=json.loads((R/'data/raw/inventec-practical-2026-10-04/golduck/reference-pdf-proposal.json').read_text())['documents'][0]
for k in ['pdf','text']:
 src=R/'data/raw/inventec-practical-2026-10-04/golduck'/e[k]['path'];dst=R/'data/raw'/e[k]['path'];assert sha(src)==e[k]['sha256']
 if dst.exists():assert dst.read_bytes()==src.read_bytes()
 else:fresh(dst,src.read_bytes())
new=dict(old);new['documents']=old['documents']+[e];bp=R/'data/raw/reference-documents-with-inventec-practical-2026-10-04.json';fresh(bp,(json.dumps(new,indent=2,ensure_ascii=False)+'\n').encode());assert new['documents'][:46]==old['documents'];prepare(bp)
cp=R/'data/candidate-with-inventec-practical-2026-10-04.sqlite';fresh(cp,(R/'data/devices.sqlite').read_bytes());c=Catalog(cp)
res={'reference':import_source(c,'reference-documents',bp),'curated':import_source(c,'curated',mp)};c.db.close()
con=sqlite3.connect(cp.as_uri()+'?mode=ro',uri=True);counts={t:con.execute('select count(*) from '+t).fetchone()[0] for t in a['expected_counts']};assert counts==a['expected_counts'],counts
nm=json.loads(json.dumps(m));job=[j for j in nm['imports'] if j['source']=='reference-documents'];assert len(job)==1 and job[0]['path']==str(Path(a['complete_old_bundle']).relative_to('data'));job[0]['path']=str(bp.relative_to(R/'data'));nm['imports'].append({'source':'curated','path':mp.name})
register=[mp,bp]+[R/'data/raw'/e[k]['path'] for k in ['pdf','text']]+[R/n for n in a['files'] if n.startswith('data/')]
for p in register:nm['files'][str(p.relative_to(R/'data'))]=sha(p)
for k in ['devices','records','documents','document_pages']:nm['expected'][k]=counts[k]
nm['expected']['sources']=dict(con.execute('select id,records_seen from sources'));assert all(nm['files'][k]==v for k,v in m['files'].items());assert len(nm['imports'])==120
# Shared manifest write occurs only after frozen inputs, complete preparation and exact candidate counts pass.
dump(R/'data/import-manifest.json',nm)
dump(O/'old-complete-bundle.json',old);dump(O/'stage-result.json',{'counts':counts,'jobs':120,'inputs':len(nm['files']),'imports':res,'old_hashed_inputs_preserved':2191,'old_bundle_entries_preserved':46,'candidate_sha256':sha(cp),'live_sha256':sha(R/'data/devices.sqlite')});print(counts)
