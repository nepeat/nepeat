import json, shutil
from pathlib import Path
from devicehunt.catalog import Catalog, digest
from devicehunt import rra, rra_company_names
root=Path('data/raw/rra-celestica-official'); bundle=root/'bundle.json'
data=json.loads(bundle.read_text()); old=rra_company_names.load(root,data['company_evidence'])
entry=dict(path='celestica-historical-companies.json',sha256=digest((root/'celestica-historical-companies.json').read_bytes()))
new=rra_company_names.load(root,entry)
c=Catalog(Path('data/devices.sqlite')); sid='rra-official-query-41e4ca912b33'
before={t:c.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in ('devices','records','sources')}
rows=[dict(r) for r in c.db.execute('SELECT * FROM rra_equipment WHERE source_id=?',(sid,))]
added=[r for r in rows if rra.company_target(r['manufacturer'],new) and not rra.company_target(r['manufacturer'],old)]
assert len(added)==16
backup=root/'bundle-before-historical-companies.json'
assert not backup.exists(); shutil.copyfile(bundle,backup)
data['company_evidence']=entry;bundle.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
result=rra.import_bundle(c,bundle)
after={t:c.db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in before}
audit=dict(before=before,after=after,import_result=result,additional_manufacturer_rows=added,
    eligible_manufacturer_rows=sum(bool(rra.company_target(r['manufacturer'],new)) for r in rows))
Path('data/exports/celestica-historical-import-audit.json').write_text(json.dumps(audit,ensure_ascii=False,indent=2)+'\n')
manifest=Path('data/import-manifest.json');m=json.loads(manifest.read_text())
for name in ('bundle.json','celestica-historical-companies.json','celestica-sec-8.1-reviewed.json','celestica-archived-21.1-reviewed.json'):
    file=root/name;m['files'][str(file.relative_to('data'))]=digest(file.read_bytes())
m['expected']['devices']=after['devices'];m['expected']['records']=after['records']
manifest.write_text(json.dumps(m,ensure_ascii=False,indent=2)+'\n')
c.db.close()
print(json.dumps(dict(before=before,after=after,result=result,added_rows=len(added),jobs=len(m['imports']),files=len(m['files']))))
