import json,sqlite3
from pathlib import Path
from devicehunt.catalog import Catalog,digest
from devicehunt import validation,lookup,vendor_inventory
from devicehunt.__main__ import report,write_rows
path=Path('data/devices.sqlite').resolve();before=digest(path.read_bytes())
db=sqlite3.connect(path.as_uri()+'?mode=ro',uri=True);db.row_factory=sqlite3.Row
c=Catalog.__new__(Catalog);c.db=db
out=Path('data/exports')
base=validation.run(db,Path('validation/owned-hardware.json'))
(out/'inventory-validation.json').write_text(json.dumps(base,ensure_ascii=False,indent=2)+'\n')
inv=Path('/tmp/devicehunt-celestica-newer-models.json')
inv.write_text(json.dumps(dict(schema_version=1,devices=[dict(identifier=m,vendor='Celestica') for m in ['DS6000','DS6001','DS6010']])))
validated=validation.run(db,inv)
assert validated['summary']['hits']==3
(out/'celestica-networking-snapshot-validation.json').write_text(json.dumps(validated,ensure_ascii=False,indent=2)+'\n')
rows=[dict(r) for r in db.execute("SELECT d.model,r.source_url,f.name,f.value FROM devices d JOIN records r ON r.device_id=d.id JOIN facts f ON f.record_id=r.id WHERE r.source_id='celestica-networking-index' ORDER BY d.model,f.name,f.value")]
with (out/'celestica-networking-index-facts.csv').open('w',newline='') as stream: write_rows(rows,'csv',stream)
for scope in ['manufacturer','factory','all']:
 result=vendor_inventory.run(db,scope=scope,limit=-1)
 (out/('vendor-inventory-'+scope+'.json')).write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
 with (out/('vendor-inventory-'+scope+'.csv')).open('w',newline='') as stream: write_rows(vendor_inventory.flat_rows(result),'csv',stream)
report(c,out/'catalog.html')
for query in ['DS6000','DS6001','DS6010']:
 result=lookup.run(db,query)
 (out/('lookup-'+query.lower()+'.json')).write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
for p in out.glob('lookup-*.json'):
 previous=json.loads(p.read_text())
 if 'query' in previous and 'limit_per_dataset' in previous:
  result=lookup.run(db,previous['query'],previous['limit_per_dataset'])
  p.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
after=digest(path.read_bytes());assert before==after
(out/'celestica-networking-readonly-check.json').write_text(json.dumps(dict(database_sha256=after,readonly_exports_unchanged_database=True,owned_baseline=base['summary'],newer_models=validated['summary']),indent=2)+'\n')
print('Exports refreshed; DS6000/DS6001/DS6010 all corroborated; database unchanged.')
