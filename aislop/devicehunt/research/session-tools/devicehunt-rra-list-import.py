import json
from pathlib import Path
from collections import Counter
from devicehunt.catalog import Catalog, digest
from devicehunt import rra, rra_company_names, rra_manufacturer_lists as lists, validation, vendor_inventory
from devicehunt.__main__ import write_rows

root=Path.cwd(); raw=root/'data/raw'; out=root/'data/exports'
captures=[]; keys=set();review=[]; counts=Counter()
for folder in ('rra-celestica-official','rra-accton-official','rra-oracle-official'):
    path=raw/folder/'bundle.json'
    captures.append(dict(path=path.relative_to(raw).as_posix(),sha256=digest(path.read_bytes())))
    data,ready=rra.validate(path)
    mapped=rra_company_names.load(path.parent,data.get('company_evidence'))
    for _,entry,row in ready:
        parts=lists.REVIEWED.get(row['manufacturer'])
        if not parts:continue
        targets={rra.company_target(name,mapped) for name in parts}-{None}
        for target in sorted(targets):
            key=(row['certificate_number'],target)
            if key in keys:continue
            keys.add(key); models=rra.models(row);counts[target]+=len(models)
            review.append(dict(certificate=row['certificate_number'],manufacturer=row['manufacturer'],
                selected_names=[name for name in parts if rra.company_target(name,mapped)==target],
                context_parts=parts,target=target,main_model=row['main_model'],derived_models=row['derived_models'],
                models=models,source_url=entry['url'],detail_sha256=entry['sha256']))
bundle=dict(schema_version=1,kind=lists.SOURCE,reviewed_on='2026-09-27',captures=captures,
    expected=dict(approval_target_pairs=len(keys),model_assertions=sum(counts.values()),assertions_by_target=dict(sorted(counts.items()))))
path=raw/'rra-manufacturer-lists.json';path.write_text(json.dumps(bundle,indent=2)+'\n')
out.joinpath('rra-manufacturer-lists-review.json').write_text(json.dumps(review,ensure_ascii=False,indent=2)+'\n')
print(json.dumps(bundle['expected']),flush=True)
c=Catalog(root/'data/devices.sqlite');db=c.db
before_records={r[0] for r in db.execute('SELECT id FROM records')}
before_devices={r[0] for r in db.execute('SELECT id FROM devices')}
before_baseline=validation.run(db,root/'validation/owned-hardware.json')
result=lists.import_bundle(c,path)
after_records={r[0] for r in db.execute('SELECT id FROM records')}
after_devices={r[0] for r in db.execute('SELECT id FROM devices')}
assert before_records <= after_records and before_devices <= after_devices
added=[dict(r) for r in db.execute('SELECT r.id AS record_id,d.id AS device_id,d.brand,d.model,r.source_key,r.source_url '
    'FROM records r JOIN devices d ON d.id=r.device_id WHERE r.source_id=? ORDER BY d.brand,d.model,r.source_key',(lists.SOURCE,))]
assert len(added)==result['model_assertions']==len(after_records-before_records)
for row in added:row['new_catalog_identity']=row['device_id'] not in before_devices
after_baseline=validation.run(db,root/'validation/owned-hardware.json')
assert before_baseline['results']==after_baseline['results']
inventory=vendor_inventory.run(db,limit=0)
audit=dict(import_result=result,added_assertions=len(added),new_catalog_identities=len(after_devices-before_devices),
    current_devices=len(after_devices),current_records=len(after_records),manufacturer_inventory=inventory,
    unchanged_baseline_results=True,baseline=after_baseline['summary'])
for name,value in [('rra-manufacturer-lists-import-audit',audit),('rra-manufacturer-lists-additions',added)]:
    out.joinpath(name+'.json').write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n')
with out.joinpath('rra-manufacturer-lists-additions.csv').open('w',newline='') as stream:write_rows(added,'csv',stream)
manifest_path=root/'data/import-manifest.json';manifest=json.loads(manifest_path.read_text())
manifest['imports'].append(dict(source='rra-manufacturer-lists',path=path.relative_to(root/'data').as_posix()))
manifest['files'][path.relative_to(root/'data').as_posix()]=digest(path.read_bytes())
manifest['expected']['devices']=len(after_devices);manifest['expected']['records']=len(after_records)
manifest['expected']['sources']=dict(db.execute('SELECT id,records_seen FROM sources'))
manifest_path.write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
db.close();print(json.dumps(audit,indent=2))
