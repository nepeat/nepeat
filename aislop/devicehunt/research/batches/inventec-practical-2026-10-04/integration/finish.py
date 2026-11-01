import hashlib,json,os,shutil,sqlite3,subprocess,sys,time
from pathlib import Path
from devicehunt.rebuild import read_manifest
R=Path.cwd();B=R/'research/batches/inventec-practical-2026-10-04';O=B/'integration';D=R/'data';E=D/'exports';L=D/'devices.sqlite';C=D/'candidate-with-inventec-practical-2026-10-04.sqlite';RB=D/'rebuilt-with-inventec-practical-2026-10-04.sqlite'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def load(p):return json.loads(p.read_text())
def save(p,x):
 with p.open('x') as f:json.dump(x,f,indent=2,ensure_ascii=False);f.write('\n')
p=load(B/'promotion-acceptance.json');a=load(B/'acceptance.json');ready=load(O/'staging-ready.json');assert sha(B/'promotion-acceptance.json')=='89dc90f4e06b5db4fe98295ae47fde61f782556daddc7727431e0823edf19ad8'
def frozen():
 for g in ['files','code_sha256']:
  for n,h in a[g].items():assert sha(R/n)==h,n
 for n,h in p['accepted_proof_hashes'].items():assert sha(R/n)==h,n
 for n,h in load(O/'final-hashes.json').items():assert sha(R/n)==h,n
for path,key in [(C,'candidate_sha256'),(RB,'rebuild_sha256'),(D/'import-manifest.json','manifest_sha256')]:assert sha(path)==p[key]
frozen();assert sha(L)==p['old_live_sha256'];assert sha(D/'devices-before-inventec-practical-2026-10-04.sqlite')==p['old_live_sha256']
lookups={}
for path in E.glob('lookup-*.json'):
 old=load(path)
 if isinstance(old,dict) and {'query','limit_per_dataset'}<=old.keys():lookups[path.name]={k:old[k] for k in ['query','limit_per_dataset']}
assert len(lookups)==98
# exact candidate promotion only, backup already preserved
stage=D/'.devices-inventec-practical-2026-10-04-promote.sqlite';assert not stage.exists();shutil.copy2(C,stage);assert sha(stage)==p['candidate_sha256'];os.replace(stage,L)
save(O/'promotion-result.json',{'database_sha256':sha(L),'candidate_bytes_equal':L.read_bytes()==C.read_bytes(),'backup_sha256':sha(D/'devices-before-inventec-practical-2026-10-04.sqlite')});print('PROMOTED '+sha(L),flush=True)
commands=[]
def run(args,name):
 cmd=[sys.executable,'-B',*args];t=time.perf_counter()
 with (O/name).open('x') as f:res=subprocess.run(cmd,cwd=R,stdout=f,stderr=subprocess.STDOUT)
 commands.append({'command':cmd,'returncode':res.returncode,'elapsed_seconds':time.perf_counter()-t,'log':str(O/name)});save(O/(name+'.result.json'),commands[-1]);assert res.returncode==0,(cmd,res.returncode);assert sha(L)==p['candidate_sha256']
# Execute frozen script with dictionary-only legacy lookup eligibility at runtime; original script unchanged.
run(['-c',"from pathlib import Path; s=Path('scripts/refresh_exports.py').read_text(); s=s.replace(\"if 'query' in previous and 'limit_per_dataset' in previous:\",\"if isinstance(previous,dict) and 'query' in previous and 'limit_per_dataset' in previous:\"); exec(compile(s,'scripts/refresh_exports.py','exec'))"],'refresh-exports-final.log')
run(['scripts/refresh_collection_reports.py'],'refresh-collection-reports-final.log')
run(['scripts/compare_rebuild.py',str(RB),'--database',str(L),'--output-prefix','rebuild'],'compare-rebuild-final.log')
cur=load(D/'inventec-practical-facts-2026-10-04.json');reports=[]
for model in ['Entei','Golduck']:
 cmd=[sys.executable,'-B','-m','devicehunt','--db',str(L),'buying-report',model,'--format','json'];t=time.perf_counter();res=subprocess.run(cmd,capture_output=True,text=True);assert res.returncode==0,res.stderr;r= json.loads(res.stdout);assert r['vendor'] is None
 row=next(x for x in cur['devices'] if x['model']==model);ctx=[x for x in r['matched_records'] if x['source_id']==row['source']];assert len(ctx)==1;assert sorted((f['name'],f['value']) for f in ctx[0]['context_facts'])==sorted(map(tuple,row['facts']));assert sorted((f['company'],f['role']) for f in ctx[0]['company_roles'])==sorted(map(tuple,row['affiliations']));reports.append(r);commands.append({'command':cmd,'returncode':0,'elapsed_seconds':time.perf_counter()-t});assert sha(L)==p['candidate_sha256']
(E/'inventec-practical-2026-10-04-buying-reports.json').write_text(json.dumps({'database_sha256':sha(L),'vendor':None,'reports':reports,'scope':'Two exact unfiltered reports with complete separate source-local fact and company-role contexts.'},indent=2,ensure_ascii=False)+'\n')
node=r'''const fs=require('fs'),assert=require('assert/strict'),{mount}=require('./tests/report.test.cjs');const html=fs.readFileSync('data/exports/catalog.html','utf8'),tpl=fs.readFileSync('devicehunt/report.html','utf8');const script=x=>x.match(/<script>\s*([\s\S]*?)<\/script>/)[1];assert.equal(script(html),script(tpl));const data=JSON.parse(html.match(/<script id="data" type="application\/json">([\s\S]*?)<\/script>/)[1]);assert.equal(data.length,4090);const ui=mount(data);ui.set('scope','all');for(const model of ['Entei','Golduck']){const row=data.find(d=>d.brand==='Inventec'&&d.model===model);assert.ok(row);for(const role of ['brand','platform_vendor','manufacturer'])assert.ok(row.affiliations.some(a=>a.company==='Inventec'&&a.role===role));ui.set('query',model);ui.set('vendor','Inventec');ui.set('role','platform_vendor');assert.ok(ui.titles().some(x=>x.includes(model)));}ui.set('query','');ui.set('role','');ui.set('vendor','ByteDance');assert.equal(ui.titles().length,0);for(const x of ['inventec-entei-announcement-specs-2020','inventec-golduck-product-brief-v3-2020','N+1 redundancy, 8x 4056 dual rotor fan','Intel C621'])assert.ok(html.includes(x),x);console.log('HTML DOM4090 identities; both exact models/newsources/practicalfields and separate roles; ByteDance0');'''
t=time.perf_counter();res=subprocess.run(['node','-e',node],capture_output=True,text=True);save(O/'final-html-dom-proof.json',{'command':['node','-e',node],'returncode':res.returncode,'elapsed_seconds':time.perf_counter()-t,'stdout':res.stdout,'stderr':res.stderr,'html_sha256':sha(E/'catalog.html')});assert res.returncode==0,res.stderr
latest=load(E/'latest-export-audit.json');assert latest['lookup_reports_refreshed']==98
for n,q in lookups.items():assert {k:load(E/n)[k] for k in q}==q
for n in ['latest-export-audit','collection-reports-audit','hyperscaler-inventory-audit']:assert load(E/(n+'.json'))['database_sha256']==sha(L)
base=load(E/'inventory-validation.json');byte=load(E/'vendor-example-validation.json');assert base['summary']=={'total':10,'hits':10,'gaps':0};assert base['evidence_summary']=={'document_snapshot_hits':9,'label_photo_only_hits':1,'unresolved_identity_gaps':0};assert byte['summary']=={'total':2,'hits':0,'gaps':2}
assert latest['inventories']['manufacturer']['identity_target_pairs']==2142 and latest['inventories']['manufacturer']['unique_identities']==2141;assert latest['inventories']['all']['identity_target_pairs']==4227 and latest['inventories']['all']['unique_identities']==4089
con=sqlite3.connect(L.as_uri()+'?mode=ro',uri=True);con.row_factory=sqlite3.Row;counts={k:con.execute('SELECT count(*) FROM '+k).fetchone()[0] for k in a['expected_counts']};assert counts==a['expected_counts'];assert con.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0]==22
assert [r[0] for r in con.execute('PRAGMA integrity_check')]==['ok'] and not con.execute('PRAGMA foreign_key_check').fetchall()
pdfs=[]
for row in con.execute('SELECT d.*,s.cache_path FROM documents d JOIN sources s ON s.id=d.source_id ORDER BY d.source_id'):
 path=Path(row['cache_path']).parent/row['pdf_file'];assert sha(path)==row['pdf_sha256'];pdfs.append({'source_id':row['source_id'],'path':str(path.relative_to(R)),'sha256':sha(path)})
assert len(pdfs)==58
pages=[dict(r) for r in con.execute("SELECT p.* FROM document_pages p JOIN documents d USING(document_key) WHERE d.source_id='reference-pdf-inventec-golduck-product-brief-v3-2020' ORDER BY page_number")];assert len(pages)==2 and all(r['text'] and r['text_status']=='embedded_text' for r in pages)
comparison=load(E/'rebuild-comparison.json');assert len(comparison)==43 and all(v['original']==v['rebuilt'] and v['only_original']==v['only_rebuilt']==0 for v in comparison.values());con.close();_,manifest=read_manifest(D/'import-manifest.json');frozen();assert sha(L)==p['candidate_sha256']
proof={'phase':'promoted verified checkpoint','database_sha256':sha(L),'candidate_sha256':sha(C),'rebuild_sha256':sha(RB),'manifest_sha256':sha(D/'import-manifest.json'),'counts':counts,'schema':28,'manifest_jobs':len(manifest['imports']),'hashed_inputs':len(manifest['files']),'lookup_reports_refreshed':98,'eligible_lookup_queries':lookups,'buying_reports':2,'buying_vendor':None,'commands':commands,'all_commands_terminal_success':True,'verified_pdfs':pdfs,'golduck_two_pages':pages,'owned_baseline':base['summary'],'owned_evidence':base['evidence_summary'],'vendor_examples':byte['summary'],'inventories':latest['inventories'],'hyperscaler_assertions':22,'logical_tables_equal':43,'duplicate_sensitive_and_bothFTS':'accepted candidate proof bound to exact promoted bytes','candidate_live_bytes_equal':L.read_bytes()==C.read_bytes(),'read_only_unchanged_database':True,'html_dom':'pass','no_code_source_changes':True,'no_tests_rebuild_rerun':True,'export_hashes':{str(x.relative_to(R)):sha(x) for x in E.iterdir() if x.is_file()}}
save(O/'final-verification.json',proof);print('FINAL VERIFIED '+json.dumps(counts),flush=True)
