"""Stage reviewed wave4 and SPEC continuation with accepted frozen code; never promote."""
from contextlib import closing
from pathlib import Path
import copy
import hashlib
import json
import shutil
import sqlite3
import sys
ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT))
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt import reference_documents, rebuild, review_queue, validation
OUT = Path(__file__).resolve().parent
BATCH = OUT.parent
DATA = ROOT / 'data'
CAND_DB = DATA / 'devices-candidate-wave4-2026-10-12.sqlite'
CAND_MANIFEST = DATA / 'import-manifest-candidate-wave4-2026-10-12.json'
OLD_BUNDLE = DATA / 'raw/reference-documents-with-wave3-2026-10-12.json'
NEW_BUNDLE = DATA / 'raw/reference-documents-with-wave4-2026-10-12.json'
GROUPS = {
 'as8000-sc6100-facts-r2': ('original-vendor-wave4/as8000-sc6100-facts-r2', '7c73ce18d7a5812f776b3bde2dbec704a45bed68c7922b3e1e4dad0db2dcaa3e', 'as8000-sc6100-facts-r2-review.md'),
 'spec-continuation-facts': ('spec-continuation-facts', '580ff0e2aa3f46b996fc28612fe8d0489de9a8ca8fdd649948dd202ffc8b8608', 'spec-continuation-facts-review.md')}
EXPECTED_LIVE = {'devices.sqlite': '4ae2cd0d6a614d582cf6fc47e6ba0e022521ac2b5e73bb268fcfa0dcb87c03ea',
 'import-manifest.json': '9b4d93229e1519770a79f665e02987e9706e74cf86db086599160b905157ffec'}
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1<<20),b''):h.update(b)
 return h.hexdigest()
def save(p,body):
 with p.open('x') as f:json.dump(body,f,indent=2,ensure_ascii=False);f.write('\n')
def clone(src,dst):
 if dst.exists():
  assert sha(src)==sha(dst)
  return
 with src.open('rb') as a,dst.open('xb') as b:shutil.copyfileobj(a,b)
 assert sha(src)==sha(dst)
def main():
 for p in [CAND_DB,CAND_MANIFEST,NEW_BUNDLE,OUT/'stage-report.json']:assert not p.exists(),p
 before={n:sha(DATA/n) for n in EXPECTED_LIVE};assert before==EXPECTED_LIVE,before
 held=json.loads((BATCH/'integration-wave3/code-under-test.json').read_text())['files']
 code_before={n:sha(ROOT/n) for n in held}
 assert code_before==held
 assert json.loads((BATCH/'integration-wave3/promotion-result.json').read_text())['promoted']
 manifest=json.loads((DATA/'import-manifest.json').read_text())
 bundle=json.loads(OLD_BUNDLE.read_text());original=copy.deepcopy(bundle['documents']);assert len(original)==57
 new_inputs=[];curated=[]
 for group,(relative,wanted,review) in GROUPS.items():
  source_dir=BATCH/relative
  assert 'accept' in (BATCH/'review'/review).read_text().lower()
  source=source_dir/'candidate.json';assert sha(source)==wanted
  dest_dir=DATA/'raw'/f'{group}-wave4-2026-10-12';(dest_dir/'snapshots').mkdir(parents=True,exist_ok=True)
  dest=dest_dir/'candidate.json';clone(source,dest);new_inputs.append(dest);curated.append(dest)
  payload=json.loads(source.read_text())
  for s in payload['sources']:
   for field in ('snapshot','text_sidecar'):
    if field not in s:continue
    rel=Path(s[field]);assert not rel.is_absolute() and '..' not in rel.parts
    target=dest_dir/rel;target.parent.mkdir(parents=True,exist_ok=True);clone(source_dir/rel,target);new_inputs.append(target)
    if field=='snapshot':assert sha(target)==s['original_sha256']
   if 'text_sidecar' in s:
    pdf=DATA/'raw'/('wave4-2026-10-12-'+Path(s['snapshot']).name)
    text=DATA/'raw'/('wave4-2026-10-12-'+Path(s['text_sidecar']).name)
    clone(source_dir/s['snapshot'],pdf);clone(source_dir/s['text_sidecar'],text);new_inputs.extend([pdf,text])
    bundle['documents'].append({'id':s['id'],'url':s['url'],'title':pdf.stem+' — complete captured vendor document',
     'category':'original vendor hardware documentation','tier':s['tier'],'scope':s['coverage'],
     'page_count':s['extraction_metadata']['pdf_page_count'],
     'pdf':{'path':pdf.name,'sha256':sha(pdf)},'text':{'path':text.name,'sha256':sha(text)}})
 assert bundle['documents'][:57]==original
 save(NEW_BUNDLE,bundle);_,prepared=reference_documents.prepare(NEW_BUNDLE);assert len(prepared)==59
 new_inputs.append(NEW_BUNDLE)
 jobs=[j for j in manifest['imports'] if j['source']=='reference-documents']
 assert len(jobs)==1 and jobs[0]['path']==str(OLD_BUNDLE.relative_to(DATA))
 jobs[0]['path']=str(NEW_BUNDLE.relative_to(DATA))
 manifest['imports'].extend({'source':'curated','path':str(p.relative_to(DATA))} for p in curated)
 for p in new_inputs:
  key=str(p.relative_to(DATA));assert key not in manifest['files'];manifest['files'][key]=sha(p)
 release=review_queue.gate_inputs(DATA,manifest)
 with closing(sqlite3.connect((DATA/'devices.sqlite').as_uri()+'?mode=ro',uri=True)) as src,closing(sqlite3.connect(CAND_DB)) as dst:src.backup(dst)
 c=Catalog(CAND_DB)
 try:
  results={str(p.relative_to(DATA)):import_source(c,'curated',p) for p in curated}
  results['reference-documents']=import_source(c,'reference-documents',NEW_BUNDLE)
 finally:c.db.close()
 with closing(sqlite3.connect(CAND_DB.as_uri()+'?mode=ro',uri=True)) as db:
  db.row_factory=sqlite3.Row
  db.execute('ATTACH DATABASE ? AS live',((DATA/'devices.sqlite').as_uri()+'?mode=ro',))
  review_queue.gate_projections(db,release)
  changes=[dict(r) for r in db.execute('SELECT l.id,l.sha256 AS old_sha256,m.sha256 AS new_sha256 FROM live.sources l JOIN main.sources m ON m.id=l.id WHERE l.sha256 IS NOT m.sha256 ORDER BY l.id')]
  assert {r['id'] for r in changes}=={'reference-pdf-'+e['id'] for e in original}
  assert all(r['old_sha256']==sha(OLD_BUNDLE) and r['new_sha256']==sha(NEW_BUNDLE) for r in changes)
  preservation={}
  for (table,) in db.execute("SELECT name FROM live.sqlite_master WHERE type='table'").fetchall():
   if table.startswith(('sqlite_','search_index','document_search')):continue
   cols=[r[1] for r in db.execute('PRAGMA main.table_info("'+table+'")') if r[1] not in ('observed_at','fetched_at','cache_path')]
   if table=='sources':cols.remove('sha256')
   fields=','.join('"'+x+'"' for x in cols);query=f'SELECT {fields},count(*) AS multiplicity FROM {{}}."{table}" GROUP BY {fields}'
   preservation[table]=db.execute('SELECT count(*) FROM ('+query.format('live')+' EXCEPT '+query.format('main')+')').fetchone()[0]
  assert not any(preservation.values()),preservation
  counts={n:db.execute('SELECT count(*) FROM '+n).fetchone()[0] for n in ('devices','records','facts','sources','documents','document_pages')}
  assert counts=={'devices':5122,'records':7261,'facts':74602,'sources':2115,'documents':70,'document_pages':3798},counts
  manifest['expected'].update({k:counts[k] for k in ('devices','records','documents','document_pages')})
  manifest['expected']['sources']=dict(db.execute('SELECT id,records_seen FROM sources'))
  integrity=[r[0] for r in db.execute('PRAGMA integrity_check')];fk=db.execute('PRAGMA foreign_key_check').fetchall();assert integrity==['ok'] and not fk
  baseline=validation.run(db,ROOT/'validation/owned-hardware.json');assert baseline['summary']=={'total':10,'hits':10,'gaps':0}
 save(CAND_MANIFEST,manifest);rebuild.read_manifest(CAND_MANIFEST)
 assert before=={n:sha(DATA/n) for n in EXPECTED_LIVE}
 assert code_before=={n:sha(ROOT/n) for n in code_before}
 report={'counts':counts,'import_results':results,'preservation_missing_rows':preservation,
  'preservation_excludes':['observed_at','fetched_at','cache_path','FTS internal tables'],
  'expected_complete_bundle_source_hash_changes':changes,'integrity':integrity,'foreign_key_violations':len(fk),
  'baseline':baseline['summary'],'complete_reference_entries':len(prepared),'original_entries_unchanged':True,
  'campaign_gate':'inputs and projections pass','manifest_jobs':len(manifest['imports']),'manifest_inputs':len(manifest['files']),
  'live_unchanged':before,'candidate_manifest_sha256':sha(CAND_MANIFEST),'candidate_database_sha256':sha(CAND_DB),
  'new_input_hashes':{str(p.relative_to(DATA)):sha(p) for p in new_inputs},'accepted_code_hashes':code_before}
 save(OUT/'stage-report.json',report);print(json.dumps(report,indent=2))
if __name__=='__main__':main()
