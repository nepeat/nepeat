"""Final suite/freeze/helper proof bindings; no helper execution or shared writes."""
from pathlib import Path
import ast,datetime,hashlib,json
ROOT=Path.cwd();B=ROOT/'research/batches/discovery-2026-10-12';I=B/'integration-wave3';R=B/'review'
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1<<20),b''):h.update(b)
 return h.hexdigest()
freeze=json.loads((I/'code-under-test.json').read_text());assert len(freeze['files'])==209
for name,h in freeze['files'].items():assert sha(ROOT/name)==h,name
suitepath=ROOT/'research/batches/campaign-c2-2026-10-05/acceptance/discovery-2026-10-12-curated-wave3-suite-result.json'
suite=json.loads(suitepath.read_text());assert suite['passed'] is True and suite['tests']==791 and suite['seconds']==783.32
for key in ['failures','errors','skipped','blocked_network_or_external_process_calls']:assert suite[key]==0
assert freeze['suite_label']=='discovery-2026-10-12-curated-wave3'
log=(I/'full-suite.log').read_text();assert 'Ran 791 tests in 783.248s' in log and '\nOK\n' in log
assert json.loads(log.strip().splitlines()[-1])==suite
trace=suitepath.with_name('discovery-2026-10-12-curated-wave3-network-attempts.jsonl');assert not trace.exists() or not trace.read_bytes()
assert 'Ran 15 tests in 11.232s' in (R/'shopping-python-tests.log').read_text() and '\nOK\n' in (R/'shopping-python-tests.log').read_text()
js=(R/'shopping-js-tests.log').read_text();assert '# tests 7' in js and '# pass 7' in js and '# fail 0' in js and '# skipped 0' in js
for name in ['curated-import-fix-review.md','shopping-query-fix-review.md','wave3-stage-review.md','oracle7420-facts-review.md','psu-edge-facts-review.md']:assert 'accept' in (R/name).read_text().lower()
promote=ast.parse((I/'promote.py').read_text());main=next(n for n in promote.body if isinstance(n,ast.FunctionDef) and n.name=='main')
required_node=next(n.value for n in main.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='required' for t in n.targets))
required=eval(compile(ast.Expression(required_node),'mandatory-proof-set','eval'),{'OUT':I,'ROOT':ROOT,'str':str})
assert len(required)==20
extras=[I/'full-suite.log',I/'rebuild.log',ROOT/'research/batches/campaign-c2-2026-10-05/acceptance/run_offline_suite.py',R/'curated-fix-independent-focused.log',R/'curated-fix-independent-probes.json',R/'wave3-integration-final-stage-check.json',R/'wave3-integration-final-comparison-check.json']
paths=sorted(required|{str(p.relative_to(ROOT)) for p in extras})
proofs={}
for name in paths:
 rel=Path(name);assert not rel.is_absolute() and '..' not in rel.parts;path=(ROOT/rel).resolve();assert path.is_relative_to(ROOT);proofs[name]=sha(path)
for name in ['stage.py','stage-r2.py','promote.py','refresh_exports.py','check_queries.py']:compile((I/name).read_text(),str(I/name),'exec')
assert sha(I/'promote.py')==json.loads((R/'wave3-integration-preparatory-guards.json').read_text())['promote_sha256']
q=json.loads((I/'model-query-check.json').read_text());stage=json.loads((I/'stage-report.json').read_text());assert q['candidate_sha256_unchanged']==stage['candidate_database_sha256']
assert {m:len(v['specifications']) for m,v in q['queries'].items()}=={'SD6200':9,'Wedge 100-32X':8,'Sun ZFS Storage 7420':9,'Sun ZFS Storage 7420 M2':1}
for n,h in stage['live_unchanged'].items():assert sha(ROOT/'data'/n)==h
assert sha(ROOT/'data/devices-candidate-wave3-2026-10-12.sqlite')==stage['candidate_database_sha256']
assert sha(ROOT/'data/import-manifest-candidate-wave3-2026-10-12.json')==stage['candidate_manifest_sha256']
for n in ['promotion-result.json','promotion-journal.json']:assert not (I/n).exists()
for suffix in ['-journal','-wal','-shm']:assert not (ROOT/'data'/('devices.sqlite'+suffix)).exists()
for path in ['devices-before-wave3-2026-10-12.sqlite','import-manifest-before-wave3-2026-10-12.json','.devices-wave3-2026-10-12.pending','.manifest-wave3-2026-10-12.pending']:assert not (ROOT/'data'/path).exists()
assert not (I/'exports-before').exists();assert not (ROOT/'data/exports/wave3-specifications-2026-10-12.json').exists()
report={'status':'PASS final proof bindings; promotion not executed','checked_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'suite':suite,'frozen_files_verified':209,'mandatory_proof_count':20,'recommended_receipt_proof_hashes':proofs,'review_path_for_separate_final_hash':str((R/'wave3-integration-review.md').relative_to(ROOT)),'candidate_database_sha256':stage['candidate_database_sha256'],'candidate_manifest_sha256':stage['candidate_manifest_sha256'],'live_unchanged':stage['live_unchanged'],'fresh_backup_pending_and_export_paths':True,'limits':'Read-only proof checks; helper syntax compilation and mandatory-set AST expression only, no helper execution, import, suite or rebuild.'}
with (R/'wave3-integration-final-proof-check.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print('PASS791 guarded /209freeze /20mandatory +7supporting proof hashes; stage/live pair unchanged')
