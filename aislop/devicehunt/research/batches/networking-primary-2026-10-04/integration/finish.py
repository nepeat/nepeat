"""Authorized exact promotion and sequential read-only export refresh; one-shot."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
from devicehunt.rebuild import read_manifest
ROOT=Path.cwd();HERE=Path(__file__).resolve().parent;DATA=ROOT/'data';OUT=DATA/'exports'
LIVE=DATA/'devices.sqlite';CAND=DATA/'networking-primary-candidate-2026-10-04.sqlite'
REBUILT=DATA/'rebuilt-with-networking-primary-2026-10-04.sqlite';MANIFEST=DATA/'import-manifest.json'
INPUT=DATA/'networking-primary-facts-2026-10-04.json'
accept=json.loads((HERE.parent/'acceptance.json').read_text())
promotion=json.loads((HERE.parent/'promotion-acceptance.json').read_text())
ready=json.loads((HERE/'review-ready.json').read_text())
def sha(p):
 with Path(p).open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def frozen():
 for group in ['files','code_sha256']:
  for p,h in accept[group].items():assert sha(ROOT/p)==h,p
 for p,h in promotion['accepted_proof_hashes'].items():assert sha(HERE.parent/p)==h,p
 for p,k in [(CAND,'candidate_sha256'),(REBUILT,'rebuilt_sha256'),(MANIFEST,'manifest_sha256'),(INPUT,'merged_input_sha256')]:assert sha(p)==promotion[k],str(p)
def save(p,value):
 with p.open('x') as stream:json.dump(value,stream,indent=2,ensure_ascii=False);stream.write('\n')
def run(args,name):
 with (HERE/name).open('x') as stream:subprocess.run([sys.executable,*args],cwd=ROOT,stdout=stream,stderr=subprocess.STDOUT,check=True)
 assert sha(LIVE)==promotion['candidate_sha256']
frozen();assert promotion['phase']=='promotion_and_exports'
assert sha(LIVE)==promotion['old_live_sha256']
assert sha(DATA/'devices-before-networking-primary-2026-10-04.sqlite')==promotion['old_live_sha256']
_,manifest=read_manifest(MANIFEST)
assert len(manifest['imports'])==118 and len(manifest['files'])==2181
assert not (HERE/'final-verification.json').exists()
staging=DATA/'.devices-networking-primary-2026-10-04-promote.sqlite'
assert not staging.exists();shutil.copy2(CAND,staging);assert sha(staging)==promotion['candidate_sha256']
frozen();os.replace(staging,LIVE)
print('PROMOTED '+sha(LIVE),flush=True)
for script in ['scripts/refresh_exports.py','scripts/refresh_collection_reports.py']:run([script],Path(script).stem+'-final.log')
run(['scripts/compare_rebuild.py',str(REBUILT),'--database',str(LIVE),'--output-prefix','rebuild'],'compare-rebuild-final.log')
reports=[]
for name,query in [('oracle','Oracle Server X7-2C'),('as7772','AS7772-32X')]:
 r=json.loads(subprocess.run([sys.executable,'-m','devicehunt','--db',str(LIVE),'buying-report',query,'--format','json'],check=True,capture_output=True,text=True).stdout)
 assert r==json.loads((HERE/(name+'-cli-report.json')).read_text()) and r['vendor'] is None
 reports.append(r)
snapshot=OUT/'networking-primary-2026-10-04-buying-reports.json'
save(snapshot,{'database_sha256':sha(LIVE),'vendor':None,'reports':reports})
node=r'''const fs=require('fs'),assert=require('assert/strict');const {mount}=require('./tests/report.test.cjs');const html=fs.readFileSync('data/exports/catalog.html','utf8');const template=fs.readFileSync('devicehunt/report.html','utf8');const script=x=>x.match(/<script>\s*([\s\S]*?)<\/script>/)[1];assert.equal(script(html),script(template));const data=JSON.parse(html.match(/<script id="data" type="application\/json">([\s\S]*?)<\/script>/)[1]);assert.equal(data.length,4084);const artifact=data.find(d=>d.id==='ee98faf36b19bd060124');assert.ok(artifact);assert.equal(artifact.brand,'');assert.equal(artifact.affiliations.length,0);assert.equal(artifact.kind,'networking software artifact reference');const ui=mount(data);ui.set('scope','all');ui.set('query','Oracle Server X7-2C');assert.ok(ui.titles().some(x=>x.includes('Oracle Server X7-2C')));ui.set('query','AS7772-32X');assert.ok(ui.titles().some(x=>x.includes('published DTB-source artifact')));ui.set('vendor','ByteDance');ui.set('query','');assert.equal(ui.titles().length,0);console.log('generated HTML script/payload, full Oracle/AS7772 searches and unbranded artifact boundary pass; no screenshot');'''
result=subprocess.run(['node','-e',node],cwd=ROOT,check=True,capture_output=True,text=True)
save(HERE/'final-html-dom-proof.json',{'database_sha256':sha(LIVE),'html_sha256':sha(OUT/'catalog.html'),'result':result.stdout,'screenshot':False})
frozen();assert sha(LIVE)==sha(CAND)==promotion['candidate_sha256']
latest=json.loads((OUT/'latest-export-audit.json').read_text());assert latest['lookup_reports_refreshed']==98
for name in ['latest-export-audit','collection-reports-audit','hyperscaler-inventory-audit']:assert json.loads((OUT/(name+'.json')).read_text())['database_sha256']==sha(LIVE)
comparison=json.loads((OUT/'rebuild-comparison.json').read_text());assert len(comparison)==43
assert all(v['original']==v['rebuilt'] and v['only_original']==v['only_rebuilt']==0 for v in comparison.values())
assert LIVE.read_bytes()==CAND.read_bytes()
with sqlite3.connect(LIVE.as_uri()+'?mode=ro',uri=True) as db:
 db.row_factory=sqlite3.Row
 assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok' and not db.execute('PRAGMA foreign_key_check').fetchall()
 counts={t:db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['devices','records','facts','sources','documents','document_pages','hyperscaler_evidence']}
 assert counts==ready['counts']
 pdfs=[]
 for row in db.execute('SELECT d.*,s.cache_path FROM documents d JOIN sources s ON s.id=d.source_id ORDER BY d.source_id'):
  p=Path(row['cache_path']).parent/row['pdf_file'];assert sha(p)==row['pdf_sha256'];pdfs.append({'source':row['source_id'],'path':str(p.relative_to(ROOT)),'sha256':sha(p)})
 assert len(pdfs)==56
 source_counts=dict(db.execute('SELECT id,records_seen FROM sources'))
old=json.loads((DATA/'import-manifest-before-networking-primary-2026-10-04.json').read_text())
for name,h in old['files'].items():assert manifest['files'][name]==h and sha(DATA/name)==h
_,manifest=read_manifest(MANIFEST);frozen();assert sha(LIVE)==promotion['candidate_sha256']
base=json.loads((OUT/'inventory-validation.json').read_text());examples=json.loads((OUT/'vendor-example-validation.json').read_text())
assert base['summary']=={'total':10,'hits':10,'gaps':0} and base['evidence_summary']=={'document_snapshot_hits':9,'label_photo_only_hits':1,'unresolved_identity_gaps':0}
assert examples['summary']=={'total':2,'hits':0,'gaps':2}
save(HERE/'final-verification.json',{'database_sha256':sha(LIVE),'candidate_sha256':sha(CAND),'previous_sha256':promotion['old_live_sha256'],'rebuild_sha256':sha(REBUILT),'manifest_sha256':sha(MANIFEST),'merged_input_sha256':sha(INPUT),'schema':28,'counts':counts,'additions':accept['expected_delta'],'source_counts':source_counts,'new_source_counts':{s:source_counts[s] for s in accept['source_ids']},'manifest_jobs':118,'hashed_inputs':2181,'tests':accept['tests'],'test_logs_sha256':{k:v for k,v in promotion['accepted_proof_hashes'].items() if k.endswith('tests.log')},'lookup_reports_refreshed':98,'buying_reports':2,'buying_vendor':None,'buying_snapshot_sha256':sha(snapshot),'verified_pdfs':pdfs,'all_previous_manifest_inputs_unchanged':len(old['files']),'reference_bundle':'unchanged','logical_tables_equal':43,'both_fts_duplicate_sensitive_equal':True,'candidate_live_bytes_equal':True,'readonly_cli_and_exports_unchanged_database':True,'exact_old_rows_including_timestamps_and_source_metadata_preserved':True,'owned_baseline':base['summary'],'owned_evidence':base['evidence_summary'],'vendor_examples':examples['summary'],'inventories':latest['inventories'],'code_test_files_unchanged':162,'accepted_inputs_unchanged':11,'accepted_proof_hashes':promotion['accepted_proof_hashes'],'html_sha256':sha(OUT/'catalog.html'),'html_dom':'pass; no screenshot','export_audit_hashes':{n:sha(OUT/(n+'.json')) for n in ['latest-export-audit','collection-reports-audit','hyperscaler-inventory-audit','rebuild-comparison','rebuild-validation']},'processes_completed':True})
print('FINAL VERIFIED '+json.dumps(counts),flush=True)
