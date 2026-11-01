import collections
import json
import sqlite3
from pathlib import Path

from devicehunt import rra
from devicehunt.catalog import digest
from devicehunt.rra import company_target as target_for
from devicehunt.__main__ import write_rows

root = Path.cwd()
out = root/'data/exports'
slugs = ['amazon-data-services', 'edgecore', 'accton', 'oracle']
manifest_path = root/'data/import-manifest.json'
manifest = json.loads(manifest_path.read_text())
db = sqlite3.connect('file:data/devices.sqlite?mode=ro', uri=True)
db.row_factory = sqlite3.Row
audits = []
for slug in slugs:
    path = root/('data/raw/rra-'+slug+'-official/bundle.json')
    data, ready = rra.validate(path)
    sid = 'rra-official-query-' + digest(json.dumps(data['query'], sort_keys=True).encode())[:12]
    imported = db.execute('SELECT count(*) FROM rra_equipment WHERE source_id=?', (sid,)).fetchone()[0]
    assert imported == len(ready), (sid, imported, len(ready))
    rows = [dict(row) for row in db.execute('SELECT * FROM rra_equipment WHERE source_id=? ORDER BY row_number', (sid,))]
    (out/('rra-'+slug+'-official.json')).write_text(json.dumps(rows, ensure_ascii=False, indent=2)+'\n')
    with (out/('rra-'+slug+'-official.csv')).open('w', newline='') as stream:
        write_rows(rows, 'csv', stream)
    counts = collections.Counter(row['manufacturer'] for row in rows if target_for(row['manufacturer']) is None)
    audits.append(dict(query=data['query'], source_id=sid, registry_rows=imported,
        matched_manufacturer_rows=sum(target_for(row['manufacturer']) is not None for row in rows),
        model_assertions=db.execute('SELECT count(*) FROM records WHERE source_id=?',(sid,)).fetchone()[0],
        unmapped_manufacturers=dict(counts.most_common()),
        csv_sha256=data['csv_before']['sha256'], bundle_sha256=digest(path.read_bytes())))
    job = dict(source='rra', path=str(path.relative_to(root/'data')))
    if job not in manifest['imports']:
        manifest['imports'].append(job)
    dependencies = [path] + [path.parent/data[key]['path'] for key in ('form','csv_before','csv_after')]
    dependencies += [path.parent/entry['path'] for entry in data['details']]
    for dependency in dependencies:
        manifest['files'][str(dependency.relative_to(root/'data'))] = digest(dependency.read_bytes())
for table in ('devices', 'records', 'rra_equipment'):
    manifest['expected'][table] = db.execute('SELECT count(*) FROM '+table).fetchone()[0]
manifest['expected']['sources'] = dict(db.execute('SELECT id,records_seen FROM sources'))
manifest_path.write_text(json.dumps(manifest, indent=2)+'\n')
db.execute("ATTACH DATABASE 'file:data/rebuilt-with-rra-official.sqlite?mode=ro' AS previous")
new = [dict(row) for row in db.execute('SELECT d.* FROM devices d LEFT JOIN previous.devices p ON p.id=d.id WHERE p.id IS NULL ORDER BY d.brand,d.model')]
(out/'rra-target-new-identities.json').write_text(json.dumps(new, ensure_ascii=False, indent=2)+'\n')
result = dict(queries=audits, new_identities=len(new),
    total_registry_rows=db.execute('SELECT count(*) FROM rra_equipment').fetchone()[0],
    distinct_certificates=db.execute('SELECT count(DISTINCT certificate_number) FROM rra_equipment').fetchone()[0],
    totals={table:db.execute('SELECT count(*) FROM '+table).fetchone()[0] for table in ('devices','records','sources')},
    manifest_jobs=len(manifest['imports']), manifest_files=len(manifest['files']))
(out/'rra-target-query-audit.json').write_text(json.dumps(result, ensure_ascii=False, indent=2)+'\n')
print(json.dumps(result, ensure_ascii=False, indent=2))
