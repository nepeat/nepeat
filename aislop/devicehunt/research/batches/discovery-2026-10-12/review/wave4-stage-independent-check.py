"""Read-only actual-stage independent audit, no stage/import/rebuild execution."""
from pathlib import Path
from collections import Counter
import datetime,hashlib,json,sqlite3
from devicehunt import rebuild,reference_documents,review_queue,validation
ROOT=Path.cwd();B=ROOT/'research/batches/discovery-2026-10-12';I=B/'integration-wave4';DATA=ROOT/'data';R=B/'review'
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for block in iter(lambda:f.read(1<<20),b''):h.update(block)
 return h.hexdigest()
report=json.loads((I/'stage-report.json').read_text());db=DATA/'devices-candidate-wave4-2026-10-12.sqlite';mp=DATA/'import-manifest-candidate-wave4-2026-10-12.json'
assert sha(db)==report['candidate_database_sha256']=='fc88dca7e8b6f23aa8078134be767c22fed902303c2880ee923ad1d55952e3bf'
assert sha(mp)==report['candidate_manifest_sha256']=='43e9fdf88703d979b13eef475410d6f138be90833b1fe67de72bf99abe741562'
before={str(p.relative_to(ROOT)):sha(p) for p in [db,mp,DATA/'devices.sqlite',DATA/'import-manifest.json']}
assert {n:sha(DATA/n) for n in report['live_unchanged']}==report['live_unchanged']
freeze=json.loads((I/'code-under-test.json').read_text());previous=json.loads((B/'integration-wave3/code-under-test.json').read_text());assert freeze==previous and len(freeze['files'])==209
assert report['accepted_code_hashes']==freeze['files']
for n,h in freeze['files'].items():assert sha(ROOT/n)==h
assert sha(I/'stage.py')==json.loads((R/'wave4-stage-helper-check.json').read_text())['helper_sha256']
root,manifest=rebuild.read_manifest(mp);live=json.loads((DATA/'import-manifest.json').read_text());assert manifest['campaign_gate']==live['campaign_gate']
assert all(manifest['files'][n]==h for n,h in live['files'].items());newpins=set(manifest['files'])-set(live['files']);assert len(newpins)==29 and newpins==set(report['new_input_hashes'])
assert len(manifest['files'])==3970 and len(manifest['imports'])==150
jobs=json.loads(json.dumps(live['imports']))
for j in jobs:
 if j['source']=='reference-documents':assert j['path']=='raw/reference-documents-with-wave3-2026-10-12.json';j['path']='raw/reference-documents-with-wave4-2026-10-12.json'
jobs.extend([{'source':'curated','path':'raw/as8000-sc6100-facts-r2-wave4-2026-10-12/candidate.json'},{'source':'curated','path':'raw/spec-continuation-facts-wave4-2026-10-12/candidate.json'}]);assert manifest['imports']==jobs
release=review_queue.gate_inputs(DATA,manifest)
oldpath=DATA/'raw/reference-documents-with-wave3-2026-10-12.json';newpath=DATA/'raw/reference-documents-with-wave4-2026-10-12.json';old=json.loads(oldpath.read_text());new=json.loads(newpath.read_text());assert len(old['documents'])==57 and len(new['documents'])==59 and new['documents'][:57]==old['documents'];_,ready=reference_documents.prepare(newpath);assert len(ready)==59 and sum(len(p) for _,p in ready)==3724
for e,pages in ready:
 for field in ['pdf','text']:
  dep=e[field];assert Path(dep['path']).name==dep['path'];assert manifest['files']['raw/'+dep['path']]==dep['sha256']==sha(newpath.parent/dep['path'])
c=sqlite3.connect(db.as_uri()+'?mode=ro',uri=True);c.row_factory=sqlite3.Row;c.execute('ATTACH DATABASE ? AS live',((DATA/'devices.sqlite').as_uri()+'?mode=ro',));c.execute('PRAGMA query_only=ON');review_queue.gate_projections(c,release)
counts={n:c.execute('SELECT count(*) FROM '+n).fetchone()[0] for n in report['counts']};assert counts==report['counts'];assert dict(c.execute('SELECT id,records_seen FROM sources'))==manifest['expected']['sources']
changes=[dict(r) for r in c.execute('SELECT l.id,l.sha256 AS old_sha256,m.sha256 AS new_sha256 FROM live.sources l JOIN main.sources m ON m.id=l.id WHERE l.sha256 IS NOT m.sha256 ORDER BY l.id')];assert changes==report['expected_complete_bundle_source_hash_changes'] and len(changes)==57
assert {r['id'] for r in changes}=={'reference-pdf-'+d['id'] for d in old['documents']};assert all(r['old_sha256']==sha(oldpath) and r['new_sha256']==sha(newpath) for r in changes)
preservation={}
for (t,) in c.execute("SELECT name FROM live.sqlite_master WHERE type='table'").fetchall():
 if t.startswith(('sqlite_','search_index','document_search')):continue
 cols=[r[1] for r in c.execute('PRAGMA main.table_info("'+t+'")') if r[1] not in ('observed_at','fetched_at','cache_path')]
 if t=='sources':cols.remove('sha256')
 fields=','.join('"'+col+'"' for col in cols);group=f'SELECT {fields},count(*) AS multiplicity FROM {{}}."{t}" GROUP BY {fields}'
 preservation[t]=c.execute('SELECT count(*) FROM ('+group.format('live')+' EXCEPT '+group.format('main')+')').fetchone()[0]
assert len(preservation)==50 and not any(preservation.values()) and preservation==report['preservation_missing_rows']
records=[];sourceids=[];roles=Counter();facts=0
for name,parent,wanted in [('as8000-sc6100-facts-r2',B/'original-vendor-wave4/as8000-sc6100-facts-r2','7c73ce18d7a5812f776b3bde2dbec704a45bed68c7922b3e1e4dad0db2dcaa3e'),('spec-continuation-facts',B/'spec-continuation-facts','580ff0e2aa3f46b996fc28612fe8d0489de9a8ca8fdd649948dd202ffc8b8608')]:
 dest=DATA/'raw'/f'{name}-wave4-2026-10-12';assert sha(parent/'candidate.json')==sha(dest/'candidate.json')==wanted;payload=json.loads((parent/'candidate.json').read_text())
 for s in payload['sources']:
  sourceids.append(s['id']);assert not c.execute('SELECT 1 FROM live.sources WHERE id=?',(s['id'],)).fetchone()
  stored=c.execute('SELECT * FROM sources WHERE id=?',(s['id'],)).fetchone();assert stored['sha256']==s['original_sha256'] and stored['url']==s['url'] and stored['coverage']==s['coverage']
  for field in ['snapshot','text_sidecar']:
   if field in s:assert (parent/s[field]).read_bytes()==(dest/s[field]).read_bytes()
 for d in payload['devices']:
  row=c.execute('SELECT r.*,v.brand,v.model FROM records r JOIN devices v ON v.id=r.device_id WHERE source_id=? AND source_key=?',(d['source'],d['key'])).fetchall();assert len(row)==1;row=row[0];rid=row['id'];assert row['brand']==d['brand'] and row['model'].casefold()==d['model'].casefold();assert json.loads(row['raw_json'])==d
  for table,columns,key in [('facts','name,value','facts'),('affiliations','company,role','affiliations'),('identifiers','kind,value','identifiers')]:assert Counter(tuple(r) for r in c.execute('SELECT '+columns+' FROM '+table+' WHERE record_id=?',(rid,)))==Counter(tuple(v) for v in d.get(key,[]))
  assert not c.execute('SELECT 1 FROM hyperscaler_evidence WHERE record_id=?',(rid,)).fetchone();roles.update(tuple(v) for v in d['affiliations']);facts+=len(d['facts']);records.append({'source':d['source'],'printed_key':d['key'],'stored_model':row['model'],'device_id':row['device_id'],'facts':len(d['facts'])})
assert len(records)==20 and facts==392 and len(sourceids)==20
for printed,display in [('Inspur I24M6','Inspur i24M6'),('Inspur I24LM6','Inspur i24LM6')]:
 row=c.execute('SELECT model FROM devices WHERE brand=? AND lower(model)=lower(?)',('Inspur Corporation',printed)).fetchall();assert len(row)==1 and row[0][0]==display
for e in new['documents'][57:]:
 for field in ['pdf','text']:
  src=B/'original-vendor-wave4/as8000-sc6100-facts-r2/snapshots'/e[field]['path'].removeprefix('wave4-2026-10-12-');assert src.read_bytes()==(newpath.parent/e[field]['path']).read_bytes()
integrity=[r[0] for r in c.execute('PRAGMA integrity_check')];fk=len(c.execute('PRAGMA foreign_key_check').fetchall());assert integrity==['ok'] and fk==0
baseline=validation.run(c,ROOT/'validation/owned-hardware.json')['summary'];vendor=validation.run(c,ROOT/'validation/vendor-examples.json')['summary'];assert baseline=={'total':10,'hits':10,'gaps':0} and vendor['hits']==0;c.close()
assert before=={str(p.relative_to(ROOT)):sha(p) for p in [db,mp,DATA/'devices.sqlite',DATA/'import-manifest.json']}
proof={'verdict':'ACCEPT STAGE ONLY; clean rebuild/final integration pending','checked_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'input_hashes':{str(p.relative_to(ROOT)):sha(p) for p in [I/'stage.py',I/'stage-report.json',I/'code-under-test.json',db,mp]},'all3970pins_verified':True,'manifest_jobs':150,'new_pins_verified':29,'complete_bundle_entries':59,'complete_bundle_pages':3724,'old_entries_unchanged':57,'allowed_reference_hash_changes':len(changes),'counts':counts,'records':records,'role_counts':{str(k):v for k,v in roles.items()},'preservation':preservation,'integrity':integrity,'foreign_key_violations':fk,'baseline':baseline,'vendor_examples':vendor,'campaign_gate':'input/projection pass','frozen_files':209,'all_pair_hashes_unchanged':before,'limits':'Read-only actual-stage audit only; no imports, DB/bundle writes, rebuild or suite execution.'}
with (R/'wave4-stage-independent-check.json').open('x') as f:json.dump(proof,f,indent=2);f.write('\n')
print('PASS actual stage3970pins59bundle50preservation392facts209freeze; no shared writes')
