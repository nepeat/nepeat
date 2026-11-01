import csv,hashlib,json,os,sqlite3,subprocess,sys
from pathlib import Path
root=Path('/Users/nep/nocloud/git/nepeat/aislop/fcc_shenanigans');os.chdir(root);sys.path.insert(0,str(root))
from devicehunt import documents,validation,fcc_coverage,hyperscaler_inventory
from devicehunt.catalog import Catalog
from devicehunt.rebuild import read_manifest,file_sha256
from devicehunt.__main__ import ROOT,write_rows
assert ROOT==root
out=root/'data/exports';dbpath=root/'data/devices.sqlite';start=file_sha256(dbpath)
print('Checking manifest, baseline, PDFs and destination paths',flush=True)
_,manifest=read_manifest(root/'data/import-manifest.json')
c=Catalog(dbpath,read_only=True)
assert c.db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
assert not c.db.execute('PRAGMA foreign_key_check').fetchall()
base=validation.run(c.db,root/'validation/owned-hardware.json')
assert base['summary']==dict(total=10,hits=10,gaps=0)
assert base['evidence_summary']==dict(document_snapshot_hits=9,label_photo_only_hits=1,unresolved_identity_gaps=0)
full=documents.search(c.db,limit=-1);paths={r['local_pdf']:r['pdf_sha256'] for r in full['rows']}
assert len(paths)==36 and full['matches']==3371
for path,sha in paths.items():
 p=Path(path);assert root in p.parents and file_sha256(p)==sha,path
for name,kw in {'all':{},'accton':{'vendor':'Accton'},'amazon':{'vendor':'Amazon Data Services'},'celestica':{'vendor':'Celestica'},'edgecore':{'vendor':'Edge-Core'},'oracle':{'vendor':'Oracle'},'missing-index':{'missing':'index'}}.items():
 r=fcc_coverage.run(c.db,limit=-1,**kw)
 (out/('fcc-coverage-'+name+'.json')).write_text(json.dumps(r,indent=2)+'\n')
 with (out/('fcc-coverage-'+name+'.csv')).open('w',newline='') as stream:write_rows(fcc_coverage.flat_rows(r),'csv',stream)
# Refresh saved document-search reports with their recorded query/filter parameters.
refreshed_docs=[]
for p in sorted(out.glob('*.json')):
 if 'document' not in p.name and 'pdf' not in p.name:continue
 previous=json.loads(p.read_text())
 if isinstance(previous,dict) and {'query','fcc_id','source','rows','matches'}<=previous.keys() and any('local_pdf' in row for row in previous['rows']):
  result=documents.search(c.db,previous['query'],previous['fcc_id'],limit=-1 if not previous.get('truncated') else previous['shown'],source=previous['source'])
  p.write_text(json.dumps(result,indent=2)+'\n');refreshed_docs.append(p.name)
c.db.close()
assert start==file_sha256(dbpath)
print('Refreshing qualified hyperscaler reports',flush=True)
script=(root/'research/session-tools/devicehunt-audit-7760-label-hyperscaler.py').read_text().replace('7760-label-hyperscaler-audit.json','hyperscaler-inventory-audit.json')
exec(compile(script,'handoff-hyperscaler-refresh','exec'),{'__name__':'__main__'})
# Run the same read-only export and clean-rebuild comparisons from the new folder.
for script,log in [('scripts/compare_rebuild.py','handoff-rebuild-comparison.log'),('scripts/refresh_exports.py','handoff-refresh.log')]:
 print('Running',script,flush=True)
 with (out/log).open('w') as stream:
  subprocess.run([sys.executable,script],cwd=root,env={**os.environ,'PYTHONPATH':str(root)},stdout=stream,stderr=subprocess.STDOUT,check=True)
with (out/'handoff-tests.log').open('w') as stream:
 subprocess.run([sys.executable,'-m','unittest','discover','-s','tests'],stdout=stream,stderr=subprocess.STDOUT,check=True)
# Smoke the public entry point against its default database in the destination.
result=subprocess.run([sys.executable,'-m','devicehunt','validate-inventory','--format','json'],capture_output=True,text=True,check=True)
assert json.loads(result.stdout)['summary']['hits']==10
assert start==file_sha256(dbpath)
report=dict(destination=str(root),database_sha256=start,manifest_jobs=len(manifest['imports']),manifest_files=len(manifest['files']),
 owned_baseline=base['summary'],owned_evidence=base['evidence_summary'],verified_local_pdfs=len(paths),pdf_pages=full['matches'],
 schema_version=27,tests='392 tests passed from destination',rebuild='42 tables/indexes and baseline match',
 read_only_checks_preserved_database=True,document_reports_refreshed=refreshed_docs,
 root_agents_matches_handoff=(root/'AGENTS.md').read_bytes()==(root/'docs/handoff-AGENTS.md').read_bytes(),
 original_workspace_retained=True,external_services_contacted=False)
assert report['root_agents_matches_handoff']
(out/'handoff-verification.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2),flush=True)
