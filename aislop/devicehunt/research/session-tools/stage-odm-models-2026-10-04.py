"""One-time additive import and preservation audit before clean rebuild."""
import json
from pathlib import Path
import subprocess
import sys
from devicehunt.catalog import Catalog
from devicehunt.pipeline import import_source
from devicehunt.rebuild import file_sha256, read_manifest
from devicehunt import validation, specifications, vendor_inventory

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'data'
LIVE = DATA / 'devices.sqlite'
BEFORE = 'b241e62c0d4b8ea3edd8d100a4fe50b85df26d3b7b68f8a4f7571a052a7bcc47'
assert file_sha256(LIVE) == BEFORE
_, manifest = read_manifest(DATA / 'import-manifest.json')
candidate = DATA / 'odm-model-candidate-2026-10-04.sqlite'
bundle = DATA / 'raw/reference-documents-with-odm-models-2026-10-04.json'
if '--review-existing' not in sys.argv:
    assert not candidate.exists()
    subprocess.run(['/bin/cp', '-cp', str(LIVE), str(candidate)], check=True)
    c = Catalog(candidate)
    for source, path in [('reference-documents', bundle), ('curated', DATA / 'odm-model-facts-2026-10-04.json')]:
        print(source, import_source(c, source, path), flush=True)
    c.db.close()
# URI support is required for the read-only attached database, too.
c = Catalog(candidate, read_only=True)
c.db.execute('ATTACH DATABASE ? AS old', (LIVE.as_uri() + '?mode=ro',))
tables = json.loads((DATA / 'exports/odm-verified-rebuild-2026-10-04-comparison.json').read_text())
audit = {}
for table in tables:
    if table in ('sources', 'search_index', 'document_search'):
        continue
    cols = [r['name'] for r in c.db.execute('PRAGMA main.table_info(' + table + ')')
            if r['name'] not in ('observed_at', 'fetched_at', 'cache_path')]
    fields = ','.join('"' + col + '"' for col in cols)
    missing = c.db.execute(f'SELECT count(*) FROM (SELECT {fields} FROM old.{table} EXCEPT SELECT {fields} FROM main.{table})').fetchone()[0]
    assert missing == 0, (table, missing)
    audit[table] = dict(previous=c.db.execute('SELECT count(*) FROM old.' + table).fetchone()[0],
                        current=c.db.execute('SELECT count(*) FROM main.' + table).fetchone()[0], missing=missing)
for old in c.db.execute('SELECT * FROM old.sources'):
    new = c.db.execute('SELECT * FROM main.sources WHERE id=?', (old['id'],)).fetchone()
    assert new is not None
    for column in old.keys():
        if column in ('fetched_at', 'cache_path'):
            continue
        if column == 'sha256' and old['id'].startswith('reference-pdf-'):
            assert new[column] == file_sha256(bundle)
            continue
        assert old[column] == new[column], (old['id'], column)
assert c.db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
assert not c.db.execute('PRAGMA foreign_key_check').fetchall()
baseline = validation.run(c.db, ROOT / 'validation/owned-hardware.json')
assert baseline['summary'] == dict(total=10, hits=10, gaps=0)
assert baseline['evidence_summary']['document_snapshot_hits'] == 9
source_ids = ('wiwynn-sv7100g4-datasheet', 'hyve-jade-datasheet-2026')
assert c.db.execute('SELECT count(*) FROM records WHERE source_id IN (?,?)', source_ids).fetchone()[0] == 3
assert c.db.execute('SELECT count(*) FROM identifiers WHERE record_id IN (SELECT id FROM records WHERE source_id IN (?,?))', source_ids).fetchone()[0] == 0
assert c.db.execute('SELECT count(*) FROM hyperscaler_evidence WHERE record_id IN (SELECT id FROM records WHERE source_id IN (?,?))', source_ids).fetchone()[0] == 0
assert not vendor_inventory.run(c.db, 'TD SYNNEX', scope='manufacturer', limit=-1)['rows']
specs = {model: specifications.search(c, model, exact_model=True, limit=-1)
         for model in ('SV7100G4', 'SV7100G4-C', 'Jade')}
assert all(specs.values())
assert all('blade-table' in row['statement'].lower() for row in specs['SV7100G4-C'] if row['category'] == 'power_description')
inventory = {scope: vendor_inventory.run(c.db, scope=scope, limit=-1)
             for scope in ('manufacturer', 'factory', 'all')}
counts = {key: c.db.execute('SELECT count(*) FROM ' + ('fcc_grantees' if key == 'grantees' else key)).fetchone()[0]
          for key in manifest['expected'] if key != 'sources'}
counts['sources'] = dict(c.db.execute('SELECT id,records_seen FROM sources'))
assert (counts['devices'], counts['records'], len(counts['sources']), counts['documents'], counts['document_pages']) == (3839, 5010, 227, 38, 3374)
report = dict(previous_sha256=BEFORE, baseline=baseline['summary'], evidence_summary=baseline['evidence_summary'],
              preserved_tables=audit, specs=specs, expected_counts=counts,
              inventories={scope: {key: value[key] for key in ('identity_target_pairs', 'unique_identities', 'counts_by_target')}
                           for scope, value in inventory.items()})
c.db.close()
report['database_sha256'] = file_sha256(candidate)
assert file_sha256(LIVE) == BEFORE
(DATA / 'exports/odm-model-review-2026-10-04.json').write_text(json.dumps(report, indent=2) + '\n')
backup = DATA / 'import-manifest-before-odm-models-2026-10-04.json'
assert not backup.exists()
backup.write_bytes((DATA / 'import-manifest.json').read_bytes())
jobs = [job for job in manifest['imports'] if job['source'] == 'reference-documents']
assert len(jobs) == 1
jobs[0]['path'] = str(bundle.relative_to(DATA))
manifest['imports'].append(dict(source='curated', path='odm-model-facts-2026-10-04.json'))
inputs = list((DATA / 'raw/odm-models-2026-10-04').iterdir()) + list((DATA / 'raw').glob('odm-*-2026-10-04.*'))
inputs += [bundle, DATA / 'odm-model-facts-2026-10-04.json']
for path in inputs:
    assert path.is_file()
    manifest['files'][str(path.relative_to(DATA))] = file_sha256(path)
manifest['expected'] = counts
(DATA / 'import-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
print(json.dumps(dict(counts={k:v for k,v in counts.items() if k in ('devices','records','documents','document_pages')}, sources=len(counts['sources']), jobs=len(manifest['imports']), inputs=len(manifest['files']), baseline=report['baseline']), indent=2))
