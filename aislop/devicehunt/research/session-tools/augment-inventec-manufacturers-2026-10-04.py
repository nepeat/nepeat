"""Audit the late primary manufacturer evidence before the batch's clean rebuild."""
import json
import subprocess
from pathlib import Path
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt.rebuild import file_sha256, read_manifest
from devicehunt import validation, vendor_inventory
ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'data'
OUT = DATA / 'exports'
live = DATA / 'devices.sqlite'
candidate = DATA / 'inventec-qct-candidate-2026-10-04.sqlite'
review = json.loads((OUT / 'inventec-qct-review-2026-10-04.json').read_text())
assert file_sha256(live) == review['previous_sha256']
assert file_sha256(candidate) == review['database_sha256']
_, manifest = read_manifest(DATA / 'import-manifest.json')
prior = DATA / 'inventec-qct-before-manufacturer-review-2026-10-04.sqlite'
assert not prior.exists()
subprocess.run(['/bin/cp', '-cp', str(candidate), str(prior)], check=True)
facts = DATA / 'inventec-manufacturer-facts-2026-10-04.json'
c = Catalog(candidate)
assert import_source(c, 'curated', facts) == 5
c.db.close()
c = Catalog(candidate, read_only=True)
db = c.db
db.execute('ATTACH DATABASE ? AS old', (prior.as_uri() + '?mode=ro',))
audit = {}
tables = json.loads((OUT / 'ingrasys-rebuild-2026-10-04-comparison.json').read_text())
for table in tables:
    if table in ('search_index', 'document_search'):
        continue
    cols = [r['name'] for r in db.execute('PRAGMA main.table_info(' + table + ')')
            if r['name'] not in ('observed_at', 'fetched_at', 'cache_path')]
    fields = ','.join('"' + col + '"' for col in cols)
    missing = db.execute(f'SELECT count(*) FROM (SELECT {fields} FROM old.{table} EXCEPT SELECT {fields} FROM main.{table})').fetchone()[0]
    assert missing == 0, (table, missing)
    audit[table] = dict(previous=db.execute('SELECT count(*) FROM old.' + table).fetchone()[0],
                        current=db.execute('SELECT count(*) FROM main.' + table).fetchone()[0], missing=missing)
assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
assert not db.execute('PRAGMA foreign_key_check').fetchall()
base = validation.run(db, ROOT / 'validation/owned-hardware.json')
examples = validation.run(db, ROOT / 'validation/vendor-examples.json')
assert base['summary'] == dict(total=10, hits=10, gaps=0)
assert base['evidence_summary']['document_snapshot_hits'] == 9
assert examples['summary'] == dict(total=2, hits=0, gaps=2)
sid = 'inventec-dongguan-manufacturer-2020'
models = [r[0] for r in db.execute("SELECT d.model FROM devices d JOIN records r ON r.device_id=d.id JOIN affiliations a ON a.record_id=r.id WHERE r.source_id=? AND a.target='Inventec' AND a.role='manufacturer' ORDER BY d.model", (sid,))]
assert set(models) == {'Entei', 'Horsea', 'Golduck', 'E850G4', 'FPGA SmartNIC C5020X'}
assert db.execute('SELECT count(*) FROM records WHERE source_id=?', (sid,)).fetchone()[0] == 5
assert not db.execute('SELECT 1 FROM hyperscaler_evidence WHERE record_id IN (SELECT id FROM records WHERE source_id=?)', (sid,)).fetchone()
inventories = {scope: vendor_inventory.run(db, scope=scope, limit=-1) for scope in ('manufacturer', 'factory', 'all')}
for row in review['inventories']['manufacturer']['counts_by_target']:
    new = next(r for r in inventories['manufacturer']['counts_by_target'] if r['target'] == row['target'])
    assert new['identities'] == row['identities'] + (5 if row['target'] == 'Inventec' else 0)
assert inventories['factory']['counts_by_target'] == review['inventories']['factory']['counts_by_target']
counts = {key: db.execute('SELECT count(*) FROM ' + ('fcc_grantees' if key == 'grantees' else key)).fetchone()[0]
          for key in manifest['expected'] if key != 'sources'}
counts['sources'] = dict(db.execute('SELECT id,records_seen FROM sources'))
assert (counts['devices'], counts['records'], len(counts['sources']), counts['documents'], counts['document_pages']) == (3941, 5140, 247, 44, 3474)
db.close()
final = dict(review, database_sha256=file_sha256(candidate), expected_counts=counts,
    baseline=base['summary'], evidence_summary=base['evidence_summary'], vendor_examples=examples['summary'],
    staged_preservation_chain=dict(previous_review='inventec-qct-review-2026-10-04.json',
        staged_database_sha256=file_sha256(prior), additional_preservation=audit),
    added_identities=41, added_records=53, added_original_pdfs=3, added_manufacturer_models=models,
    inventories={scope: {key: value[key] for key in ('identity_target_pairs', 'unique_identities', 'counts_by_target')}
                 for scope, value in inventories.items()})
(OUT / 'inventec-qct-final-review-2026-10-04.json').write_text(json.dumps(final, indent=2) + '\n')
assert file_sha256(live) == review['previous_sha256']
checkpoint = DATA / 'import-manifest-inventec-qct-index-stage-2026-10-04.json'
assert not checkpoint.exists()
checkpoint.write_bytes((DATA / 'import-manifest.json').read_bytes())
manifest['imports'].append(dict(source='curated', path=facts.name))
for path in (facts, DATA / 'raw/inventec-dongguan-2020-2026-10-04.html', DATA / 'raw/inventec-dongguan-2020-2026-10-04.fetch.json'):
    manifest['files'][str(path.relative_to(DATA))] = file_sha256(path)
manifest['expected'] = counts
(DATA / 'import-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
print(json.dumps(dict(devices=counts['devices'], records=counts['records'], sources=len(counts['sources']),
    jobs=len(manifest['imports']), files=len(manifest['files']), new_manufacturer_models=models), indent=2))
