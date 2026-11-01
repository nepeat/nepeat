import json
from pathlib import Path
from devicehunt import nigeria
from devicehunt.catalog import Catalog,digest
c=Catalog(Path('data/devices.sqlite'));before={t:c.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ('devices','records','sources')}
old=set(r[0] for r in c.db.execute('SELECT id FROM devices'))
result=nigeria.import_bundle(c,Path('data/raw/nigeria-ncc/bundle.json'))
after={t:c.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in before}
new=[dict(r) for r in c.db.execute('SELECT * FROM devices') if r['id'] not in old]
Path('data/exports/nigeria-import-audit.json').write_text(json.dumps(dict(before=before,after=after,import_result=result,new_identities=new),ensure_ascii=False,indent=2)+'\n')
mfile=Path('data/import-manifest.json');m=json.loads(mfile.read_text())
job=dict(source='nigeria-ncc',path='raw/nigeria-ncc/bundle.json');assert job not in m['imports'];m['imports'].append(job)
for p in Path('data/raw/nigeria-ncc').iterdir():
 if p.is_file():m['files'][str(p.relative_to('data'))]=digest(p.read_bytes())
m['expected']['devices']=after['devices'];m['expected']['records']=after['records'];m['expected']['sources'][nigeria.SOURCE]=result['registry_rows'];m['expected']['nigeria_equipment']=result['registry_rows']
mfile.write_text(json.dumps(m,ensure_ascii=False,indent=2)+'\n');c.db.close()
print(json.dumps(dict(before=before,after=after,result=result,new_identities=new,jobs=len(m['imports']),files=len(m['files'])),indent=2))
