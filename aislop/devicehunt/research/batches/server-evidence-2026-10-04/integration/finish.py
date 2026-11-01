"""Prepared one-shot exact promotion/export helper; requires coordinator gate."""
import hashlib,json,os,shutil,sqlite3,subprocess,sys
from pathlib import Path
from devicehunt.rebuild import read_manifest
ROOT=Path.cwd();B=ROOT/'research/batches/server-evidence-2026-10-04';O=B/'integration';D=ROOT/'data';E=D/'exports'
LIVE=D/'devices.sqlite';CAND=D/'candidate-with-server-evidence-2026-10-04.sqlite';REBUILT=D/'rebuilt-with-server-evidence-2026-10-04.sqlite'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def save(p,obj):
 with p.open('x') as f:json.dump(obj,f,indent=2,ensure_ascii=False);f.write('\n')
promotion=json.loads((B/'promotion-acceptance.json').read_text());accepted=json.loads((B/'acceptance.json').read_text());ready=json.loads((O/'staging-ready.json').read_text())
assert promotion['phase']=='promotion_and_exports'
assert sha(O/'staging-ready.json')==promotion['staging_ready_sha256']
for p,k in [(CAND,'candidate_sha256'),(REBUILT,'rebuild_sha256'),(D/'import-manifest.json','manifest_sha256')]:assert sha(p)==promotion[k]
def frozen():
 for group in ['files','code_sha256']:
  for name,h in accepted[group].items():assert sha(ROOT/name)==h,name
 for name,h in ready['hashes'].items():assert sha(ROOT/name)==h,name
 for name,h in promotion['accepted_proof_hashes'].items():assert sha(ROOT/name)==h,name
frozen();assert sha(LIVE)==promotion['old_live_sha256']==ready['live_sha256']
assert sha(D/'devices-before-server-evidence-2026-10-04.sqlite')==promotion['old_live_sha256']
_,manifest=read_manifest(D/'import-manifest.json');assert len(manifest['imports'])==119 and len(manifest['files'])==2191
assert not (O/'final-verification.json').exists()
snapshot=E/'server-evidence-2026-10-04-buying-reports.json';assert not snapshot.exists()
# Freeze exact98 eligible lookup identities before standard refresh.
lookups={}
for path in E.glob('lookup-*.json'):
 previous=json.loads(path.read_text())
 if isinstance(previous,dict) and {'query','limit_per_dataset'} <= previous.keys():
  lookups[path.name]={k:previous[k] for k in ['query','limit_per_dataset']}
assert len(lookups)==98
staging=D/'.devices-server-evidence-2026-10-04-promote.sqlite';assert not staging.exists();shutil.copy2(CAND,staging);assert sha(staging)==promotion['candidate_sha256'];frozen();os.replace(staging,LIVE)
commands=[]
def run(args,log):
 cmd=[sys.executable,'-B',*args]
 with (O/log).open('x') as f:p=subprocess.run(cmd,cwd=ROOT,stdout=f,stderr=subprocess.STDOUT)
 commands.append({'command':cmd,'returncode':p.returncode,'log':str(O/log)});assert p.returncode==0,(cmd,p.returncode)
 assert sha(LIVE)==sha(CAND)==promotion['candidate_sha256']
print('PROMOTED '+sha(LIVE),flush=True)
run(['scripts/refresh_exports.py'],'refresh-exports-final.log')
run(['scripts/refresh_collection_reports.py'],'refresh-collection-reports-final.log')
run(['scripts/compare_rebuild.py',str(REBUILT),'--database',str(LIVE),'--output-prefix','rebuild'],'compare-rebuild-final.log')
new=json.loads((D/'server-evidence-facts-2026-10-04.json').read_text());by_model={}
for r in new['devices']:by_model.setdefault(r['model'],[]).append(r)
reports=[]
models=['SV1120A','SV1120I','SV1121A','SV1121I','SV2121A','SV2121I','SV2221A','SV2221I','Oracle Server E4-2C']
for model in models:
 cmd=[sys.executable,'-B','-m','devicehunt','--db',str(LIVE),'buying-report',model,'--format','json'];p=subprocess.run(cmd,capture_output=True,text=True);assert p.returncode==0,p.stderr
 report=json.loads(p.stdout);assert report['vendor'] is None
 for r in by_model[model]:
  matches=[v for v in report['matched_records'] if v['source_id']==r['source']];assert len(matches)==1 and matches[0]['model']==model
  ctx=matches[0];assert sorted((f['name'],f['value']) for f in ctx['context_facts'])==sorted((name,str(value)) for name,value in r['facts'])
  assert sorted((v['company'],v['role']) for v in ctx['company_roles'])==sorted(map(tuple,r['affiliations']))
 reports.append(report);commands.append({'command':cmd,'returncode':0,'report_query':model,'vendor':None})
 assert sha(LIVE)==promotion['candidate_sha256']
save(snapshot,{'database_sha256':sha(LIVE),'vendor':None,'reports':reports,'scope':'Exact record-local reports; reconstruction provenance retained. Eight suffixed Ingrasys model reports plus full Oracle E4-2C. Parent portfolio is separate from vendor specifications; CMVP configurations are software-module tests, not general OS support.'})
node=r'''const fs=require('fs'),assert=require('assert/strict');const {mount}=require('./tests/report.test.cjs');const html=fs.readFileSync('data/exports/catalog.html','utf8');const template=fs.readFileSync('devicehunt/report.html','utf8');const script=x=>x.match(/<script>\s*([\s\S]*?)<\/script>/)[1];assert.equal(script(html),script(template));const data=JSON.parse(html.match(/<script id="data" type="application\/json">([\s\S]*?)<\/script>/)[1]);assert.equal(data.length,4090);const ui=mount(data);ui.set('scope','all');for(const model of ['SV1120A','SV1120I','SV1121A','SV1121I','SV2121A','SV2121I']){const row=data.find(d=>d.brand==='Ingrasys'&&d.model===model);assert.ok(row,model);assert.ok(row.affiliations.some(a=>a.company==='Foxconn'&&a.role==='parent_company_portfolio'));assert.ok(!row.affiliations.some(a=>a.company==='Foxconn'&&a.role==='manufacturer'));ui.set('query',model);ui.set('vendor','Foxconn');ui.set('role','parent_company_portfolio');assert.ok(ui.titles().some(x=>x.includes(model)),model);}ui.set('query','');ui.set('role','');ui.set('vendor','ByteDance');assert.equal(ui.titles().length,0);assert.ok(html.includes('Reconstructed second span of malformed HTTP body; appended three terminal EOF bytes only.'));console.log('generated HTML4090 identities, sixnew exact models, separate Foxconnparent roles, ByteDance0 and reconstructedPDFscope pass; no screenshot');'''
p=subprocess.run(['node','-e',node],capture_output=True,text=True,cwd=ROOT);assert p.returncode==0,p.stderr
save(O/'final-html-dom-proof.json',{'html_sha256':sha(E/'catalog.html'),'database_sha256':sha(LIVE),'command':['node','-e','recorded finish.py node program'],'returncode':p.returncode,'output':p.stdout,'screenshot':False})
frozen();assert sha(LIVE)==sha(CAND)==promotion['candidate_sha256']
latest=json.loads((E/'latest-export-audit.json').read_text());assert latest['lookup_reports_refreshed']==98
for name,query in lookups.items():assert {k:json.loads((E/name).read_text())[k] for k in query}==query
for name in ['latest-export-audit','collection-reports-audit','hyperscaler-inventory-audit']:assert json.loads((E/(name+'.json')).read_text())['database_sha256']==sha(LIVE)
assert latest['inventories']['manufacturer']['identity_target_pairs']==2142 and latest['inventories']['manufacturer']['unique_identities']==2141
assert latest['inventories']['all']['identity_target_pairs']==4227 and latest['inventories']['all']['unique_identities']==4089
comparison=json.loads((E/'rebuild-comparison.json').read_text());assert len(comparison)==43 and all(v['original']==v['rebuilt'] and v['only_original']==v['only_rebuilt']==0 for v in comparison.values())
db=sqlite3.connect(LIVE.as_uri()+'?mode=ro',uri=True);db.row_factory=sqlite3.Row
assert [r[0] for r in db.execute('PRAGMA integrity_check')]==['ok'] and not db.execute('PRAGMA foreign_key_check').fetchall()
counts={t:db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['devices','records','sources','facts','documents','document_pages','hyperscaler_evidence']}
assert counts=={'devices':4090,'records':5317,'sources':287,'facts':44975,'documents':57,'document_pages':3661,'hyperscaler_evidence':22}
pdfs=[]
for row in db.execute('SELECT d.*,s.cache_path FROM documents d JOIN sources s ON s.id=d.source_id ORDER BY d.source_id'):
 path=Path(row['cache_path']).parent/row['pdf_file'];assert sha(path)==row['pdf_sha256'];pdfs.append({'source_id':row['source_id'],'path':str(path.relative_to(ROOT)),'sha256':sha(path)})
assert len(pdfs)==57
blank=[dict(r) for r in db.execute("SELECT p.* FROM document_pages p JOIN documents d USING(document_key) WHERE d.source_id='reference-pdf-ingrasys-servers-reconstructed-2026-10-04' ORDER BY page_number")];assert len(blank)==10 and all(r['text']=='' and r['text_status']=='no_embedded_text' for r in blank)
base=json.loads((E/'inventory-validation.json').read_text());examples=json.loads((E/'vendor-example-validation.json').read_text())
assert base['summary']=={'total':10,'hits':10,'gaps':0} and base['evidence_summary']=={'document_snapshot_hits':9,'label_photo_only_hits':1,'unresolved_identity_gaps':0};assert examples['summary']=={'total':2,'hits':0,'gaps':2}
oldmanifest=json.loads((D/'import-manifest-before-server-evidence-2026-10-04.json').read_text());assert all(manifest['files'][k]==v and sha(D/k)==v for k,v in oldmanifest['files'].items())
_,manifest=read_manifest(D/'import-manifest.json');frozen();assert sha(LIVE)==promotion['candidate_sha256'];db.close()
# Bind refreshed exports; retain older dated buying snapshots as historical.
exporthashes={str(p.relative_to(ROOT)):sha(p) for p in sorted(E.iterdir()) if p.is_file()}
proof={'phase':'promoted verified checkpoint','database_sha256':sha(LIVE),'candidate_sha256':sha(CAND),'rebuild_sha256':sha(REBUILT),'manifest_sha256':sha(D/'import-manifest.json'),'schema':28,'counts':counts,'manifest_jobs':119,'hashed_inputs':2191,'old_manifest_inputs_preserved':len(oldmanifest['files']),'complete_reference_entries':46,'exact_old_preservation':'accepted candidate-verification.json; only45old reference source sha/cache/fetched refreshed','accepted_staging_ready_sha256':promotion['staging_ready_sha256'],'accepted_proof_hashes':promotion['accepted_proof_hashes'],'source_provenance_hashes':json.loads((O/'stage-result.json').read_text())['inputs_provenance'],'verified_pdfs':pdfs,'derivative_blank_pages':blank,'lookup_reports_refreshed':98,'eligible_lookup_queries':lookups,'buying_reports':9,'buying_vendor':None,'all_new_accepted_record_context_facts_and_roles_verified':True,'owned_baseline':base['summary'],'owned_evidence':base['evidence_summary'],'vendor_examples':examples['summary'],'inventories':latest['inventories'],'logical_tables_equal':43,'duplicate_sensitive43_and_bothFTS':'frozen accepted candidate proof, bound to exact promoted bytes','candidate_live_bytes_equal':LIVE.read_bytes()==CAND.read_bytes(),'readonly_operations_unchanged_database':True,'tests':'reused frozen428full/11focused; no repeat','code_test_freeze_sha256':accepted['code_sha256'],'commands':commands,'export_hashes':exporthashes,'html_dom':'pass; no screenshot','processes_completed':True}
save(O/'final-verification.json',proof)
print('FINAL VERIFIED '+json.dumps(counts),flush=True)
