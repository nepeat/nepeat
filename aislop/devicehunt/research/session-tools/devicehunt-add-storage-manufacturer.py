import json,hashlib,sqlite3
from pathlib import Path
from devicehunt.fcc_index import document
from devicehunt.catalog import Catalog
from devicehunt.importers import curated
from devicehunt import vendor_inventory
root=Path.cwd();raw=root/'data/raw/celestica-storage-manufacturer'
models={'SC6100':('2U 24 SSD High Availability All-Flash PCIe® Gen 5 Storage Controller','storage controller'),
        'SD4100':('2U 12 HDD SAS-4 Storage Expansion System','storage enclosure'),
        'SD6200':('4U 90 HDD Dense Storage SAS-4 Expansion System','storage enclosure')}
review={'reviewed_on':'2026-09-27','sources':[],'devices':[]};checks=[]
fetch=json.loads((raw/'fetch-report.json').read_text())
scope=('Explicit manufacturer statement on the model-specific Celestica product page, saved as original HTML. '
       'Manufacturer is the source-stated Celestica name; no subsidiary, physical factory, current stock or individual unit provenance is established. '
       'Only the exact page model is covered. Chassis description is a platform specification, not a promise of installed drives or auction-unit configuration. '
       'Application-workload marketing is not hyperscaler deployment evidence; no aliases, ordering codes or deployment assertions are inferred.')
for model,(subtitle,kind) in models.items():
 p=raw/(model.lower()+'.html');body=p.read_bytes();sha=hashlib.sha256(body).hexdigest()
 fetched=next(r for r in fetch if r['model']==model);url=fetched['url']
 assert fetched['status']==200 and fetched['final_url']==url and fetched['sha256']==sha
 tree=document(body.decode('utf8')); headings=[n.text() for n in tree.walk('h1')]
 assert [n.text() for n in tree.walk('title')]==['Storage Expansion System - '+model]
 assert headings[:2]==[model,subtitle]
 claims=[n.text() for n in tree.walk('p') if 'manufactured by' in n.text().lower()]
 claim='Designed and manufactured by Celestica with '+('30+' if model=='SD4100' else 'nearly 30')+' years of experience in storage'
 assert claims==[claim]
 sid='celestica-storage-manufacturer-'+model.lower()
 review['sources'].append(dict(id=sid,url=url,tier='vendor',license='Factual model and manufacturer statements; original page rights retained',
                              snapshot=str(p.relative_to(root/'data')),coverage=scope))
 row=dict(source=sid,key=model,url=url,brand='Celestica',model=model,title='Celestica '+model,kind=kind,
          affiliations=[['Celestica','brand'],['Celestica','manufacturer']],
          facts=[['manufacturer_statement',claim],['physical_description',subtitle],['capture_method','Original HTML; model h1, product subtitle h1 and manufacturer paragraph reviewed'],['source_html_sha256',sha],['evidence_scope',scope]])
 review['devices'].append(row)
 checks.append(dict(model=model,url=url,html_sha256=sha,model_heading=headings[0],physical_description=subtitle,manufacturer_statement=claim))
path=root/'data/celestica-storage-manufacturer-reviewed.json';assert not path.exists();path.write_text(json.dumps(review,ensure_ascii=False,indent=2)+'\n')
(raw/'review-notes.json').write_text(json.dumps({'reviewed_on':'2026-09-27','scope':scope,'checks':checks},ensure_ascii=False,indent=2)+'\n')
c=Catalog(root/'data/devices.sqlite');db=c.db
before=vendor_inventory.run(db,scope='manufacturer',limit=-1)
counts_before={t:db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['devices','records','sources','affiliations','identifiers','facts','hyperscaler_evidence']}
assert all(not any(r['brand']=='Celestica' and r['model']==model for r in before['rows']) for model in models)
assert curated(c,path)==3
after=vendor_inventory.run(db,scope='manufacturer',limit=-1)
assert after['identity_target_pairs']==before['identity_target_pairs']+3
new=[r for r in after['rows'] if r['brand']=='Celestica' and r['model'] in models]
assert len(new)==3
counts_after={t:db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in counts_before}
assert counts_before['devices']==counts_after['devices'] and counts_before['records']+3==counts_after['records']
assert counts_before['hyperscaler_evidence']==counts_after['hyperscaler_evidence'] and counts_before['identifiers']==counts_after['identifiers']
assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok';assert not db.execute('PRAGMA foreign_key_check').fetchall()
manifest=root/'data/import-manifest.json';m=json.loads(manifest.read_text())
m['imports'].append({'source':'curated','path':path.name})
for p in [path,*sorted(raw.iterdir())]:m['files'][str(p.relative_to(root/'data'))]=hashlib.sha256(p.read_bytes()).hexdigest()
m['expected']['devices']=counts_after['devices'];m['expected']['records']=counts_after['records'];m['expected']['sources']=dict(db.execute('SELECT id,records_seen FROM sources'))
manifest.write_text(json.dumps(m,ensure_ascii=False,indent=2)+'\n');db.close()
audit=dict(counts_before=counts_before,counts_after=counts_after,manufacturer_inventory_before=before['identity_target_pairs'],manufacturer_inventory_after=after['identity_target_pairs'],added_evidence=new,checks=checks,manifest_jobs=len(m['imports']),manifest_files=len(m['files']))
(root/'data/exports/celestica-storage-manufacturer-import-audit.json').write_text(json.dumps(audit,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({k:v for k,v in audit.items() if k not in ('added_evidence','checks')},indent=2))
