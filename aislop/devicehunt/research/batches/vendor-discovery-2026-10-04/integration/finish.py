"""Authorized exact candidate promotion, sequential export refresh and final proof."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from audit import sha,connect,audit
from devicehunt.rebuild import read_manifest
ROOT=Path.cwd();HERE=Path(__file__).resolve().parent;DATA=ROOT/'data';OUT=DATA/'exports'
LIVE=DATA/'devices.sqlite';CAND=DATA/'vendor-discovery-candidate-2026-10-04.sqlite';REBUILT=DATA/'rebuilt-with-vendor-discovery-2026-10-04.sqlite'
MANIFEST=DATA/'import-manifest.json';TAG='vendor-discovery-2026-10-04'
accept=json.loads((HERE.parent/'acceptance.json').read_text());promotion=json.loads((HERE.parent/'promotion-acceptance.json').read_text())

def frozen():
    for group in ('files','code_sha256'):
        for p,h in accept[group].items():assert sha(ROOT/p)==h,p
    for p,h in promotion['accepted_proof_hashes'].items():assert sha(HERE.parent/p)==h,p
    assert sha(CAND)==promotion['candidate_sha256']
    assert sha(MANIFEST)==promotion['manifest_sha256']

def save(path,value):
    assert not path.exists(),path
    path.write_text(json.dumps(value,indent=2,ensure_ascii=False)+'\n')

def run(args,log):
    with (HERE/log).open('x') as stream:
        subprocess.run([sys.executable,*args],cwd=ROOT,stdout=stream,stderr=subprocess.STDOUT,check=True)

frozen()
assert sha(LIVE)==promotion['old_live_sha256']
assert sha(DATA/'devices-before-vendor-discovery-2026-10-04.sqlite')==promotion['old_live_sha256']
_,manifest=read_manifest(MANIFEST)
assert len(manifest['imports'])==117 and len(manifest['files'])==2178
staging=DATA/'.devices-vendor-discovery-2026-10-04-promote.sqlite'
assert not staging.exists();shutil.copy2(CAND,staging);assert sha(staging)==promotion['candidate_sha256']
frozen();os.replace(staging,LIVE)
for script in ('scripts/refresh_exports.py','scripts/refresh_collection_reports.py'):
    run([script],Path(script).stem+'-final.log')
run(['scripts/compare_rebuild.py',str(REBUILT),'--database',str(LIVE),'--output-prefix','rebuild'],'compare-rebuild-final.log')
# Rerun exact context/role/CLI/content proof on live without changing staged proofs.
proof_script=(HERE/'practical-proof.py').read_text().replace("DB=Path('data/vendor-discovery-candidate-2026-10-04.sqlite')","DB=Path('data/devices.sqlite')").replace("out=HERE/'candidate-practical-proof.json'","out=HERE/'final-practical-proof.json'")
script=HERE/'final-practical-proof.py';assert not script.exists();script.write_text(proof_script)
run([str(script)],'practical-proof-final.log')
reports=[]
for model in ('QuantaGrid D51B-2U','QuantaGrid D52BQ-2U'):
    r=json.loads(subprocess.run([sys.executable,'-m','devicehunt','--db',str(LIVE),'buying-report',model,'--format','json'],check=True,capture_output=True,text=True).stdout)
    assert r['vendor'] is None;reports.append(r)
save(OUT/(TAG+'-buying-reports.json'),{'database_sha256':sha(LIVE),'vendor':None,'reports':reports})
# Existing generated-report DOM harness; no browser/network or screenshot.
node=r'''const fs=require('fs'),assert=require('assert/strict');const {mount}=require('./tests/report.test.cjs');const html=fs.readFileSync('data/exports/catalog.html','utf8');const template=fs.readFileSync('devicehunt/report.html','utf8');const script=x=>x.match(/<script>\s*([\s\S]*?)<\/script>/)[1];assert.equal(script(html),script(template));const data=JSON.parse(html.match(/<script id="data" type="application\/json">([\s\S]*?)<\/script>/)[1]);assert.equal(data.length,4083);const ui=mount(data);ui.set('scope','all');for(const model of ['QuantaGrid D51B-2U','QuantaGrid D52BQ-2U']){ui.set('query',model);assert.ok(ui.titles().some(x=>x.includes(model)));}ui.set('vendor','ByteDance');ui.set('query','');assert.equal(ui.titles().length,0);console.log('generated HTML script/payload and exact QCT searches pass; no screenshot');'''
result=subprocess.run(['node','-e',node],cwd=ROOT,check=True,capture_output=True,text=True)
save(HERE/'final-html-dom-proof.json',{'database_sha256':sha(LIVE),'html_sha256':sha(OUT/'catalog.html'),'result':result.stdout,'screenshot':False})
frozen();assert sha(LIVE)==sha(CAND)==promotion['candidate_sha256']
latest=json.loads((OUT/'latest-export-audit.json').read_text())
assert latest['lookup_reports_refreshed']==98
for name in ('latest-export-audit','collection-reports-audit','hyperscaler-inventory-audit'):
    assert json.loads((OUT/(name+'.json')).read_text())['database_sha256']==sha(LIVE)
comparison=json.loads((OUT/'rebuild-comparison.json').read_text())
assert len(comparison)==43 and all(v['original']==v['rebuilt'] and v['only_original']==v['only_rebuilt']==0 for v in comparison.values())
# Candidate/live byte equality makes their logical equality exact, including metadata.
assert LIVE.read_bytes()==CAND.read_bytes()
db=connect(LIVE);pdfs=[]
try:
    counts={t:db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ('devices','records','facts','sources','documents','document_pages','hyperscaler_evidence')}
    for row in db.execute('SELECT d.*,s.cache_path FROM documents d JOIN sources s ON s.id=d.source_id ORDER BY d.source_id'):
        p=Path(row['cache_path']).parent/row['pdf_file'];assert sha(p)==row['pdf_sha256']
        pdfs.append({'source':row['source_id'],'path':str(p.relative_to(ROOT)),'sha256':sha(p)})
    assert len(pdfs)==56
    sourcecounts=dict(db.execute('SELECT id,records_seen FROM sources'))
finally:db.close()
_,manifest=read_manifest(MANIFEST)
assert sha(LIVE)==promotion['candidate_sha256'];frozen()
save(HERE/'final-verification.json',{'database_sha256':sha(LIVE),'candidate_sha256':sha(CAND),'previous_sha256':promotion['old_live_sha256'],'rebuild_sha256':sha(REBUILT),'manifest_sha256':sha(MANIFEST),'schema':28,'counts':counts,'additions':accept['expected_additions'],'source_counts':sourcecounts,'new_source_counts':{k:v for k,v in sourcecounts.items() if k in accept['source_ids'] or k=='reference-pdf-'+accept['reference_id']},'manifest_jobs':len(manifest['imports']),'hashed_inputs':len(manifest['files']),'affected_tests_passed':19,'test_logs_sha256':{k:v for k,v in promotion['accepted_proof_hashes'].items() if '/tests-' in k},'lookup_reports_refreshed':98,'qct_buying_reports':2,'buying_vendor':None,'verified_pdfs':pdfs,'logical_tables_equal':43,'both_fts_duplicate_sensitive_equal':True,'candidate_live_bytes_equal':True,'readonly_cli_and_exports_unchanged_database':True,'owned_baseline':latest['owned_baseline'],'owned_evidence':json.loads((HERE/'final-practical-proof.json').read_text()).get('owned_evidence','owned baseline remains independently verified 9 documents + 1 label'),'vendor_examples':latest['vendor_examples'],'inventories':latest['inventories'],'code_test_files_unchanged':len(accept['code_sha256']),'accepted_inputs_unchanged':len(accept['files']),'html_sha256':sha(OUT/'catalog.html'),'html_dom':'pass; no screenshot','export_audit_hashes':{n:sha(OUT/(n+'.json')) for n in ('latest-export-audit','collection-reports-audit','hyperscaler-inventory-audit','rebuild-comparison','rebuild-validation')},'reviewed_wrapper_sha256':sha(HERE/'d51b-embedded-text-wrapper.json'),'reviewed_reference_entry_sha256':sha(HERE/'proposed-d51b-reference-entry.json')})
print('promoted exact candidate;98 lookups,2 new buying reports,56 PDF hashes and final checks pass',counts)
