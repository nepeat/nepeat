import json,sqlite3
from pathlib import Path
from devicehunt.catalog import Catalog,digest
from devicehunt import validation,lookup,vendor_inventory
from devicehunt.__main__ import report,write_rows
path=Path('data/devices.sqlite').resolve();before=digest(path.read_bytes())
c=Catalog(path,read_only=True);db=c.db;out=Path('data/exports')
base=validation.run(db,Path('validation/owned-hardware.json'))
(out/'inventory-validation.json').write_text(json.dumps(base,ensure_ascii=False,indent=2)+'\n')
rows=[dict(r) for r in db.execute("SELECT r.source_id,r.source_key,d.brand,d.model,a.company,a.target,a.role,r.source_url FROM records r JOIN devices d ON d.id=r.device_id JOIN affiliations a ON a.record_id=r.id WHERE r.source_id IN ('bsmi-reviewed-targets','ncc-reviewed-targets','bsmi-product-reviewed-targets') AND a.target IS NOT NULL ORDER BY r.source_id,r.source_key,a.target,a.role,a.company")]
(out/'taiwan-target-roles.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n')
with (out/'taiwan-target-roles.csv').open('w',newline='') as stream:write_rows(rows,'csv',stream)
for scope in ['manufacturer','factory','all']:
 result=vendor_inventory.run(db,scope=scope,limit=-1)
 (out/('vendor-inventory-'+scope+'.json')).write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
 with (out/('vendor-inventory-'+scope+'.csv')).open('w',newline='') as stream:write_rows(vendor_inventory.flat_rows(result),'csv',stream)
 print(scope,result['identity_target_pairs'],result['unique_identities'],result['counts_by_target'],flush=True)
report(c,out/'catalog.html')
for query,name in [('Celestica','lookup-rra-celestica-company'),('AZ9074-64X-DC-42','lookup-rra-az9074'),('VSX 3000','lookup-rra-cross-brand-example')]:
 result=lookup.run(db,query)
 (out/(name+'.json')).write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
for p in out.glob('lookup-*.json'):
 previous=json.loads(p.read_text())
 if 'query' in previous and 'limit_per_dataset' in previous:
  result=lookup.run(db,previous['query'],previous['limit_per_dataset'])
  p.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
c.db.close();after=digest(path.read_bytes());assert before==after
(out/'rra-official-readonly-check.json').write_text(json.dumps(dict(database_sha256=after,readonly_exports_unchanged_database=True,owned_baseline=base['summary']),indent=2)+'\n')
print('All exports refreshed; database bytes unchanged.')
