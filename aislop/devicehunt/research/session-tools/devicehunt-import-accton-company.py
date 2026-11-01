import json
from pathlib import Path
from devicehunt.catalog import Catalog,digest
from devicehunt import rra, vendor_inventory
from devicehunt.__main__ import write_rows
c=Catalog(Path('data/devices.sqlite'));sid='rra-official-query-e5984834207f'
before={r['id'] for r in c.db.execute('SELECT id FROM devices')}
keys={r[0] for r in c.db.execute('SELECT source_key FROM records WHERE source_id=?',(sid,))}
result=rra.import_bundle(c,Path('data/raw/rra-accton-official/bundle.json'))
added=[dict(r) for r in c.db.execute('SELECT r.source_key,d.brand,d.model,a.company,a.target,a.role,r.source_url FROM records r JOIN devices d ON d.id=r.device_id JOIN affiliations a ON a.record_id=r.id WHERE r.source_id=? AND a.role=? ORDER BY d.model',(sid,'registry_manufacturer')) if r['source_key'] not in keys]
new=[dict(r) for r in c.db.execute('SELECT id,brand,model FROM devices') if r['id'] not in before]
unresolved=[]
for row in c.db.execute('SELECT manufacturer,count(*) AS approval_rows FROM rra_equipment WHERE source_id=? GROUP BY manufacturer ORDER BY count(*) DESC',(sid,)):
 if not rra.company_target(row['manufacturer'],rra.rra_company_names.load(Path('data/raw/rra-accton-official'),json.loads(Path('data/raw/rra-accton-official/bundle.json').read_text())['company_evidence'])):unresolved.append(dict(row))
audit=dict(result=result,added_assertions=len(added),added_identity_count=len({(r['brand'],r['model']) for r in added}),new_identity_count=len(new),new_identities=new,unresolved_manufacturers=unresolved,counts={t:c.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['devices','records','sources']})
Path('data/exports/rra-accton-company-review.json').write_text(json.dumps(audit,indent=2)+'\n')
Path('data/exports/accton-manufacturer-additions.json').write_text(json.dumps(added,indent=2)+'\n')
with Path('data/exports/accton-manufacturer-additions.csv').open('w',newline='') as out:write_rows(added,'csv',out)
p=Path('data/import-manifest.json');m=json.loads(p.read_text())
for f in ['raw/rra-accton-official/bundle.json','raw/rra-accton-official/accton-singapore-2013.json']:m['files'][f]=digest((p.parent/f).read_bytes())
m['expected']['devices']=audit['counts']['devices'];m['expected']['records']=audit['counts']['records'];m['expected']['sources']=dict(c.db.execute('SELECT id,records_seen FROM sources'))
p.write_text(json.dumps(m,indent=2)+'\n');c.db.close();print(json.dumps(audit,indent=2))
