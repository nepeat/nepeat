"""Add the reviewed Ingrasys batch, audit preservation, then update manifest."""
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
BEFORE = 'edf5b96aabde30ea811de0089f45ea3dd7f8ab6062c6a728f988e2722d3310fe'
assert file_sha256(LIVE) == BEFORE
_, manifest = read_manifest(DATA / 'import-manifest.json')
candidate = DATA / 'ingrasys-candidate-2026-10-04.sqlite'
bundle = DATA / 'raw/reference-documents-with-ingrasys-2026-10-04.json'
facts = DATA / 'ingrasys-facts-2026-10-04.json'
if '--review-existing' not in sys.argv:
    assert not candidate.exists()
    subprocess.run(['/bin/cp', '-cp', str(LIVE), str(candidate)], check=True)
    c = Catalog(candidate)
    for source, path in [('reference-documents', bundle), ('curated', facts)]:
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
assert target_for('Ingrasys') is None and target_for('Foxconn Technology Group') is None
new_sids = tuple(source['id'] for source in json.loads(facts.read_text())['sources'])
marks = ','.join('?' for _ in new_sids)
assert db.execute(f'SELECT count(*) FROM records WHERE source_id IN ({marks})', new_sids).fetchone()[0] == 17
for table in ('identifiers', 'hyperscaler_evidence'):
    assert db.execute(f'SELECT count(*) FROM {table} WHERE record_id IN (SELECT id FROM records WHERE source_id IN ({marks}))', new_sids).fetchone()[0] == 0
affiliations = [dict(row) for row in db.execute(f'SELECT * FROM affiliations WHERE record_id IN (SELECT id FROM records WHERE source_id IN ({marks}))', new_sids)]
assert {r['role'] for r in affiliations} == {'brand', 'platform_vendor', 'parent_company_portfolio'}
assert sum(r['target'] == 'Foxconn' for r in affiliations) == 8
assert all(r['target'] is None or (r['company'] == 'Foxconn' and r['role'] == 'parent_company_portfolio') for r in affiliations)
assert db.execute("SELECT count(*) FROM devices WHERE brand='Ingrasys' AND model IN ('SV1120','SV1121','SV2121','SV2221','SV2221A','SV2221I','ES2000','ES2100')").fetchone()[0] == 8
doc = db.execute("SELECT * FROM documents WHERE source_id='reference-pdf-ingrasys-es2000-es2100'").fetchone()
assert doc['page_count'] == 2 and doc['pages_with_text'] == 0
inventories = {scope: vendor_inventory.run(db, scope=scope, limit=-1) for scope in ('manufacturer', 'factory', 'all')}
previous = json.loads((OUT / 'latest-export-audit.json').read_text())['inventories']
for scope in ('manufacturer', 'factory'):
    for key in ('identity_target_pairs', 'unique_identities', 'counts_by_target'):
        assert inventories[scope][key] == previous[scope][key], (scope, key)
counts = {key: db.execute('SELECT count(*) FROM ' + ('fcc_grantees' if key == 'grantees' else key)).fetchone()[0]
          for key in manifest['expected'] if key != 'sources'}
counts['sources'] = dict(db.execute('SELECT id,records_seen FROM sources'))
assert (counts['devices'], counts['records'], len(counts['sources']), counts['documents'], counts['document_pages']) == (3900, 5087, 238, 41, 3412)
report = dict(previous_sha256=BEFORE, baseline=base['summary'], evidence_summary=base['evidence_summary'],
    vendor_examples=examples['summary'], preserved_tables=audit, expected_counts=counts,
    added_identities=8, added_records=17, added_pdf_derivatives=1,
    inventories={scope: {key: value[key] for key in ('identity_target_pairs', 'unique_identities', 'counts_by_target')}
                 for scope, value in inventories.items()})
db.close()
report['database_sha256'] = file_sha256(candidate)
assert file_sha256(LIVE) == BEFORE
(OUT / 'ingrasys-review-2026-10-04.json').write_text(json.dumps(report, indent=2) + '\n')
backup = DATA / 'import-manifest-before-ingrasys-2026-10-04.json'
assert not backup.exists()
backup.write_bytes((DATA / 'import-manifest.json').read_bytes())
jobs = [job for job in manifest['imports'] if job['source'] == 'reference-documents']
assert len(jobs) == 1
jobs[0]['path'] = str(bundle.relative_to(DATA))
manifest['imports'].append(dict(source='curated', path=facts.name))
inputs = list((DATA / 'raw/ingrasys-servers-2026-10-04').iterdir()) + [bundle, facts]
inputs += list((DATA / 'raw').glob('ingrasys-es2000-es2100-bom-removed-2026-10-04.*'))
for path in inputs:
    assert path.is_file()
    manifest['files'][str(path.relative_to(DATA))] = file_sha256(path)
manifest['expected'] = counts
(DATA / 'import-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
print(json.dumps(dict(counts={k: v for k, v in counts.items() if k in ('devices', 'records', 'documents', 'document_pages')},
    sources=len(counts['sources']), jobs=len(manifest['imports']), inputs=len(manifest['files']),
    baseline=base['summary'], examples=examples['summary']), indent=2))
