"""Historical authorized correction and stage continuation; do not rerun."""
import hashlib,json,sqlite3
from pathlib import Path
from pypdf import PdfReader
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt.reference_documents import prepare
ROOT=Path.cwd(); B=ROOT/'research/batches/server-evidence-2026-10-04';O=B/'integration';raw=ROOT/'data/raw'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def dump(p,obj):p.write_text(json.dumps(obj,indent=2,ensure_ascii=False)+'\n')
def fresh(p,data):assert not p.exists(),str(p);p.write_bytes(data)
a=json.loads((B/'acceptance.json').read_text());before=json.loads((B/'before.json').read_text())
for group in ['files','code_sha256']:
 for name,h in a[group].items():assert sha(ROOT/name)==h,name
assert sha(ROOT/'data/devices.sqlite')==before['database_sha256']
assert sha(ROOT/'data/import-manifest.json')==before['manifest_sha256']
assert not (ROOT/'data/candidate-with-server-evidence-2026-10-04.sqlite').exists()
assert sha(ROOT/'data/devices-before-server-evidence-2026-10-04.sqlite')==before['database_sha256']
assert sha(ROOT/'data/import-manifest-before-server-evidence-2026-10-04.json')==before['manifest_sha256']
originaltext=raw/'ingrasys-servers-reconstructed-2026-10-04-text.json';d=json.loads(originaltext.read_text());assert set(d)=={'pages'}
pdf=raw/'ingrasys-servers-reconstructed-2026-10-04.pdf'; pages=[{'page':i,'text':p.extract_text() or ''} for i,p in enumerate(PdfReader(pdf,strict=True).pages,1)];assert pages==d['pages'] and len(pages)==10
extra={'pdf_sha256':sha(pdf),'extraction_method':'pypdf embedded text; no OCR'}
envelope=raw/'ingrasys-servers-reconstructed-2026-10-04-text-envelope.json';fresh(envelope,(json.dumps({**d,**extra},indent=2)+'\n').encode())
bundle=raw/'reference-documents-with-server-evidence-2026-10-04.json';new=json.loads(bundle.read_text());oldbundle=raw/'reference-documents-with-vendor-discovery-2026-10-04.json';old=json.loads(oldbundle.read_text());assert new['documents'][:45]==old['documents']
proposal=json.loads((B/'ingrasys/reference-pdf-proposal.json').read_text())['documents'][0];assert new['documents'][45]==proposal
added=json.loads(json.dumps(proposal));added['text']={'path':envelope.name,'sha256':sha(envelope)};new['documents'][45]=added;dump(bundle,new)
assert {k:v for k,v in added.items() if k!='text'}=={k:v for k,v in proposal.items() if k!='text'}
prepare(bundle)
correction={'authorization':'root explicit scoped formatting correction after staging failure','original_text_sha256':sha(originaltext),'new_envelope_sha256':sha(envelope),'exact_added_metadata':extra,'original_pages_identical':True,'strict_pypdf_pages_equal':True,'complete_bundle_original45entries_identical':True,'added_entry_diff_only_text_path_and_sha256':True,'original_source_proposal_sha256':sha(B/'ingrasys/reference-pdf-proposal.json'),'failure_proof':'staging-failure.json'};dump(O/'text-envelope-transformation.json',correction)
mergedp=ROOT/'data/server-evidence-facts-2026-10-04.json';expectedmerged={'sources':[],'devices':[]}
for n in ['oracle/candidate-curated.json','ingrasys/candidate-curated.json','ingrasys/portfolio-proposal.json']:
 d=json.loads((B/n).read_text());expectedmerged['sources']+=d['sources'];expectedmerged['devices']+=d['devices']
assert json.loads(mergedp.read_text())==expectedmerged
candidate=ROOT/'data/candidate-with-server-evidence-2026-10-04.sqlite';fresh(candidate,(ROOT/'data/devices.sqlite').read_bytes())
c=Catalog(candidate);result={'reference':import_source(c,'reference-documents',bundle),'curated':import_source(c,'curated',mergedp)};c.db.close()
con=sqlite3.connect(candidate.as_uri()+'?mode=ro',uri=True)
counts={t:con.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['devices','records','sources','facts','documents','document_pages']};assert counts=={'devices':4090,'records':5317,'sources':287,'facts':44975,'documents':57,'document_pages':3661},counts
oldmanifest=json.loads((ROOT/'data/import-manifest.json').read_text());manifest=json.loads(json.dumps(oldmanifest))
matched=[j for j in manifest['imports'] if j['source']=='reference-documents'];assert len(matched)==1 and matched[0]['path']==str(oldbundle.relative_to(ROOT/'data'));matched[0]['path']=str(bundle.relative_to(ROOT/'data'))
manifest['imports'].append({'source':'curated','path':str(mergedp.relative_to(ROOT/'data'))})
prov=raw/'ingrasys-servers-reconstructed-2026-10-04-provenance.json'
register=[mergedp,bundle,prov,pdf,originaltext,envelope]+[ROOT/n for n in a['files'] if n.startswith('data/')]
for p in register:manifest['files'][str(p.relative_to(ROOT/'data'))]=sha(p)
expected=manifest['expected'];expected.update({'devices':4090,'records':5317,'documents':57,'document_pages':3661});expected['sources']=dict(con.execute('SELECT id,records_seen FROM sources'));assert len(expected['sources'])==287
assert all(manifest['files'][k]==v for k,v in oldmanifest['files'].items());dump(ROOT/'data/import-manifest.json',manifest)
dump(O/'stage-result.json',{'phase':'staged; no promotion','accepted_freeze_sha256':sha(B/'acceptance.json'),'inputs_provenance':{str(p.relative_to(ROOT)):sha(p) for p in register},'candidate':str(candidate),'candidate_sha256':sha(candidate),'live_sha256':sha(ROOT/'data/devices.sqlite'),'manifest_sha256':sha(ROOT/'data/import-manifest.json'),'counts':counts,'import_results':result,'manifest_jobs':len(manifest['imports']),'manifest_inputs':len(manifest['files']),'old_manifest_inputs_preserved':len(oldmanifest['files']),'complete_bundle_old_entries_preserved':45,'complete_bundle_entries':46,'reused_test_result':json.loads((B/'engineering/test-results.json').read_text()),'formatting_transformation':correction})
print(json.dumps({'counts':counts,'jobs':len(manifest['imports']),'inputs':len(manifest['files']),'candidate_sha256':sha(candidate)},indent=2))
