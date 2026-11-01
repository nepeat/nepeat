"""Add the reviewed Inventec/QCT batch, audit preservation, then update manifest."""
import json
import subprocess
import sys
from pathlib import Path
from devicehunt.catalog import Catalog, target_for
from devicehunt.pipeline import import_source
from devicehunt.rebuild import file_sha256, read_manifest
from devicehunt import validation, vendor_inventory

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'data'
OUT = DATA / 'exports'
LIVE = DATA / 'devices.sqlite'
BEFORE = 'ff38451b49d4d78f40aa4ee72e69d4cca3f0fa05b7fa0c7e94bfd1a0e059776f'
assert file_sha256(LIVE) == BEFORE
_, manifest = read_manifest(DATA / 'import-manifest.json')
candidate = DATA / 'inventec-qct-candidate-2026-10-04.sqlite'
bundle = DATA / 'raw/reference-documents-with-inventec-qct-2026-10-04.json'
facts = DATA / 'inventec-qct-facts-2026-10-04.json'
if '--review-existing' not in sys.argv:
    assert not candidate.exists()
    subprocess.run(['/bin/cp', '-cp', str(LIVE), str(candidate)], check=True)
    c = Catalog(candidate)
    for source, path in [('reference-documents', bundle), ('inventec-server-index', DATA / 'raw/inventec-qct-2026-10-04/inventec-products-en.html'), ('curated', facts)]:
        print(source, import_source(c, source, path), flush=True)
    c.db.close()
c = Catalog(candidate, read_only=True)
db = c.db
db.execute('ATTACH DATABASE ? AS old', (LIVE.as_uri() + '?mode=ro',))
tables = json.loads((OUT / 'bytedance-inspur-rebuild-2026-10-04-comparison.json').read_text())
audit = {}
for table in tables:
    if table in ('sources', 'search_index', 'document_search'):
        continue
    cols = [r['name'] for r in db.execute('PRAGMA main.table_info(' + table + ')')
            if r['name'] not in ('observed_at', 'fetched_at', 'cache_path')]
    fields = ','.join('"' + col + '"' for col in cols)
    missing = db.execute(f'SELECT count(*) FROM (SELECT {fields} FROM old.{table} EXCEPT SELECT {fields} FROM main.{table})').fetchone()[0]
    assert missing == 0, (table, missing)
    audit[table] = dict(previous=db.execute('SELECT count(*) FROM old.' + table).fetchone()[0],
                        current=db.execute('SELECT count(*) FROM main.' + table).fetchone()[0], missing=missing)
for old in db.execute('SELECT * FROM old.sources'):
    new = db.execute('SELECT * FROM main.sources WHERE id=?', (old['id'],)).fetchone()
    assert new is not None
    for column in old.keys():
        if column in ('fetched_at', 'cache_path'):
            continue
        if column == 'sha256' and old['id'].startswith('reference-pdf-'):
            assert new[column] == file_sha256(bundle)
            continue
        assert old[column] == new[column], (old['id'], column)
assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
assert not db.execute('PRAGMA foreign_key_check').fetchall()
base = validation.run(db, ROOT / 'validation/owned-hardware.json')
examples = validation.run(db, ROOT / 'validation/vendor-examples.json')
assert base['summary'] == dict(total=10, hits=10, gaps=0)
assert base['evidence_summary']['document_snapshot_hits'] == 9
assert examples['summary'] == dict(total=2, hits=0, gaps=2)
assert target_for('QCT') is None and target_for('Quanta Cloud Technology') is None
new_sids = tuple(source['id'] for source in json.loads(facts.read_text())['sources']) + ('inventec-server-index-en',)
marks = ','.join('?' for _ in new_sids)
assert db.execute(f'SELECT count(*) FROM records WHERE source_id IN ({marks})', new_sids).fetchone()[0] == 48
for table in ('identifiers', 'hyperscaler_evidence'):
    assert db.execute(f'SELECT count(*) FROM {table} WHERE record_id IN (SELECT id FROM records WHERE source_id IN ({marks}))', new_sids).fetchone()[0] == 0
affiliations = [dict(row) for row in db.execute(f'SELECT * FROM affiliations WHERE record_id IN (SELECT id FROM records WHERE source_id IN ({marks}))', new_sids)]
assert {r['role'] for r in affiliations} == {'brand', 'platform_vendor', 'historical_subsidiary_portfolio'}
assert sum(r['target'] == 'Quanta' for r in affiliations) == 9
assert sum(r['target'] == 'Inventec' for r in affiliations) == 60
assert all(r['role'] == 'historical_subsidiary_portfolio' for r in affiliations if r['target'] == 'Quanta')
assert db.execute("SELECT count(*) FROM facts f JOIN records r ON r.id=f.record_id WHERE r.source_id='inventec-server-index-en' AND f.name='detail_access_status' AND f.value='login required; not fetched'").fetchone()[0] == 4
assert db.execute("SELECT count(*) FROM documents WHERE source_id IN ('reference-pdf-quanta-affiliates-2025','reference-pdf-qct-wssd','reference-pdf-qct-qxvdi-2020')").fetchone()[0] == 3
inventories = {scope: vendor_inventory.run(db, scope=scope, limit=-1) for scope in ('manufacturer', 'factory', 'all')}
previous = json.loads((OUT / 'latest-export-audit.json').read_text())['inventories']
for scope in ('manufacturer', 'factory'):
    for key in ('identity_target_pairs', 'unique_identities', 'counts_by_target'):
        assert inventories[scope][key] == previous[scope][key], (scope, key)
counts = {key: db.execute('SELECT count(*) FROM ' + ('fcc_grantees' if key == 'grantees' else key)).fetchone()[0]
          for key in manifest['expected'] if key != 'sources'}
counts['sources'] = dict(db.execute('SELECT id,records_seen FROM sources'))
assert (counts['devices'], counts['records'], len(counts['sources']), counts['documents'], counts['document_pages']) == (3937, 5135, 246, 44, 3474)
report = dict(previous_sha256=BEFORE, baseline=base['summary'], evidence_summary=base['evidence_summary'],
    vendor_examples=examples['summary'], preserved_tables=audit, expected_counts=counts,
    added_identities=37, added_records=48, added_original_pdfs=3,
    inventories={scope: {key: value[key] for key in ('identity_target_pairs', 'unique_identities', 'counts_by_target')}
                 for scope, value in inventories.items()})
db.close()
report['database_sha256'] = file_sha256(candidate)
assert file_sha256(LIVE) == BEFORE
(OUT / 'inventec-qct-review-2026-10-04.json').write_text(json.dumps(report, indent=2) + '\n')
backup = DATA / 'import-manifest-before-inventec-qct-2026-10-04.json'
assert not backup.exists()
backup.write_bytes((DATA / 'import-manifest.json').read_bytes())
jobs = [job for job in manifest['imports'] if job['source'] == 'reference-documents']
assert len(jobs) == 1
jobs[0]['path'] = str(bundle.relative_to(DATA))
manifest['imports'].append(dict(source='inventec-server-index', path='raw/inventec-qct-2026-10-04/inventec-products-en.html'))
manifest['imports'].append(dict(source='curated', path=facts.name))
inputs = list((DATA / 'raw/inventec-qct-2026-10-04').iterdir()) + [bundle, facts]
for stem in ('quanta-affiliates-2025','qct-wssd','qct-qxvdi-2020'):
    inputs += list((DATA / 'raw').glob(stem + '-2026-10-04.*'))
for path in inputs:
    assert path.is_file()
    manifest['files'][str(path.relative_to(DATA))] = file_sha256(path)
manifest['expected'] = counts
(DATA / 'import-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
print(json.dumps(dict(counts={k: v for k, v in counts.items() if k in ('devices', 'records', 'documents', 'document_pages')},
    sources=len(counts['sources']), jobs=len(manifest['imports']), inputs=len(manifest['files']),
    baseline=base['summary'], examples=examples['summary']), indent=2))
