"""Exclusive reviewed offline batch integration. Run stage, then finish after rebuild."""
import json,os,sys,subprocess,sqlite3
from pathlib import Path
from collections import Counter
from pypdf import PdfReader
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt.rebuild import file_sha256 as sha,read_manifest
from devicehunt import validation,vendor_inventory,reference_documents
ROOT=Path.cwd(); D=ROOT/'data'; O=D/'exports'; B=ROOT/'research/batches/nics-wiwynn-hyve-2026-10-04'; RAW=D/'raw'; CAP=RAW/'nics-wiwynn-hyve-2026-10-04'; TAG='nics-wiwynn-hyve-2026-10-04'
LIVE=D/'devices.sqlite'; CAND=D/'nics-wiwynn-hyve-candidate-2026-10-04.sqlite'; REBUILD=D/'rebuilt-with-nics-wiwynn-hyve-2026-10-04.sqlite'; BUNDLE=RAW/('reference-documents-with-'+TAG+'.json')
BEFORE='935d8774acac42b1906f1e30d77b127a7d40534123d72cb1cd916a3a84be8dcb'
FILES={'curated-inspur-nics-2026-10-04.json':'8f3da5d987e699fbce8888c9015061431340fd6da7831f37b31c639be12d4e51','curated-wiwynn-hyve-2026-10-04.json':'378a37307403fe9046f9cb078ef319ba2cb77824bccf9087eb0a4bd5bbe2a2aa'}
def save(p,x): p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def freshsave(p,x):
 assert not p.exists(),p
 save(p,x)
def cp(a,b):
 assert not b.exists(),b
 subprocess.run(['/bin/cp','-cp',str(a),str(b)],check=True); assert sha(a)==sha(b)
def frozen():
 checkpoint=json.loads((B/'stable-code-checkpoint.json').read_text())
 for p,h in checkpoint['code_sha256'].items(): assert sha(ROOT/p)==h,p
 for p,h in checkpoint['logs_sha256'].items(): assert sha(B/'engineering'/p)==h,p
 for p,h in FILES.items(): assert sha(D/p)==h,p
 return checkpoint
TABLES=list(json.loads((O/'inspur-2018-inventec-rebuild-2026-10-04-comparison.json').read_text()))
def check(db):
 assert db.execute('PRAGMA user_version').fetchone()[0]==28
 assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
 assert not db.execute('PRAGMA foreign_key_check').fetchall()
 base=validation.run(db,ROOT/'validation/owned-hardware.json'); ex=validation.run(db,ROOT/'validation/vendor-examples.json')
 assert base['summary']==dict(total=10,hits=10,gaps=0)
 assert base['evidence_summary']==dict(document_snapshot_hits=9,label_photo_only_hits=1,unresolved_identity_gaps=0)
 assert ex['summary']==dict(total=2,hits=0,gaps=2)
 assert db.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0]==22
 previous=json.loads((D/'exports/latest-export-audit.json').read_text())['inventories']
 for scope in ('manufacturer','factory'):
  inv=vendor_inventory.run(db,scope=scope,limit=-1)
  for k in ('identity_target_pairs','unique_identities','counts_by_target'): assert inv[k]==previous[scope][k],(scope,k)
 return dict(owned=base['summary'],owned_evidence=base['evidence_summary'],bytedance=ex['summary'])
if sys.argv[1]=='stage':
 frozen();assert sha(LIVE)==BEFORE
 _,manifest=read_manifest(D/'import-manifest.json')
 oldbundle=RAW/'reference-documents-with-inspur-2018-inventec-2026-10-04-v2.json'; bundle=json.loads(oldbundle.read_text()); assert len(bundle['documents'])==39
 added=[]
 for r in json.loads((CAP/'fetch-report.json').read_text()):
  if not r.get('path','').endswith('.pdf'): continue
  original=CAP/r['path'];assert sha(original)==r['sha256']
  sibling=RAW/(original.stem+'-2026-10-04.pdf');cp(original,sibling)
  pages=[dict(page=n,text=p.extract_text() or '') for n,p in enumerate(PdfReader(original).pages,1)]
  text=RAW/(original.stem+'-2026-10-04.pages.json');freshsave(text,dict(pdf_sha256=sha(original),extraction_method='pypdf embedded text; no OCR',pages=pages))
  bundle['documents'].append(dict(id=original.stem,pdf=dict(path=sibling.name,sha256=sha(sibling)),text=dict(path=text.name,sha256=sha(text)),url=r['url'],title=original.stem+' vendor datasheet',category='datasheet',tier='vendor',scope='Complete immutable captured vendor PDF; text search does not create identity, manufacture, aliases or deployment claims. Reviewed source-scoped facts are separately curated.',page_count=len(pages)))
  added.extend([sibling,text])
 assert sum(x['page_count'] for x in bundle['documents'][39:])==8
 freshsave(BUNDLE,bundle);reference_documents.prepare(BUNDLE)
 cp(LIVE,CAND);c=Catalog(CAND)
 for source,p in [('reference-documents',BUNDLE),*[('curated',D/p) for p in FILES]]: print(source,import_source(c,source,p),flush=True)
 c.db.close();c=Catalog(CAND,read_only=True);db=c.db;db.execute('ATTACH DATABASE ? AS old',(LIVE.as_uri()+'?mode=ro',))
 preserved={}
 for table in TABLES:
  if table in ('sources','search_index','document_search'):continue
  cols=[r['name'] for r in db.execute('PRAGMA main.table_info('+table+')') if r['name'] not in ('observed_at','fetched_at','cache_path')];fields=','.join('"'+x+'"' for x in cols)
  old=Counter(tuple(r) for r in db.execute('SELECT '+fields+' FROM old.'+table));new=Counter(tuple(r) for r in db.execute('SELECT '+fields+' FROM main.'+table));missing=old-new;assert not missing,(table,len(missing))
  preserved[table]=dict(previous=sum(old.values()),current=sum(new.values()),missing=0)
 for r in db.execute('SELECT * FROM old.sources'):
  new=db.execute('SELECT * FROM sources WHERE id=?',(r['id'],)).fetchone();assert new
  for k in r.keys():
   if k in ('fetched_at','cache_path'):continue
   assert new[k]==(sha(BUNDLE) if k=='sha256' and r['id'].startswith('reference-pdf-') else r[k]),(r['id'],k)
 validation_result=check(db)
 rows=[json.loads(r[0]) for r in db.execute("SELECT raw_json FROM records WHERE source_id='inspur-incloud-rail-nics-2023'")];assert len(rows)==10
 assert sum(r['raw_table_row']['remarks']=='' for r in rows)==3
 for path in FILES:
  facts=json.loads((D/path).read_text())
  for s in facts['sources']:
   imported=[json.loads(r[0]) for r in db.execute('SELECT raw_json FROM records WHERE source_id=?',(s['id'],))]
   expected=[r for r in facts['devices'] if r['source']==s['id']]
   assert Counter(json.dumps(r,sort_keys=True,ensure_ascii=False) for r in imported)==Counter(json.dumps(r,sort_keys=True,ensure_ascii=False) for r in expected),s['id']
 counts={k:db.execute('SELECT count(*) FROM '+('fcc_grantees' if k=='grantees' else k)).fetchone()[0] for k in manifest['expected'] if k!='sources'};counts['sources']=dict(db.execute('SELECT id,records_seen FROM sources'))
 assert counts['records']==5298 and counts['documents']==55 and counts['document_pages']==3649
 assert counts['devices']==4083
 c.db.close();assert sha(LIVE)==BEFORE
 freshsave(O/(TAG+'-preservation.json'),dict(previous_sha256=BEFORE,database_sha256=sha(CAND),preserved_tables=preserved,validation=validation_result,expected_counts=counts,code_checkpoint=frozen()))
 cp(D/'import-manifest.json',D/('import-manifest-before-'+TAG+'.json'))
 jobs=[j for j in manifest['imports'] if j['source']=='reference-documents'];assert len(jobs)==1;jobs[0]['path']=str(BUNDLE.relative_to(D))
 manifest['imports'].extend(dict(source='curated',path=p) for p in FILES)
 for p in [BUNDLE,*added,*[D/p for p in FILES],*list(CAP.iterdir())]:
  assert p.is_file();manifest['files'][str(p.relative_to(D))]=sha(p)
 manifest['expected']=counts;save(D/'import-manifest.json',manifest);read_manifest(D/'import-manifest.json')
 print(json.dumps(dict(identities=counts['devices'],records=counts['records'],sources=len(counts['sources']),jobs=len(manifest['imports']),files=len(manifest['files']))))
def proof(path,label,html):
 from devicehunt.__main__ import report
 before=sha(path);c=Catalog(path,read_only=True);v=check(c.db);report(c,html);c.db.close()
 cases=[['search','E810_2Port_LC_PCIe','--format','json'],['lookup','E810_2Port_LC_PCIe','--format','json'],['documents','SV7220G3','--format','json'],['specs','SV7220G3-V+','--vendor','Wiwynn','--exact-model','--format','json'],['specs','HS9124','--vendor','Hyve','--exact-model','--format','json'],['match-listing','--title','Wiwynn SV7220G3-V++ bare node','--vendor','Wiwynn','--with-specs','--format','json'],['match-listing','--title','Wiwynn SV7220G3-V+ bare node','--vendor','Wiwynn','--with-specs','--format','json'],['match-listing','--title','Wiwynn SV7220G3-V bare node','--vendor','Wiwynn','--with-specs','--format','json'],['validate-inventory','--inventory','validation/owned-hardware.json','--format','json'],['validate-inventory','--inventory','validation/vendor-examples.json','--format','json']]
 results=[]
 for args in cases:
  r=subprocess.run([sys.executable,'-m','devicehunt','--db',str(path),*args],capture_output=True,text=True);assert r.returncode==(2 if 'validation/vendor-examples.json' in args else 0),(args,r.stderr)
  result=json.loads(r.stdout)
  if args[0]=='match-listing':
   candidates=result['results'][0]['result']['candidates'];assert candidates
   if 'V++' in args[2]: assert all(not m['specifications'] and m['match_type']=='variant_lead' for d in candidates for m in d['matches'])
   else:
    exact=[d['model'] for d in candidates if any(m['match_type']=='model_mention' for m in d['matches'])];assert ('SV7220G3-V+' if 'V+' in args[2] else 'SV7220G3-V') in exact
    assert ('SV7220G3-V' if 'V+' in args[2] else 'SV7220G3-V+') not in exact
  results.append(dict(arguments=args,result=result))
 node="""const fs=require('fs'),assert=require('assert/strict');const {mount}=require('./tests/report.test.cjs');const html=fs.readFileSync(process.argv[1],'utf8');const template=fs.readFileSync('devicehunt/report.html','utf8');const script=x=>x.match(/<script>\\s*([\\s\\S]*?)<\\/script>/)[1];assert.equal(script(html),script(template));const data=JSON.parse(html.match(/<script id=\"data\" type=\"application\\/json\">([\\s\\S]*?)<\\/script>/)[1]);assert.equal(data.length,4083);const ui=mount(data);ui.set('vendor','Wiwynn');ui.set('scope','all');ui.set('query','SV7220G3-V+');assert.ok(ui.titles().includes('Wiwynn SV7220G3-V+'));ui.set('scope','manufacturer');assert.equal(ui.titles().length,0);ui.set('scope','all');ui.set('vendor','Hyve');ui.set('query','HS9124');assert.ok(ui.titles().includes('Hyve HS9124'));ui.set('vendor','ByteDance');ui.set('query','');assert.equal(ui.titles().length,0);"""
 r=subprocess.run(['node','-e',node,str(html)],capture_output=True,text=True);assert r.returncode==0,r.stderr
 assert sha(path)==before
 save(O/(TAG+'-'+label+'-proof.json'),dict(database_sha256=before,read_only=True,validation=v,cli=results,html_sha256=sha(html),ui='Generated HTML payload and inline script in DOM harness; no screenshot'))
if sys.argv[1]=='finish':
 checkpoint=frozen();audit=json.loads((O/(TAG+'-preservation.json')).read_text());assert sha(LIVE)==BEFORE and sha(CAND)==audit['database_sha256']
 comparison=json.loads((O/'nics-wiwynn-hyve-rebuild-2026-10-04-comparison.json').read_text());assert len(comparison)==43 and all(r['original']==r['rebuilt'] and r['only_original']==r['only_rebuilt']==0 for r in comparison.values())
 c=Catalog(REBUILD,read_only=True);check(c.db);c.db.close()
 proof(CAND,'candidate',B/'candidate-catalog.html')
 frozen();assert sha(LIVE)==BEFORE
 backup=D/('devices-before-'+TAG+'.sqlite');staging=D/'.devices-nics-wiwynn-hyve-promote.sqlite';cp(LIVE,backup);cp(CAND,staging);os.replace(staging,LIVE)
 save(O/(TAG+'-promotion.json'),dict(previous_sha256=BEFORE,database_sha256=sha(LIVE),backup=str(backup),clean_rebuild=str(REBUILD),preservation_audit=str(O/(TAG+'-preservation.json')),matched_tables=43,tests=checkpoint['tests']))
 for script in ('scripts/refresh_exports.py','scripts/refresh_collection_reports.py'):
  with (B/(Path(script).stem+'.log')).open('w') as log: subprocess.run([sys.executable,script],stdout=log,stderr=subprocess.STDOUT,check=True)
 subprocess.run([sys.executable,'scripts/compare_rebuild.py',str(REBUILD),'--database',str(LIVE),'--output-prefix','rebuild'],check=True)
 proof(LIVE,'final',O/'catalog.html')
 _,manifest=read_manifest(D/'import-manifest.json');c=Catalog(LIVE,read_only=True);db=c.db;check(db)
 pdfs=[]
 for r in db.execute('SELECT d.*,s.cache_path FROM documents d JOIN sources s ON s.id=d.source_id'):
  p=Path(r['cache_path']).parent/r['pdf_file'];assert sha(p)==r['pdf_sha256'];pdfs.append(dict(source=r['source_id'],path=str(p),sha256=sha(p)))
 assert len(pdfs)==55
 counts={k:db.execute('SELECT count(*) FROM '+k).fetchone()[0] for k in ('devices','records','sources','documents','document_pages','inspur_support_entries')};c.db.close()
 current=sha(LIVE)
 for name in ('latest-export-audit','collection-reports-audit','hyperscaler-inventory-audit',TAG+'-final-proof',TAG+'-promotion'):
  assert json.loads((O/(name+'.json')).read_text())['database_sha256']==current,name
 assert json.loads((O/'latest-export-audit.json').read_text())['lookup_reports_refreshed']==98
 frozen();assert sha(LIVE)==current
 save(O/(TAG+'-final-verification.json'),dict(database_sha256=current,schema=28,counts=counts,manifest_jobs=len(manifest['imports']),hashed_inputs=len(manifest['files']),verified_pdfs=pdfs,lookup_reports_refreshed=98,old_rows_preserved=True,owned_evidence='9 documents + 1 reviewed label',bytedance_hits=0,hyperscaler_records=22,manufacturer_factory_counts_unchanged=True,read_only_proof=True,rebuild_tables=43,tests=checkpoint['tests']))
 print(json.dumps(dict(database_sha256=current,counts=counts,manifest_jobs=len(manifest['imports']),hashed_inputs=len(manifest['files'])),indent=2))
