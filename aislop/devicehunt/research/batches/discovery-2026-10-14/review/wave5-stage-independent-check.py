"""2026-10-14 read-only audit of the already staged wave5 batch. No imports."""
from pathlib import Path
from collections import Counter
from contextlib import closing
import datetime
import hashlib
import json
import sqlite3
import sys

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT))
from devicehunt import rebuild, reference_documents, validation, review_queue
from devicehunt.importers import prepare_curated

OUT = Path(__file__).resolve().parent
BATCH = ROOT / 'research/batches/discovery-2026-10-12'
INTEGRATION = BATCH / 'integration-wave5'
DATA = ROOT / 'data'

def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1 << 20), b''):
            h.update(block)
    return h.hexdigest()

def load(path):
    return json.loads(path.read_text())

def ro(path):
    c = sqlite3.connect(path.as_uri() + '?mode=ro', uri=True)
    c.row_factory = sqlite3.Row
    c.execute('PRAGMA query_only=ON')
    return c

report_path = INTEGRATION / 'stage-report.json'
report = load(report_path)
dbpath = DATA / 'devices-candidate-wave5-2026-10-12.sqlite'
mpath = DATA / 'import-manifest-candidate-wave5-2026-10-12.json'
held = load(INTEGRATION / 'code-under-test.json')['files']
assert held == load(BATCH / 'integration-wave3/code-under-test.json')['files']
assert len(held) == 209
hashes = {str(p.relative_to(ROOT)): sha(p) for p in [
    DATA / 'devices.sqlite', DATA / 'import-manifest.json', dbpath, mpath,
    report_path, *[INTEGRATION / p for p in (
        'stage.py', 'promote.py', 'refresh_exports.py', 'check_queries.py',
        'check_inventory.py', 'code-under-test.json')],
    *[BATCH / 'review' / p for p in (
        'original-vendor-wave5-facts-review.md', 'qct-compatibility-facts-review.md',
        'wave5-stage-helper-review.md')]]}
assert hashes[str(dbpath.relative_to(ROOT))] == report['candidate_database_sha256']
assert hashes[str(mpath.relative_to(ROOT))] == report['candidate_manifest_sha256']
assert {p: sha(ROOT / p) for p in held} == held == report['accepted_code_hashes']
assert {n: sha(DATA / n) for n in report['live_unchanged']} == report['live_unchanged']
assert sha(INTEGRATION / 'stage.py') == 'ec9ebc10d4f7bebab2875467ff7fdc70ce9fbb94ca02d3f56c6f09734007418e'
manifest = load(mpath)
old_manifest = load(DATA / 'import-manifest.json')
# read_manifest strictly verifies every declared pin and nested consumed inputs.
rebuild.read_manifest(mpath)
assert len(manifest['files']) == 3984 and len(manifest['imports']) == 152
assert all(manifest['files'][k] == v for k, v in old_manifest['files'].items())
new_pins = set(manifest['files']) - set(old_manifest['files'])
assert len(new_pins) == 14 and new_pins == set(report['new_input_hashes'])
assert {k: sha(DATA / k) for k in new_pins} == report['new_input_hashes']
expected_jobs = list(old_manifest['imports'])
expected_jobs = [dict(j, path='raw/reference-documents-with-wave5-2026-10-12.json')
                 if j['source'] == 'reference-documents' else j for j in expected_jobs]
expected_jobs += [dict(source='curated', path=f'raw/{g}-wave5-2026-10-12/candidate.json')
                 for g in ('original-vendor-wave5-facts', 'qct-compatibility-facts')]
assert manifest['imports'] == expected_jobs
old_bundle_path = DATA / 'raw/reference-documents-with-wave4-2026-10-12.json'
bundle_path = DATA / 'raw/reference-documents-with-wave5-2026-10-12.json'
old_bundle, bundle = load(old_bundle_path), load(bundle_path)
assert len(old_bundle['documents']) == 59 and len(bundle['documents']) == 61
assert bundle['documents'][:59] == old_bundle['documents']
_, prepared = reference_documents.prepare(bundle_path)
assert len(prepared) == 61 and sum(len(p) for _, p in prepared) == 3730

preservation = {}
with closing(ro(dbpath)) as c:
    c.execute('ATTACH DATABASE ? AS old', ((DATA / 'devices.sqlite').as_uri() + '?mode=ro',))
    tables = [r[0] for r in c.execute("SELECT name FROM old.sqlite_master WHERE type='table'")
              if not r[0].startswith(('sqlite_', 'search_index', 'document_search'))]
    assert len(tables) == 50
    for table in tables:
        cols = [r[1] for r in c.execute(f'PRAGMA main.table_info("{table}")')
                if r[1] not in ('observed_at', 'fetched_at', 'cache_path')]
        if table == 'sources':
            cols.remove('sha256')
        fields = ','.join('"' + p + '"' for p in cols)
        # Compare duplicate-sensitive logical rows independently of the stage helper.
        old_rows = Counter(tuple(r) for r in c.execute(f'SELECT {fields} FROM old."{table}"'))
        new_rows = Counter(tuple(r) for r in c.execute(f'SELECT {fields} FROM main."{table}"'))
        missing = old_rows - new_rows
        assert not missing, (table, len(missing))
        preservation[table] = {'old_rows': sum(old_rows.values()), 'missing_rows': sum(missing.values())}
    source_changes = [dict(r) for r in c.execute('''SELECT o.id,o.sha256 old_sha256,n.sha256 new_sha256
        FROM old.sources o JOIN main.sources n USING(id) WHERE o.sha256 IS NOT n.sha256 ORDER BY o.id''')]
    assert source_changes == report['expected_complete_bundle_source_hash_changes']
    assert len(source_changes) == 59
    assert {r['id'] for r in source_changes} == {'reference-pdf-' + e['id'] for e in old_bundle['documents']}
    assert all(r['old_sha256'] == sha(old_bundle_path) and r['new_sha256'] == sha(bundle_path) for r in source_changes)
    counts = {n: c.execute('SELECT count(*) FROM ' + n).fetchone()[0] for n in report['counts']}
    assert counts == report['counts']
    assert [r[0] for r in c.execute('PRAGMA integrity_check')] == ['ok']
    assert not c.execute('PRAGMA foreign_key_check').fetchall()
    assert validation.run(c, ROOT / 'validation/owned-hardware.json')['summary'] == {'total': 10, 'hits': 10, 'gaps': 0}
    release = review_queue.gate_inputs(DATA, manifest)
    review_queue.gate_projections(c, release)
    imported = []
    roles = Counter()
    for group, digest in [('original-vendor-wave5-facts', '687da4d58027a507cce2f2f4d31225548d3a57fd73ec6a12be5605c4fe5de86b'),
                          ('qct-compatibility-facts', '9f2f76cfb228e18298bd24c5c33d87b588a17e556b424a5ef41a8ee95aacebfd')]:
        original = BATCH / group / 'candidate.json'
        dest = DATA / 'raw' / (group + '-wave5-2026-10-12') / 'candidate.json'
        assert sha(original) == sha(dest) == digest
        payload = load(dest)
        assert len(prepare_curated(dest)) == len(payload['sources'])
        for source in payload['sources']:
            for field in ('snapshot', 'text_sidecar'):
                if field in source:
                    assert sha(original.parent / source[field]) == sha(dest.parent / source[field])
            for dep in source.get('evidence_dependencies', []):
                p = dest.parent / dep['path']
                assert sha(p) == sha(original.parent / dep['path']) == dep['sha256']
                assert manifest['files'][str(p.relative_to(DATA))] == dep['sha256']
            assert not c.execute('SELECT 1 FROM old.sources WHERE id=?', (source['id'],)).fetchone()
        for d in payload['devices']:
            rows = c.execute('''SELECT r.*,v.brand,v.model FROM records r JOIN devices v ON v.id=r.device_id
                WHERE source_id=? AND source_key=?''', (d['source'], d['key'])).fetchall()
            assert len(rows) == 1
            row = rows[0]
            assert row['brand'] == d['brand'] and row['model'] == d['model']
            assert json.loads(row['raw_json']) == d
            for table, cols, key in [('facts', 'name,value', 'facts'), ('affiliations', 'company,role', 'affiliations'),
                                     ('identifiers', 'kind,value', 'identifiers')]:
                actual = Counter(tuple(r) for r in c.execute(f'SELECT {cols} FROM {table} WHERE record_id=?', (row['id'],)))
                assert actual == Counter(tuple(v) for v in d.get(key, []))
            assert not c.execute('SELECT 1 FROM hyperscaler_evidence WHERE record_id=?', (row['id'],)).fetchone()
            roles.update(role for _, role in d['affiliations'])
            imported.append({'source': d['source'], 'key': d['key'], 'device_id': row['device_id'], 'model': row['model'], 'facts': len(d['facts'])})
    assert len(imported) == 41 and sum(r['facts'] for r in imported) == 235
    assert roles == {'documentation_vendor': 41}
    assert c.execute('SELECT count(*) FROM devices WHERE id NOT IN (SELECT id FROM old.devices)').fetchone()[0] == 35
    assert dict(c.execute('SELECT id,records_seen FROM sources')) == manifest['expected']['sources']

assert hashes == {p: sha(ROOT / p) for p in hashes}
assert held == {p: sha(ROOT / p) for p in held}
result = dict(status='PASS independent actual-stage checks; rebuild and final acceptance pending',
              checked_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
              hashes=hashes, code_files_verified=len(held), manifest_jobs=152, manifest_pins=3984,
              new_pins=sorted(new_pins), complete_bundle_entries=61, complete_bundle_pages=3730,
              counts=counts, preservation=preservation, source_hash_changes=source_changes,
              imported_records=imported, roles=dict(roles), integrity='ok', foreign_key_violations=0,
              owned_baseline={'total': 10, 'hits': 10, 'gaps': 0}, campaign_gates='inputs and projections pass',
              unchanged_read_only_inputs=True,
              limits='No network, imports, shared writes, rebuild, query/inventory helper execution, promotion or export execution.')
with (OUT / 'wave5-stage-independent-check.json').open('x') as f:
    json.dump(result, f, indent=2, ensure_ascii=False)
    f.write('\n')
print(result['status'])
