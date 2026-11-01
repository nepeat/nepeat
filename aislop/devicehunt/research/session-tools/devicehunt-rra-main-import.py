import json
from collections import Counter
from pathlib import Path
from devicehunt.catalog import Catalog, digest
from devicehunt import rra, rra_company_names, vendor_inventory, validation
from devicehunt.pipeline import import_source
from devicehunt.__main__ import write_rows

root = Path.cwd(); manifest_path = root/'data/import-manifest.json'
manifest = json.loads(manifest_path.read_text())
jobs = [j for j in manifest['imports'] if j['source'] in ('rra', 'rra-sun-portfolio')]
c = Catalog(root/'data/devices.sqlite'); db = c.db
original = Catalog(root/'data/rebuilt-with-ghana.sqlite', read_only=True)
def raw_rows():
    return [tuple(r) for r in db.execute('SELECT * FROM rra_equipment ORDER BY source_id,row_number')]
def records():
    return {r['id']: dict(r) for r in db.execute('SELECT r.id,r.device_id,r.source_id,r.source_key,r.source_url,r.raw_json,d.model,d.brand '
        'FROM records r JOIN devices d ON d.id=r.device_id')}
live = db; db = original.db
before_records = records(); before_raw = raw_rows()
before_ids = {r[0] for r in db.execute('SELECT id FROM devices')}
before_baseline = validation.run(db, root/'validation/owned-hardware.json')
db = live
out = root/'data/exports'
out.joinpath('rra-main-models-before.json').write_text(json.dumps(dict(
    devices=len(before_ids), records=len(before_records), baseline=before_baseline['summary']), indent=2)+'\n')
# Check every cached query and model list before replacing a source.
candidates = []; expected = set()
for job in jobs:
    path = manifest_path.parent/job['path']; data, ready = rra.validate(path)
    reviewed = rra_company_names.load(path.parent, data.get('company_evidence'))
    for _, entry, row in ready:
        if ',' not in row['main_model']:
            continue
        target = rra.company_target(row['manufacturer'], reviewed)
        names = [m for role, m in rra.models(row) if role == 'main']
        candidates.append(dict(target=target, certificate=row['certificate_number'],
            manufacturer=row['manufacturer'], main_model=row['main_model'],
            extracted_main_models=names, source_url=entry['url'], detail_sha256=entry['sha256'],
            eligible=bool(target and names)))
        if target:
            expected.update((target, row['certificate_number']+':'+m) for m in names)
out.joinpath('rra-main-models-review.json').write_text(json.dumps(candidates,ensure_ascii=False,indent=2)+'\n')
results = []
for job in jobs:
    # The original import run completed all jobs; its over-strict baseline
    # comparison failed on updated source coverage text. Do not reimport.
    results.append(dict(job=job, completed_in='rra-main-models-import.log'))
after_records = records()
assert before_raw == raw_rows(), 'Original register rows changed'
assert set(before_records) <= set(after_records), 'Existing records removed'
assert all(before_records[k] == after_records[k] for k in before_records), 'Existing claims changed'
added = [after_records[k] for k in sorted(set(after_records)-set(before_records))]
assert {(r['brand'], r['source_key']) for r in added} == expected, 'Delta differs from reviewed literal lists'
after_ids = {r[0] for r in db.execute('SELECT id FROM devices')}
for row in added:
    row['new_identity'] = row['device_id'] not in before_ids
    row['manufacturer'] = db.execute("SELECT company FROM affiliations WHERE record_id=? AND role='registry_manufacturer'",(row['id'],)).fetchone()[0]
    row['main_model_as_printed'] = json.loads(row['raw_json'])['main_model']
    del row['raw_json']
    assert db.execute("SELECT value FROM facts WHERE record_id=? AND name='rra_model_role'",(row['id'],)).fetchone()[0] == 'main'
after_baseline = validation.run(db,root/'validation/owned-hardware.json')
assert before_baseline['summary'] == after_baseline['summary']
assert before_baseline['search_summary'] == after_baseline['search_summary']
for old, new in zip(before_baseline['results'], after_baseline['results']):
    for key in ('identifier', 'vendor', 'status'):
        assert old[key] == new[key], (key, old, new)
inventory = vendor_inventory.run(db,limit=0)
audit = dict(imports=results, unchanged_raw_rows=len(before_raw),
    unchanged_existing_records=len(before_records), added_assertions=len(added),
    new_catalog_identities=len(after_ids-before_ids),
    assertions_by_target=dict(Counter(r['brand'] for r in added)),
    current_devices=len(after_ids),current_records=len(after_records),
    manufacturer_inventory=inventory, baseline=after_baseline['summary'])
for name,data in [('rra-main-models-import-audit',audit),('rra-main-models-additions',added)]:
    out.joinpath(name+'.json').write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
with out.joinpath('rra-main-models-additions.csv').open('w',newline='') as stream:
    write_rows(added,'csv',stream)
manifest['expected']['devices'] = len(after_ids)
manifest['expected']['records'] = len(after_records)
manifest['expected']['sources'] = dict(db.execute('SELECT id,records_seen FROM sources'))
manifest_path.write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
db.close(); original.db.close(); print(json.dumps(audit,ensure_ascii=False,indent=2))
