"""Final read-only handoff audit after all exports finish."""
import json
from pathlib import Path
from devicehunt.catalog import Catalog,TARGETS
from devicehunt.rebuild import file_sha256,read_manifest
from devicehunt import validation
ROOT=Path(__file__).resolve().parents[2];DATA=ROOT/'data';OUT=DATA/'exports'
dbpath=DATA/'devices.sqlite';before=file_sha256(dbpath)
promotion=json.loads((OUT/'bytedance-inspur-promotion-2026-10-04.json').read_text())
assert promotion['database_sha256']==before
_,manifest=read_manifest(DATA/'import-manifest.json')
assert len(manifest['imports'])==106 and len(manifest['files'])==2071
for name in ('latest-export-audit','collection-reports-audit','hyperscaler-inventory-audit'):
    assert json.loads((OUT/(name+'.json')).read_text())['database_sha256']==before,name
latest=json.loads((OUT/'latest-export-audit.json').read_text())
assert latest['owned_baseline']==dict(total=10,hits=10,gaps=0)
assert latest['vendor_examples']==dict(total=2,hits=0,gaps=2)
c=Catalog(dbpath,read_only=True);db=c.db
assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
assert not db.execute('PRAGMA foreign_key_check').fetchall()
counts={name:db.execute('SELECT count(*) FROM '+name).fetchone()[0] for name in ('devices','records','sources','documents','document_pages')}
assert counts==dict(devices=3892,records=5070,sources=231,documents=40,document_pages=3410)
pdfs=[]
for row in db.execute('SELECT d.*,s.cache_path FROM documents d JOIN sources s ON s.id=d.source_id'):
    path=Path(row['cache_path']).parent/row['pdf_file']
    assert file_sha256(path)==row['pdf_sha256'],str(path)
    pdfs.append(dict(source=row['source_id'],path=str(path),sha256=row['pdf_sha256']))
examples=validation.run(db,ROOT/'validation/vendor-examples.json')
assert examples['summary']==latest['vendor_examples']
assert all((ROOT/'data/exports/catalog.html').read_text().count('<option>'+t+'</option>')==1 for t in TARGETS)
c.db.close();assert file_sha256(dbpath)==before
comparison=json.loads((OUT/'rebuild-comparison.json').read_text());assert len(comparison)==42
assert all(r['original']==r['rebuilt'] and r['only_original']==r['only_rebuilt']==0 for r in comparison.values())
result=dict(database_sha256=before,read_only=True,counts=counts,targets=list(TARGETS),manifest_jobs=106,hashed_inputs=2071,
    tests=399,report_script_tests=6,rebuild_tables=42,owned_baseline=latest['owned_baseline'],vendor_examples=latest['vendor_examples'],
    lookup_reports_refreshed=latest['lookup_reports_refreshed'],verified_pdfs=pdfs,
    visual_browser_check='Unavailable: browser policy blocks automated file:// navigation; no bypass attempted.')
(OUT/'bytedance-inspur-final-verification-2026-10-04.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:v for k,v in result.items() if k!='verified_pdfs'},indent=2))
