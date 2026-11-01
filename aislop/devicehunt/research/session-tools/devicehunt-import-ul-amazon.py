import json
from pathlib import Path
from devicehunt.catalog import Catalog,digest
from devicehunt import ul_amazon as ul, vendor_inventory
from devicehunt.__main__ import write_rows
c=Catalog(Path('data/devices.sqlite'))
before={r['id'] for r in c.db.execute('SELECT id FROM devices')}
result=ul.import_capture(c,Path('data/raw/ul-amazon-platforms.json'))
new=[dict(r) for r in c.db.execute('SELECT id,brand,model FROM devices') if r['id'] not in before]
rows=[dict(r) for r in c.db.execute('SELECT d.model,a.company,a.target,a.role,r.source_url FROM records r JOIN devices d ON d.id=r.device_id JOIN affiliations a ON a.record_id=r.id WHERE r.source_id=? ORDER BY d.model',(ul.SOURCE,))]
Path('data/exports/ul-amazon-models.json').write_text(json.dumps(rows,indent=2)+'\n')
with Path('data/exports/ul-amazon-models.csv').open('w',newline='') as out:write_rows(rows,'csv',out)
audit={'result':result,'new_identities':new,'new_identity_count':len(new),'counts':{t:c.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['devices','records','sources']}}
Path('data/exports/ul-amazon-audit.json').write_text(json.dumps(audit,indent=2)+'\n')
p=Path('data/import-manifest.json');manifest=json.loads(p.read_text());job={'source':'ul-amazon-platforms','path':'raw/ul-amazon-platforms.json'}
assert job not in manifest['imports']
manifest['imports'].append(job);manifest['files'][job['path']]=digest((p.parent/job['path']).read_bytes())
manifest['expected']['devices']=audit['counts']['devices'];manifest['expected']['records']=audit['counts']['records'];manifest['expected']['sources']=dict(c.db.execute('SELECT id,records_seen FROM sources'))
p.write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps(audit,indent=2))
c.db.close()
