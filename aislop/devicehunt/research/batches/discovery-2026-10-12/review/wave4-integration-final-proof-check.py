"""Completed wave4 proof/freeze bindings; no staging/promote/export/import execution."""
from pathlib import Path
import ast,datetime,hashlib,json
ROOT=Path.cwd();B=ROOT/'research/batches/discovery-2026-10-12';I=B/'integration-wave4';R=B/'review';DATA=ROOT/'data'
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for block in iter(lambda:f.read(1<<20),b''):h.update(block)
 return h.hexdigest()
freeze=json.loads((I/'code-under-test.json').read_text());oldfreeze=json.loads((B/'integration-wave3/code-under-test.json').read_text());assert freeze==oldfreeze and len(freeze['files'])==209
for name,h in freeze['files'].items():assert sha(ROOT/name)==h
preparatory=json.loads((R/'wave4-integration-preparatory-check.json').read_text())
for name,h in preparatory['helper_hashes'].items():assert sha(ROOT/name)==h
suitepath=ROOT/'research/batches/campaign-c2-2026-10-05/acceptance/discovery-2026-10-12-curated-wave3-suite-result.json';suite=json.loads(suitepath.read_text());assert suite['tests']==791 and suite['passed'] and all(suite[k]==0 for k in ['failures','errors','skipped','blocked_network_or_external_process_calls'])
previous_accept=json.loads((B/'integration-wave3/promotion-acceptance.json').read_text());assert sha(suitepath)==previous_accept['proof_hashes'][str(suitepath.relative_to(ROOT))]
logpath=B/'integration-wave3/full-suite.log';assert json.loads(logpath.read_text().strip().splitlines()[-1])==suite;assert 'Ran 791 tests' in logpath.read_text()
main=next(n for n in ast.parse((I/'promote.py').read_text()).body if isinstance(n,ast.FunctionDef) and n.name=='main')
value=next(n.value for n in main.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='required' for t in n.targets))
required=eval(compile(ast.Expression(value),'mandatory-proofs','eval'),{'OUT':I,'ROOT':ROOT,'str':str});assert len(required)==22
extras=[logpath,I/'rebuild.log',I/'check-queries.log',I/'compare-prefix-error.log',ROOT/'research/batches/campaign-c2-2026-10-05/acceptance/run_offline_suite.py',R/'curated-fix-independent-focused.log',R/'curated-fix-independent-probes.json',R/'wave4-integration-final-stage-check.json',R/'wave4-integration-final-comparison-check.json',R/'wave4-integration-preparatory-check.json']
proofs={}
for name in sorted(required|{str(p.relative_to(ROOT)) for p in extras}):
 rel=Path(name);assert not rel.is_absolute() and '..' not in rel.parts;p=(ROOT/rel).resolve();assert p.is_relative_to(ROOT);proofs[name]=sha(p)
assert 'All 52 tables/indexes and baseline results match.' in (I/'compare.log').read_text();reb=json.loads((I/'rebuild.log').read_text().strip().splitlines()[-1]);assert reb['verified_files']==3970 and reb['devices']==5122 and reb['records']==7261 and reb['sources']==2115 and reb['documents']==70 and reb['document_pages']==3798
error=(I/'compare-prefix-error.log').read_text();assert 'Output prefix must contain only letters, digits, hyphens or underscores' in error
compare_script=(ROOT/'scripts/compare_rebuild.py').read_text();assert compare_script.index('if not args.output_prefix')<compare_script.index('db=sqlite3.connect')
q=json.loads((I/'model-query-check.json').read_text());assert len(q['all_new_records_checked'])==20 and sum(r['facts'] for r in q['all_new_records_checked'])==392
qlog=json.loads((I/'check-queries.log').read_text().strip().splitlines()[-1]);assert qlog['records']==20 and qlog['facts']==392 and qlog['spec_counts']=={m:len(v['specifications']) for m,v in q['queries'].items()}
stage=json.loads((I/'stage-report.json').read_text());assert q['candidate_sha256_unchanged']==stage['candidate_database_sha256']
assert sha(DATA/'devices-candidate-wave4-2026-10-12.sqlite')==stage['candidate_database_sha256'];assert sha(DATA/'import-manifest-candidate-wave4-2026-10-12.json')==stage['candidate_manifest_sha256']
for name,h in stage['live_unchanged'].items():assert sha(DATA/name)==h
for name in ['promote.py','refresh_exports.py','check_queries.py','stage.py']:compile((I/name).read_text(),str(I/name),'exec')
for name in ['promotion-result.json','promotion-journal.json']:assert not (I/name).exists()
for path in ['devices-before-wave4-2026-10-12.sqlite','import-manifest-before-wave4-2026-10-12.json','.devices-wave4-2026-10-12.pending','.manifest-wave4-2026-10-12.pending']:assert not (DATA/path).exists()
for suffix in ['-journal','-wal','-shm']:assert not (DATA/('devices.sqlite'+suffix)).exists()
assert not (I/'exports-before').exists() and not (DATA/'exports/wave4-specifications-2026-10-12.json').exists()
report={'status':'PASS completed final wave4 proof bindings; promotion unexecuted','checked_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'code_files_verified':209,'suite_reused_unchanged_code':suite,'helper_hashes':preparatory['helper_hashes'],'mandatory_count':22,'recommended_receipt_proof_hashes':proofs,'review_path_for_separate_final_hash':str((R/'wave4-integration-review.md').relative_to(ROOT)),'stage_pair':{'database':stage['candidate_database_sha256'],'manifest':stage['candidate_manifest_sha256']},'live_pair_unchanged':stage['live_unchanged'],'rebuild_verified_inputs':3970,'six_model_spec_counts':qlog['spec_counts'],'corrected_comparator_first_rejection_before_db_access':True,'fresh_backup_pending_export_paths':True,'limits':'Read-only completed proofs and guard AST/syntax only; no imports, suite/rebuild/promotion/export execution or shared writes.'}
with (R/'wave4-integration-final-proof-check.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print('PASS209freeze/791accepted suite/22mandatory+10supporting/3970rebuildpins; live pair unchanged')
