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
for query,name in [('HEDML2560','lookup-fcc-gap-ml2560'),('HEDML60LW','lookup-fcc-gap-ml60lw'),('HEDMR3202A','lookup-fcc-gap-mr3202a')]:
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
(out/'fcc-gap-probes-export-audit.json').write_text(json.dumps(audit,indent=2)+'\n')
print(json.dumps(audit,indent=2))
