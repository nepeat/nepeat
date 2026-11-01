"""Inspect completed stage/query and unexecuted corrected helpers; no incomplete rebuild proof."""
from pathlib import Path
import ast,datetime,hashlib,json
from devicehunt.catalog import Catalog
from devicehunt import specifications
ROOT=Path.cwd();B=ROOT/'research/batches/discovery-2026-10-12';I=B/'integration-wave4';R=B/'review';DATA=ROOT/'data'
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1<<20),b''):h.update(b)
 return h.hexdigest()
helpers=['stage.py','promote.py','refresh_exports.py','check_queries.py'];hashes={str((I/name).relative_to(ROOT)):sha(I/name) for name in helpers}
for name in helpers:compile((I/name).read_text(),str(I/name),'exec')
code=json.loads((I/'code-under-test.json').read_text());assert code==json.loads((B/'integration-wave3/code-under-test.json').read_text()) and len(code['files'])==209
for name,h in code['files'].items():assert sha(ROOT/name)==h
promote=(I/'promote.py').read_text();old=(I/'promote.py.before-review-fixes').read_text()
assert 'for path in [dest for _, dest in backups] + [pending_db, pending_manifest]:' in promote
assert promote.index('assert not path.exists(), path')<promote.index('for source, dest in backups:')
export=(I/'refresh_exports.py').read_text();oldexport=(I/'refresh_exports.py.before-review-fixes').read_text()
assert export==oldexport.replace('specifications.search(c, model, exact_model=True, limit=-1)','specifications.search(c, model, exact_model=True, include_source_facts=True, limit=-1)')
assert 'specs = specifications.search(c, limit=-1)' in export
main=next(n for n in ast.parse(promote).body if isinstance(n,ast.FunctionDef) and n.name=='main')
value=next(n.value for n in main.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='required' for t in n.targets))
required=eval(compile(ast.Expression(value),'required','eval'),{'OUT':I,'ROOT':ROOT,'str':str});assert len(required)==22
assert str((R/'wave4-stage-review.md').relative_to(ROOT)) in required
stage=json.loads((I/'stage-report.json').read_text());db=DATA/'devices-candidate-wave4-2026-10-12.sqlite';before=sha(db);assert before==stage['candidate_database_sha256']
query=json.loads((I/'model-query-check.json').read_text());assert query['candidate_sha256_unchanged']==before and len(query['all_new_records_checked'])==20 and sum(r['facts'] for r in query['all_new_records_checked'])==392
c=Catalog(db,read_only=True)
try:
 counts={};origins={}
 for model,data in query['queries'].items():
  actual=specifications.search(c,model,exact_model=True,include_source_facts=True,limit=-1);assert actual==data['specifications'];counts[model]=len(actual);origins[model]=sorted({r.get('origin') for r in actual})
finally:c.db.close()
assert counts=={'Minipack-AS8000':11,'SC6100':11,'Inspur NF5170M4':3,'Inspur NF5180M4':3,'Inspur NF5270M4':3,'Inspur NF5488M5':0}
assert before==sha(db)
for name,h in stage['live_unchanged'].items():assert sha(DATA/name)==h
for p in ['devices-before-wave4-2026-10-12.sqlite','import-manifest-before-wave4-2026-10-12.json','.devices-wave4-2026-10-12.pending','.manifest-wave4-2026-10-12.pending']:assert not (DATA/p).exists()
assert not (I/'promotion-result.json').exists() and not (I/'exports-before').exists() and not (DATA/'exports/wave4-specifications-2026-10-12.json').exists()
assert hashes=={str((I/name).relative_to(ROOT)):sha(I/name) for name in helpers}
proof={'status':'PENDING final rebuild/comparison; corrected helper findings resolved','checked_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'helper_hashes':hashes,'preserved_before_fix_hashes':{str((I/name).relative_to(ROOT)):sha(I/name) for name in ['promote.py.before-review-fixes','refresh_exports.py.before-review-fixes']},'mandatory_proof_paths':sorted(required),'mandatory_count':22,'required_missing_but_pending':[n for n in sorted(required) if not (ROOT/n).exists()],'code_freeze_verified':209,'corrected_fresh_backup_pending_guard':True,'dated_source_fact_options_match_query_checker':True,'six_model_spec_counts':counts,'six_model_origins':origins,'candidate_sha256_unchanged':before,'live_pair_unchanged':stage['live_unchanged'],'limits':'Only completed stage/query results and helper code inspected. Rebuild/comparator outcomes intentionally not inspected while running; no helper, imports, suite or rebuild executed.'}
with (R/'wave4-integration-preparatory-check.json').open('x') as f:json.dump(proof,f,indent=2);f.write('\n')
print('PASS corrected helper/query/freeze checks; PENDING completed rebuild/final review')
