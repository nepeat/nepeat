import json,hashlib
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt import validation,lookup,vendor_inventory
from devicehunt.__main__ import report,write_rows
root=Path.cwd();path=root/'data/devices.sqlite';out=root/'data/exports'
before=hashlib.sha256(path.read_bytes()).hexdigest();c=Catalog(path,read_only=True);db=c.db
base=validation.run(db,root/'validation/owned-hardware.json')
(out/'inventory-validation.json').write_text(json.dumps(base,ensure_ascii=False,indent=2)+'\n')
counts={}
for scope in ['manufacturer','factory','all']:
    result=vendor_inventory.run(db,scope=scope,limit=-1)
    (out/('vendor-inventory-'+scope+'.json')).write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    with (out/('vendor-inventory-'+scope+'.csv')).open('w',newline='') as stream:write_rows(vendor_inventory.flat_rows(result),'csv',stream)
    counts[scope]={k:result[k] for k in ['identity_target_pairs','unique_identities','counts_by_target']}
report(c,out/'catalog.html')
sql="""SELECT d.id,d.model,a.company,a.target,a.role,r.source_key,r.source_url
FROM records r JOIN devices d ON d.id=r.device_id
JOIN affiliations a ON a.record_id=r.id
WHERE r.source_id='fcc-sporton-cover-roles' ORDER BY d.model,a.role"""
rows=[dict(r) for r in db.execute(sql)]
(out/'fcc-sporton-roles.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n')
with (out/'fcc-sporton-roles.csv').open('w',newline='') as stream:write_rows(rows,'csv',stream)
for query,name in [('ECW7210-L','lookup-fcc-sporton-oem'),('EW-7822GMn','lookup-fcc-sporton-edimax')]:
    (out/(name+'.json')).write_text(json.dumps(lookup.run(db,query),ensure_ascii=False,indent=2)+'\n')
refreshed=0
for p in out.glob('lookup-*.json'):
    previous=json.loads(p.read_text())
    if 'query' in previous and 'limit_per_dataset' in previous:
        p.write_text(json.dumps(lookup.run(db,previous['query'],previous['limit_per_dataset']),ensure_ascii=False,indent=2)+'\n')
        refreshed+=1
c.db.close();assert before==hashlib.sha256(path.read_bytes()).hexdigest()
audit={'database_sha256':before,'read_only_exports_unchanged_database':True,'owned_baseline':base['summary'],
       'inventories':counts,'lookup_reports_refreshed':refreshed}
(out/'fcc-sporton-export-audit.json').write_text(json.dumps(audit,indent=2)+'\n')
print(json.dumps(audit,indent=2))
