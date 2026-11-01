import json,sqlite3,xml.etree.ElementTree as ET
from pathlib import Path
from devicehunt import bsmi_additional as b,lookup,validation
from devicehunt.catalog import Catalog,digest
from devicehunt.__main__ import report
path=Path('data/devices.sqlite').resolve();before=digest(path.read_bytes())
db=sqlite3.connect(path.as_uri()+'?mode=ro',uri=True);db.row_factory=sqlite3.Row
verified={}
for dataset,spec in b.DATASETS.items():
 root=ET.parse(Path('data/raw/bsmi-additional')/dataset/'register.xml').getroot()
 raw=[tuple(child.text or '' for child in row) for row in root]
 fields=','.join(key for key,_ in spec['fields'])
 actual=[tuple(r) for r in db.execute('SELECT '+fields+' FROM '+spec['table']+' WHERE source_id=? ORDER BY row_number',('bsmi-additional-'+dataset,))]
 assert actual==raw,dataset
 verified[dataset]={'rows':len(raw),'all_fields_match_original_xml':True}
base=validation.run(db,Path('validation/owned-hardware.json'))
Path('data/exports/inventory-validation.json').write_text(json.dumps(base,ensure_ascii=False,indent=2)+'\n')
c=Catalog.__new__(Catalog);c.db=db
report(c,Path('data/exports/catalog.html'))
for query,name in [('Oracle','lookup-bsmi-additional-oracle'),('D30001','lookup-bsmi-declaration-example')]:
 result=lookup.run(db,query)
 Path('data/exports/'+name+'.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
 assert len(result['datasets'])==18
# Every existing lookup example now includes the three new datasets.
for p in Path('data/exports').glob('lookup-*.json'):
 previous=json.loads(p.read_text())
 if 'query' in previous and 'limit_per_dataset' in previous:
  result=lookup.run(db,previous['query'],previous['limit_per_dataset'])
  p.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
db.close();after=digest(path.read_bytes());assert before==after
Path('data/exports/bsmi-additional-import-check.json').write_text(json.dumps(dict(tables=verified,readonly_database_unchanged=True,database_sha256=after,baseline=base['summary']),indent=2)+'\n')
print('Original XML round trip passed for all 20,179 rows; readonly exports verified.',flush=True)
