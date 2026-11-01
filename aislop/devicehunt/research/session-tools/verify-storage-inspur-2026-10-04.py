"""Verify final source/data preservation, exports and raw discovery boundaries."""
import json
from pathlib import Path
from devicehunt.catalog import Catalog,TARGETS
from devicehunt.rebuild import read_manifest,file_sha256
from devicehunt import validation

ROOT=Path(__file__).resolve().parents[2]
DATA=ROOT/'data'
OUT=DATA/'exports'
before=file_sha256(DATA/'devices.sqlite')
for name in ('storage-inspur-promotion-2026-10-04','latest-export-audit','collection-reports-audit',
             'hyperscaler-inventory-audit','storage-inspur-ui-audit-2026-10-04','storage-inspur-cli-checks-2026-10-04'):
    assert json.loads((OUT/(name+'.json')).read_text())['database_sha256']==before,name
_,manifest=read_manifest(DATA/'import-manifest.json')
assert len(manifest['imports'])==112 and len(manifest['files'])==2120
c=Catalog(DATA/'devices.sqlite',read_only=True)
db=c.db
assert db.execute('PRAGMA user_version').fetchone()[0]==28
assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
assert not db.execute('PRAGMA foreign_key_check').fetchall()
counts={name:db.execute('SELECT count(*) FROM '+name).fetchone()[0]
        for name in ('devices','records','sources','documents','document_pages','inspur_support_entries')}
assert counts==dict(devices=3942,records=5143,sources=253,documents=46,document_pages=3523,inspur_support_entries=1064)
raw=json.loads((DATA/'raw/storage-inspur-2026-10-04/inspur-product-names.json').read_text())
assert [json.loads(r[0]) for r in db.execute('SELECT raw_json FROM inspur_support_entries ORDER BY row_number')]==raw
assert db.execute("SELECT count(*) FROM records WHERE source_id='inspur-support-models'").fetchone()[0]==0
assert db.execute('SELECT count(DISTINCT name) FROM inspur_support_entries').fetchone()[0]==1055
pdfs=[]
for row in db.execute('SELECT d.*,s.cache_path FROM documents d JOIN sources s ON s.id=d.source_id'):
    path=Path(row['cache_path']).parent/row['pdf_file']
    assert file_sha256(path)==row['pdf_sha256']
    pdfs.append(dict(source=row['source_id'],path=str(path),sha256=row['pdf_sha256']))
base=validation.run(db,ROOT/'validation/owned-hardware.json')
examples=validation.run(db,ROOT/'validation/vendor-examples.json')
assert base['summary']==dict(total=10,hits=10,gaps=0)
assert base['evidence_summary']==dict(document_snapshot_hits=9,label_photo_only_hits=1,unresolved_identity_gaps=0)
assert examples['summary']==dict(total=2,hits=0,gaps=2)
assert db.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0]==21
db.close()
assert file_sha256(DATA/'devices.sqlite')==before
comparison=json.loads((OUT/'rebuild-comparison.json').read_text())
assert len(comparison)==43
assert all(r['original']==r['rebuilt'] and r['only_original']==r['only_rebuilt']==0 for r in comparison.values())
latest=json.loads((OUT/'latest-export-audit.json').read_text())
assert latest['lookup_reports_refreshed']==98
assert all((OUT/'catalog.html').read_text().count('<option>'+t+'</option>')==1 for t in TARGETS)
result=dict(database_sha256=before,schema=28,read_only=True,counts=counts,targets=list(TARGETS),
    manifest_jobs=112,hashed_inputs=2120,tests=408,report_script_tests=6,rebuild_tables=43,
    owned_baseline=base['summary'],vendor_examples=examples['summary'],verified_pdfs=pdfs,
    lookup_reports_refreshed=98,raw_support_rows_match_original=True,raw_support_catalog_assertions=0,
    browser_visual_check='No screenshot; generated HTML/script exercised in DOM harness. Prior file:// restriction respected.')
(OUT/'storage-inspur-final-verification-2026-10-04.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:v for k,v in result.items() if k!='verified_pdfs'},indent=2))
