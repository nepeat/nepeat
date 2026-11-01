"""Read-only independent stage audit; no importer or rebuild execution."""
from pathlib import Path
import json,hashlib,sqlite3,datetime
from devicehunt import reference_documents,rebuild,review_queue,validation
ROOT=Path.cwd();B=ROOT/'research/batches/discovery-2026-10-12';DATA=ROOT/'data';OUT=B/'review/wave3-integration-final-stage-check.json'
def sha(path):
 h=hashlib.sha256()
 with Path(path).open('rb') as f:
  for b in iter(lambda:f.read(1<<20),b''):h.update(b)
 return h.hexdigest()
report=json.loads((B/'integration-wave3/stage-report.json').read_text());oldcode=json.loads((B/'integration-wave3/old-code-under-test.json').read_text());design=json.loads((B/'curated-import-design/production-code-sha256.json').read_text())
started=datetime.datetime.now(datetime.timezone.utc).isoformat();db=DATA/'devices-candidate-wave3-2026-10-12.sqlite';mp=DATA/'import-manifest-candidate-wave3-2026-10-12.json';live_before={n:sha(DATA/n) for n in report['live_unchanged']};assert live_before==report['live_unchanged'];assert sha(db)==report['candidate_database_sha256'];assert sha(mp)==report['candidate_manifest_sha256']
for name,h in report['old_code_hashes'].items():assert h==oldcode['hashes'][name]==oldcode['design_profile']['files'][name]==design['files'][name]
current_code={name:sha(ROOT/name) for name in design['files']}
root,manifest=rebuild.read_manifest(mp);live_manifest=json.loads((DATA/'import-manifest.json').read_text());assert manifest['campaign_gate']==live_manifest['campaign_gate'];assert all(manifest['files'][k]==v for k,v in live_manifest['files'].items());assert len(manifest['files'])-len(live_manifest['files'])==13
assert set(report['new_input_hashes'])==set(manifest['files'])-set(live_manifest['files'])
for name,h in report['new_input_hashes'].items():assert sha(DATA/name)==h==manifest['files'][name]
for group,wanted in [('psu-edge-facts','0fbf35eb2c9e35364ca915cdba6c4622ae6cd7ae037e9955376576d38d7d03ab'),('oracle7420-facts','a82e27385459ddafbe54d510d73959c237bad98b743a7cbd4c97f32032c5b25c')]:
 src=B/'original-vendor-wave3'/group;dest=DATA/'raw'/f'{group}-wave3-2026-10-12';assert sha(src/'candidate.json')==sha(dest/'candidate.json')==wanted;assert wanted in (B/'review'/f'{group}-review.md').read_text();assert 'accept' in (B/'review'/f'{group}-review.md').read_text().lower()
 for source in json.loads((src/'candidate.json').read_text())['sources']:
  for field in ['snapshot','text_sidecar']:
   if field in source:assert (src/source[field]).read_bytes()==(dest/source[field]).read_bytes()
oldbundle=DATA/'raw/reference-documents-with-psu-2026-10-12.json';newbundle=DATA/'raw/reference-documents-with-wave3-2026-10-12.json';old=json.loads(oldbundle.read_text());new=json.loads(newbundle.read_text());assert len(old['documents'])==55 and len(new['documents'])==57 and new['documents'][:55]==old['documents']
_,prepared=reference_documents.prepare(newbundle);assert len(prepared)==57 and sum(len(pages) for _,pages in prepared)==3710
for entry,pages in prepared:
 for field in ['pdf','text']:
  dep=entry[field];assert Path(dep['path']).name==dep['path'];assert manifest['files']['raw/'+dep['path']]==dep['sha256']==sha(newbundle.parent/dep['path'])
for e in new['documents'][55:]:
 orig=B/'original-vendor-wave3/psu-edge-facts/snapshots'
 for field in ['pdf','text']:
  name=e[field]['path'].removeprefix('wave3-2026-10-12-');assert (orig/name).read_bytes()==(newbundle.parent/e[field]['path']).read_bytes()
expected_jobs=json.loads(json.dumps(live_manifest['imports']))
for j in expected_jobs:
 if j['source']=='reference-documents':assert j['path']=='raw/reference-documents-with-psu-2026-10-12.json';j['path']='raw/reference-documents-with-wave3-2026-10-12.json'
expected_jobs.extend([{'source':'curated','path':'raw/psu-edge-facts-wave3-2026-10-12/candidate.json'},{'source':'curated','path':'raw/oracle7420-facts-wave3-2026-10-12/candidate.json'}]);assert manifest['imports']==expected_jobs
release=review_queue.gate_inputs(DATA,manifest)
c=sqlite3.connect(db.as_uri()+'?mode=ro',uri=True);c.row_factory=sqlite3.Row;c.execute('ATTACH DATABASE ? AS live',((DATA/'devices.sqlite').as_uri()+'?mode=ro',));c.execute('PRAGMA query_only=ON')
review_queue.gate_projections(c,release)
counts={n:c.execute('SELECT count(*) FROM '+n).fetchone()[0] for n in ('devices','records','facts','sources','documents','document_pages')};assert counts==report['counts'];assert dict(c.execute('SELECT id,records_seen FROM sources'))==manifest['expected']['sources']
changes=[dict(x) for x in c.execute('SELECT l.id,l.sha256 AS old_sha256,m.sha256 AS new_sha256 FROM live.sources l JOIN main.sources m ON m.id=l.id WHERE l.sha256 IS NOT m.sha256 ORDER BY l.id')];assert changes==report['expected_complete_bundle_source_hash_changes'];assert {x['id'] for x in changes}=={'reference-pdf-'+x['id'] for x in old['documents']};assert all(x['old_sha256']==sha(oldbundle) and x['new_sha256']==sha(newbundle) for x in changes)
preservation={}
for (table,) in c.execute("SELECT name FROM live.sqlite_master WHERE type='table'").fetchall():
 if table.startswith(('sqlite_','search_index','document_search')):continue
 cols=[x[1] for x in c.execute('PRAGMA main.table_info("'+table+'")') if x[1] not in ('observed_at','fetched_at','cache_path')]
 if table=='sources':cols.remove('sha256')
 fields=','.join('"'+x+'"' for x in cols);sql=f'SELECT {fields}, count(*) AS multiplicity FROM {{}}."{table}" GROUP BY {fields}'
 preservation[table]=c.execute('SELECT count(*) FROM ('+sql.format('live')+' EXCEPT '+sql.format('main')+')').fetchone()[0]
assert not any(preservation.values()) and preservation==report['preservation_missing_rows']
integrity=[x[0] for x in c.execute('PRAGMA integrity_check')];fk=len(c.execute('PRAGMA foreign_key_check').fetchall());assert integrity==['ok'] and fk==0
baseline=validation.run(c,ROOT/'validation/owned-hardware.json')['summary'];assert baseline==report['baseline']=={'total':10,'hits':10,'gaps':0};vendor=validation.run(c,ROOT/'validation/vendor-examples.json')['summary'];assert vendor['hits']==0
new_sources=['celestica-sd6200-datasheet-2026-10-12','edgecore-wedge100-32x-r04-2026-10-12','oracle7420-internal-components-2026-10-12','oracle7420-attached-storage-2026-10-12']
actual_new={sid:{'records':c.execute('SELECT count(*) FROM records WHERE source_id=?',(sid,)).fetchone()[0],'facts':c.execute('SELECT count(*) FROM facts f JOIN records r ON r.id=f.record_id WHERE r.source_id=?',(sid,)).fetchone()[0]} for sid in new_sources};assert actual_new=={new_sources[0]:{'records':1,'facts':11},new_sources[1]:{'records':1,'facts':16},new_sources[2]:{'records':2,'facts':11},new_sources[3]:{'records':1,'facts':1}}
assert c.execute('SELECT count(*) FROM identifiers i JOIN records r ON r.id=i.record_id WHERE r.source_id IN (?,?,?,?)',new_sources).fetchone()[0]==0
roles=[tuple(r) for r in c.execute('SELECT DISTINCT a.company,a.role FROM affiliations a JOIN records r ON r.id=a.record_id WHERE r.source_id IN (?,?,?,?) ORDER BY a.company',new_sources)];assert all(r[1]=='documentation_vendor' for r in roles)
models=[tuple(r) for r in c.execute('SELECT DISTINCT d.brand,d.model FROM devices d JOIN records r ON r.device_id=d.id WHERE r.source_id IN (?,?,?,?) ORDER BY d.brand,d.model',new_sources)];assert models==[('Celestica','SD6200'),('Edge-Core','Wedge 100-32X'),('Oracle','Sun ZFS Storage 7420'),('Oracle','Sun ZFS Storage 7420 M2')]
c.close();live_after={n:sha(DATA/n) for n in live_before};assert live_before==live_after;assert sha(db)==report['candidate_database_sha256'] and sha(mp)==report['candidate_manifest_sha256']
proof={'verdict':'PASS final read-only stage preservation/pin/gate recheck','started_utc':started,'finished_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'stage_report_sha256':sha(B/'integration-wave3/stage-report.json'),'candidate_database_sha256':sha(db),'candidate_manifest_sha256':sha(mp),'old_code_hashes_bound':report['old_code_hashes'],'current_code_hash_observation':current_code,'manifest_all_input_hashes_verified':len(manifest['files']),'manifest_jobs':len(manifest['imports']),'new_pins_verified':13,'complete_bundle_entries':57,'complete_bundle_pages':3710,'old_entries_unchanged':55,'old_bundle_sha256':sha(oldbundle),'new_bundle_sha256':sha(newbundle),'counts':counts,'new_source_counts':actual_new,'models':models,'roles':roles,'preservation_missing_rows':preservation,'allowed_reference_source_hash_changes':len(changes),'integrity':integrity,'foreign_key_violations':fk,'owned_baseline':baseline,'vendor_examples':vendor,'campaign_gate':'input and projection checks pass','live_unchanged':live_after,'limits':'No imports/rebuild/full suite/promotion; old-code staging only. Exact original artifacts untouched.'}
OUT.write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps({k:v for k,v in proof.items() if k not in ('preservation_missing_rows','models','roles')},indent=2))
