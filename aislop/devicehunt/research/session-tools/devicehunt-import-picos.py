import json
from pathlib import Path
from devicehunt.catalog import Catalog,digest
from devicehunt import picos
from devicehunt.__main__ import write_rows
c=Catalog(Path('data/devices.sqlite'));before={r[0] for r in c.db.execute('SELECT id FROM devices')}
result=picos.import_bundle(c,Path('data/raw/picos45/bundle.json'))
new=[dict(r) for r in c.db.execute('SELECT id,brand,model FROM devices') if r['id'] not in before]
rows=[]
for r in c.db.execute('SELECT source_key,raw_json FROM records WHERE source_id=? ORDER BY source_key',(picos.SOURCE,)):
 rows.append(json.loads(r['raw_json'])['table_row'])
out=Path('data/exports');(out/'picos45-platforms.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n')
with (out/'picos45-platforms.csv').open('w',newline='') as f:write_rows(rows,'csv',f)
audit=dict(result=result,new_identities=new,counts={t:c.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ['devices','records','sources']})
(out/'picos45-import-audit.json').write_text(json.dumps(audit,indent=2)+'\n')
p=Path('data/import-manifest.json');m=json.loads(p.read_text());job=dict(source='picos-hardware',path='raw/picos45/bundle.json');assert job not in m['imports'];m['imports'].append(job)
for f in [job['path'],'raw/picos45/edgecore-accton.html']:m['files'][f]=digest((p.parent/f).read_bytes())
m['expected']['devices']=audit['counts']['devices'];m['expected']['records']=audit['counts']['records'];m['expected']['sources']=dict(c.db.execute('SELECT id,records_seen FROM sources'))
p.write_text(json.dumps(m,indent=2)+'\n');c.db.close();print(json.dumps(audit,indent=2))
