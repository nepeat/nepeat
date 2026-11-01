"""Read completed comparison independently; never rebuild/import/promote."""
from pathlib import Path
import sqlite3,json,hashlib,datetime
from devicehunt import validation
ROOT=Path.cwd();B=ROOT/'research/batches/discovery-2026-10-12';I=B/'integration-wave3';DATA=ROOT/'data'
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1<<20),b''):h.update(b)
 return h.hexdigest()
reference=DATA/'devices-candidate-wave3-2026-10-12.sqlite';rebuilt=DATA/'rebuilt-wave3-2026-10-12.sqlite';stage=json.loads((I/'stage-report.json').read_text());assert sha(reference)==stage['candidate_database_sha256']
counts=json.loads((DATA/'exports/wave3-2026-10-12-comparison.json').read_text());vp=DATA/'exports/wave3-2026-10-12-validation.json';v=json.loads(vp.read_text());assert Path(v['reference_database']).resolve()==reference.resolve() and Path(v['database']).resolve()==rebuilt.resolve()
freeze=json.loads((I/'code-under-test.json').read_text());assert len(freeze['files'])==209
for n,h in freeze['files'].items():assert sha(ROOT/n)==h,n
before={p.name:sha(p) for p in [reference,rebuilt,DATA/'devices.sqlite',DATA/'import-manifest.json']}
c=sqlite3.connect(reference.as_uri()+'?mode=ro',uri=True);c.row_factory=sqlite3.Row;c.execute('ATTACH DATABASE ? AS rebuilt',(rebuilt.as_uri()+'?mode=ro',));c.execute('PRAGMA query_only=ON')
tables={r[0] for r in c.execute("SELECT name FROM sqlite_master WHERE type='table'") if not r[0].startswith(('sqlite_','search_index_','document_search_'))};assert tables==set(counts) and len(tables)==52 and {'search_index','document_search'}<=tables
results={}
for t in sorted(tables):
 cols=[r['name'] for r in c.execute('PRAGMA main.table_info("'+t+'")') if r['name'] not in ('observed_at','fetched_at','cache_path')]
 if t in ('search_index','document_search'):cols.insert(0,'rowid')
 fields=','.join('"'+n+'"' for n in cols);left=f'SELECT {fields},count(*) AS multiplicity FROM main."{t}" GROUP BY {fields}';right=f'SELECT {fields},count(*) AS multiplicity FROM rebuilt."{t}" GROUP BY {fields}'
 r={'original':c.execute('SELECT count(*) FROM main."'+t+'"').fetchone()[0],'rebuilt':c.execute('SELECT count(*) FROM rebuilt."'+t+'"').fetchone()[0],'only_original':c.execute('SELECT count(*) FROM ('+left+' EXCEPT '+right+')').fetchone()[0],'only_rebuilt':c.execute('SELECT count(*) FROM ('+right+' EXCEPT '+left+')').fetchone()[0]}
 assert r==counts[t] and r['only_original']==r['only_rebuilt']==0 and r['original']==r['rebuilt'];results[t]=r
base=validation.run(c,ROOT/'validation/owned-hardware.json');c.close();rdb=sqlite3.connect(rebuilt.as_uri()+'?mode=ro',uri=True);rdb.row_factory=sqlite3.Row;rb=validation.run(rdb,ROOT/'validation/owned-hardware.json');integrity=[r[0] for r in rdb.execute('PRAGMA integrity_check')];fk=len(rdb.execute('PRAGMA foreign_key_check').fetchall());rdb.close()
for key in ['summary','results','search_summary']:assert base[key]==rb[key]
assert base['summary']=={'total':10,'hits':10,'gaps':0} and integrity==['ok'] and fk==0
assert before=={p.name:sha(p) for p in [reference,rebuilt,DATA/'devices.sqlite',DATA/'import-manifest.json']}
out={'status':'COMPLETED REBUILD/COMPARISON CHECK ONLY; fullsuite and final promotion acceptance pending','checked_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'file_hashes':{str(p.relative_to(ROOT)):sha(p) for p in [reference,rebuilt,DATA/'import-manifest-candidate-wave3-2026-10-12.json',I/'stage-report.json',I/'code-under-test.json',I/'rebuild.log',I/'compare.log',DATA/'exports/wave3-2026-10-12-comparison.json',vp]},'frozen_code_hashes_verified':209,'all52_comparison':results,'baseline_equality_keys':['summary','results','search_summary'],'baseline':base['summary'],'integrity':integrity,'foreign_key_violations':fk,'all_read_files_byte_hashes_unchanged':before,'limits':'Only completed proofs reviewed, no suite execution or incomplete suite-result judgement; no rebuild/import/promotion/network/shared writes.'}
(B/'review/wave3-integration-completed-comparison-check.json').write_text(json.dumps(out,indent=2)+'\n');print('independent52table multiplicities+FTS rowids+baseline match;209 code files verify; pending fullsuite/final acceptance')
