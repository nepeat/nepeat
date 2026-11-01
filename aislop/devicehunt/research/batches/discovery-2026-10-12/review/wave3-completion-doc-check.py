"""Bounded read-only completion state/doc verification, no export/import execution."""
from pathlib import Path
import csv,datetime,hashlib,json,re,sqlite3
from devicehunt import rebuild,review_queue,validation,vendor_inventory
from devicehunt.__main__ import shopping_links
ROOT=Path.cwd();B=ROOT/'research/batches/discovery-2026-10-12';I=B/'integration-wave3';R=B/'review';DATA=ROOT/'data'
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1<<20),b''):h.update(b)
 return h.hexdigest()
proof=json.loads((I/'promotion-result.json').read_text());accept=json.loads((I/'promotion-acceptance.json').read_text());exports=json.loads((I/'exports-report.json').read_text());gate=json.loads((I/'promoted-gate-check.json').read_text());assert proof['promoted'] and accept['accepted']
assert sha(ROOT/accept['review_path'])==accept['review_sha256']==proof['review_sha256']
for name,h in accept['proof_hashes'].items():assert sha(ROOT/name)==h,name
for name,h in proof['new'].items():assert sha(DATA/name)==h
for name,h in proof['backups'].items():assert sha(ROOT/name)==h
assert proof['new']['devices.sqlite']==accept['candidate_database_sha256']==exports['database_sha256_unchanged']==gate['database_sha256_unchanged']
assert proof['new']['import-manifest.json']==accept['candidate_manifest_sha256']
assert sha(DATA/'devices-candidate-wave3-2026-10-12.sqlite')==sha(DATA/'devices.sqlite');assert sha(DATA/'import-manifest-candidate-wave3-2026-10-12.json')==sha(DATA/'import-manifest.json')
assert json.loads((I/'promotion-journal.json').read_text())=={k:v for k,v in proof.items() if k!='promoted'}
for name,meta in exports['files'].items():assert sha(ROOT/name)==meta['sha256'] and (ROOT/name).stat().st_size==meta['bytes']
assert len(exports['files'])==8
old=json.loads((I/'exports-before/provenance.json').read_text());assert len(old['sha256'])==7
for name,h in old['sha256'].items():assert sha(I/'exports-before'/name)==h
spec=json.loads((DATA/'exports/specifications.json').read_text());assert len(spec)==1285
with (DATA/'exports/specifications.csv').open(newline='') as f:assert len(list(csv.DictReader(f)))==1285
queries=json.loads((DATA/'exports/wave3-specifications-2026-10-12.json').read_text());assert set(queries)=={'SD6200','Wedge 100-32X','Sun ZFS Storage 7420','Sun ZFS Storage 7420 M2'}
html=(DATA/'exports/catalog.html').read_text();payload=json.loads(re.search(r'<script id="data" type="application/json">(.*?)</script>',html,re.S)[1]);assert len(payload)==5117
models=['NF5280M6','NF5280A7','wedge','7760-32X-A-AC-F','CS8200','SD6200']
examples=[d for d in payload if d['model'] in models];assert any(d['model']=='wedge' for d in examples)
for d in examples:assert d['shopping']==shopping_links(d)
assert 'if(d.shopping.length)' in html
freeze=json.loads((I/'code-under-test.json').read_text());assert len(freeze['files'])==209
for name,h in freeze['files'].items():assert sha(ROOT/name)==h
before={n:sha(DATA/n) for n in proof['new']};root,manifest=rebuild.read_manifest(DATA/'import-manifest.json');release=review_queue.gate_inputs(root,manifest)
c=sqlite3.connect((DATA/'devices.sqlite').as_uri()+'?mode=ro',uri=True);c.row_factory=sqlite3.Row;review_queue.gate_projections(c,release)
counts={n:c.execute('SELECT count(*) FROM '+n).fetchone()[0] for n in ['devices','records','facts','sources','documents','document_pages']};assert counts==json.loads((I/'stage-report.json').read_text())['counts']
assert validation.run(c,ROOT/'validation/owned-hardware.json')['summary']=={'total':10,'hits':10,'gaps':0};assert validation.run(c,ROOT/'validation/vendor-examples.json')['summary']['hits']==0
manufacturer=vendor_inventory.run(c,scope='manufacturer',limit=0);allroles=vendor_inventory.run(c,scope='all',limit=0)
assert (manufacturer['identity_target_pairs'],manufacturer['unique_identities'])==(2147,2146);assert (allroles['identity_target_pairs'],allroles['unique_identities'])==(4593,4450)
assert next(r['identities'] for r in manufacturer['counts_by_target'] if r['target']=='Celestica')==198
assert tuple(c.execute('SELECT count(*),count(DISTINCT r.device_id) FROM hyperscaler_evidence h JOIN records r ON r.id=h.record_id').fetchone())==(22,18)
lineage={t:c.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['archive_artifacts','archive_receipts','evidence_statements']};assert lineage=={'archive_artifacts':146,'archive_receipts':374,'evidence_statements':4}
schema=c.execute('PRAGMA user_version').fetchone()[0];c.close();assert schema==29
assert before=={n:sha(DATA/n) for n in proof['new']}
docpaths=[ROOT/'PROGRESS.md',ROOT/'docs/specification-search.md',I/'handoff.md'];links=[]
for p in docpaths:
 for link in re.findall(r'\[[^\]]+\]\(([^)]+)\)',p.read_text()):
  if link.startswith(('http:','https:','#')):continue
  target=(p.parent/link.split('#',1)[0]).resolve();assert target.exists(),(p,link);links.append({'document':str(p.relative_to(ROOT)),'link':link})
history=ROOT/'docs/history/2026-10-12-before-wave3-promotion';hp=json.loads((history/'provenance.json').read_text())
for original,copy in [('PROGRESS.md','PROGRESS.md'),('docs/specification-search.md','specification-search.md'),('research/batches/discovery-2026-10-12/integration-wave3/handoff.md','wave3-handoff.md')]:assert sha(history/copy)==hp['original_paths_sha256'][original]
report={'status':'PASS completed local wave3 state/docs','checked_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'doc_hashes':{str(p.relative_to(ROOT)):sha(p) for p in docpaths},'local_links_verified':links,'accepted_proof_hashes_verified':len(accept['proof_hashes']),'live_pair':before,'counts':counts,'schema':schema,'manifest_jobs':len(manifest['imports']),'manifest_inputs':len(manifest['files']),'export_hashes_verified':8,'prior_exports_preserved':7,'specification_rows':1285,'shopping_examples_verified':len(examples),'code_hashes_verified':209,'campaign_gate':'inputs/projections pass','lineage':lineage,'history_originals_verified':3,'limitations':'No suite/rebuild/import/export/promotion execution or shared writes.'}
with (R/'wave3-completion-doc-check.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print('PASS completion state/doc links:8exports,7backups,1285specs,209freeze,3941pins/gates')
