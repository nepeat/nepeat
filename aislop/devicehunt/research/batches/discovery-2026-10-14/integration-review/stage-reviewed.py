"""Stage accepted, new-source-only bulk candidates; no live writes or network."""
from pathlib import Path
from contextlib import closing
import hashlib,json,shutil,sqlite3,sys
ROOT=Path(__file__).resolve().parents[4];sys.path.insert(0,str(ROOT))
from devicehunt.catalog import Catalog
from devicehunt.importers import prepare_curated,curated
from devicehunt import rebuild,review_queue,validation
OUT=Path(__file__).resolve().parent;DATA=ROOT/'data'
CAND_DB=DATA/'devices-candidate-breadth-2026-10-14.sqlite'
CAND_MANIFEST=DATA/'import-manifest-candidate-breadth-2026-10-14.json'
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1048576),b''):h.update(b)
 return h.hexdigest()
def save(p,value):
 with p.open('x') as f:json.dump(value,f,indent=2,ensure_ascii=False);f.write('\n')
def clone(src,dst):
 assert not dst.exists(),dst;dst.parent.mkdir(parents=True,exist_ok=True)
 with src.open('rb') as a,dst.open('xb') as b:shutil.copyfileobj(a,b)
 assert sha(src)==sha(dst)
def checked_path(base,name):
 rel=Path(name);assert not rel.is_absolute() and '..' not in rel.parts,name
 p=(base/rel).resolve();assert p.is_relative_to(base.resolve());return p

def main():
 plan=json.loads((OUT/'accepted-inputs.json').read_text())
 assert plan['accepted'] is True and not CAND_DB.exists() and not CAND_MANIFEST.exists()
 before={n:sha(DATA/n) for n in ('devices.sqlite','import-manifest.json')};assert before==plan['live_hashes']
 freeze=json.loads((ROOT/'research/batches/discovery-2026-10-12/integration-wave5/code-under-test.json').read_text())['files']
 assert all(sha(ROOT/n)==h for n,h in freeze.items())
 manifest=json.loads((DATA/'import-manifest.json').read_text());release=review_queue.gate_inputs(DATA,manifest)
 source_ids=set();records=0;facts=0;inputs=[];staged=[]
 with closing(sqlite3.connect((DATA/'devices.sqlite').as_uri()+'?mode=ro',uri=True)) as db:old_ids={r[0] for r in db.execute('SELECT id FROM sources')}
 for group in plan['groups']:
  src=checked_path(ROOT,group['candidate']);assert sha(src)==group['sha256']
  review=checked_path(ROOT,group['review']);assert sha(review)==group['review_sha256']
  prepare_curated(src);payload=json.loads(src.read_text());ids=[s['id'] for s in payload['sources']]
  assert len(ids)==len(set(ids)) and not source_ids.intersection(ids) and not old_ids.intersection(ids)
  assert all(d['source'] in ids for d in payload['devices']);source_ids.update(ids)
  records+=len(payload['devices']);facts+=sum(len(d.get('facts',[])) for d in payload['devices'])
  dest_dir=DATA/'raw'/('breadth-2026-10-14-'+group['name']);assert not dest_dir.exists();dest_dir.mkdir()
  dest=dest_dir/'candidate.json';clone(src,dest);inputs.append(dest);staged.append(dest)
  paths={}
  for s in payload['sources']:
   assert s.get('snapshot'),'Every bulk source must have a capture'
   for field in ('snapshot','text_sidecar'):
    if field in s:
     original=checked_path(src.parent,s[field]);paths[s[field]]=original
     if field=='snapshot':assert sha(original)==s['original_sha256']
   for dep in s.get('evidence_dependencies',[]):
    original=checked_path(src.parent,dep['path']);assert sha(original)==dep['sha256'];paths[dep['path']]=original
  for relative,original in paths.items():
   target=checked_path(dest_dir,relative);clone(original,target);inputs.append(target)
  prepare_curated(dest)
 with closing(sqlite3.connect((DATA/'devices.sqlite').as_uri()+'?mode=ro',uri=True)) as src,closing(sqlite3.connect(CAND_DB)) as dst:src.backup(dst)
 c=Catalog(CAND_DB)
 try:
  results={str(p.relative_to(DATA)):curated(c,p) for p in staged}
 finally:c.db.close()
 manifest['imports'].extend({'source':'curated','path':str(p.relative_to(DATA))} for p in staged)
 for p in inputs:
  k=str(p.relative_to(DATA));assert k not in manifest['files'];manifest['files'][k]=sha(p)
 with closing(sqlite3.connect(CAND_DB.as_uri()+'?mode=ro',uri=True)) as db:
  db.row_factory=sqlite3.Row;db.execute('ATTACH DATABASE ? AS old',((DATA/'devices.sqlite').as_uri()+'?mode=ro',))
  preservation={}
  for row in db.execute("SELECT name FROM old.sqlite_master WHERE type='table'").fetchall():
   table=row[0]
   if table.startswith(('sqlite_','search_index','document_search')):continue
   cols=[r[1] for r in db.execute(f'PRAGMA main.table_info("{table}")') if r[1] not in ('observed_at','fetched_at','cache_path')]
   fields=','.join('"'+c+'"' for c in cols);query=f'SELECT {fields},count(*) AS multiplicity FROM {{}}."{table}" GROUP BY {fields}'
   preservation[table]=db.execute('SELECT count(*) FROM ('+query.format('old')+' EXCEPT '+query.format('main')+')').fetchone()[0]
  assert not any(preservation.values()),preservation
  counts={n:db.execute('SELECT count(*) FROM '+n).fetchone()[0] for n in ('devices','records','facts','sources','documents','document_pages')}
  for n,delta in [('records',records),('facts',facts),('sources',len(source_ids))]:assert counts[n]==db.execute('SELECT count(*) FROM old.'+n).fetchone()[0]+delta
  for n in ('documents','document_pages'):assert counts[n]==db.execute('SELECT count(*) FROM old.'+n).fetchone()[0]
  assert [r[0] for r in db.execute('PRAGMA integrity_check')]==['ok'];assert not db.execute('PRAGMA foreign_key_check').fetchall()
  baseline=validation.run(db,ROOT/'validation/owned-hardware.json');assert baseline['summary']=={'total':10,'hits':10,'gaps':0}
  review_queue.gate_projections(db,release)
  manifest['expected'].update({k:counts[k] for k in ('devices','records','documents','document_pages')})
  manifest['expected']['sources']=dict(db.execute('SELECT id,records_seen FROM sources'))
 save(CAND_MANIFEST,manifest);rebuild.read_manifest(CAND_MANIFEST)
 assert before=={n:sha(DATA/n) for n in before};assert all(sha(ROOT/n)==h for n,h in freeze.items())
 report={'counts':counts,'new_sources':sorted(source_ids),'new_records':records,'new_facts':facts,'import_results':results,'preservation_missing_rows':preservation,'integrity':'ok','foreign_keys':0,'baseline':baseline['summary'],'campaign_gate':'inputs and projections pass','manifest_jobs':len(manifest['imports']),'manifest_inputs':len(manifest['files']),'live_unchanged':before,'candidate_database_sha256':sha(CAND_DB),'candidate_manifest_sha256':sha(CAND_MANIFEST),'new_input_hashes':{str(p.relative_to(DATA)):sha(p) for p in inputs},'frozen_code_files':len(freeze),'acceptance_plan_sha256':sha(OUT/'accepted-inputs.json')}
 save(OUT/'stage-report.json',report);print(json.dumps(report,indent=2))
if __name__=='__main__':main()
