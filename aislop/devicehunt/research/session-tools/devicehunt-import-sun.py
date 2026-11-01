import json,collections
from pathlib import Path
from devicehunt.catalog import Catalog,digest
from devicehunt import rra,rra_sun
from devicehunt.__main__ import write_rows
root=Path('data/raw/rra-sun-official');p=root/'bundle.json';b,ready,proof=rra_sun.validate(p)
c=Catalog(Path('data/devices.sqlite'));before={r[0] for r in c.db.execute('SELECT id FROM devices')};result=rra_sun.import_bundle(c,p)
new=[dict(r) for r in c.db.execute('SELECT id,brand,model FROM devices') if r['id'] not in before]
rows=[dict(r) for r in c.db.execute("SELECT r.source_key,d.brand,d.model,a.company,a.target,a.role,r.source_url FROM records r JOIN devices d ON d.id=r.device_id JOIN affiliations a ON a.record_id=r.id WHERE r.source_id LIKE 'rra-sun-portfolio-%' ORDER BY d.model,a.role")]
out=Path('data/exports');(out/'sun-legacy-portfolio.json').write_text(json.dumps(rows,indent=2)+'\n')
with (out/'sun-legacy-portfolio.csv').open('w',newline='') as f:write_rows(rows,'csv',f)
audit=dict(result=result,new_identity_count=len(new),new_identities=new,unmapped_manufacturers=dict(collections.Counter(row['manufacturer'] for _,_,row in ready if not rra_sun.company_basis(row['manufacturer']))),csv_comparison=b['csv_comparison'],counts={t:c.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['devices','records','sources','rra_equipment']})
(out/'rra-sun-audit.json').write_text(json.dumps(audit,indent=2)+'\n')
p=Path('data/import-manifest.json');m=json.loads(p.read_text());job=dict(source='rra-sun-portfolio',path='raw/rra-sun-official/bundle.json');assert job not in m['imports'];m['imports'].append(job)
files=['bundle.json',*(b[k]['path'] for k in ['form','csv_before','csv_after','sun_lineage']),*(e['path'] for e in b['details'])]
for f in files:
 local='raw/rra-sun-official/'+f;m['files'][local]=digest((p.parent/local).read_bytes())
for k in ['devices','records','rra_equipment']:m['expected'][k]=audit['counts'][k]
m['expected']['sources']=dict(c.db.execute('SELECT id,records_seen FROM sources'))
p.write_text(json.dumps(m,indent=2)+'\n');c.db.close();print(json.dumps(audit,indent=2))
