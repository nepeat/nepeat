"""2026-10-14 independent completed-rebuild proof audit. All databases read-only."""
from pathlib import Path
from collections import Counter
from contextlib import closing
import ast
import datetime
import hashlib
import json
import sqlite3
import sys

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT))
from devicehunt import rebuild, validation, review_queue, vendor_inventory, lineage

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
    db = sqlite3.connect(path.as_uri() + '?mode=ro', uri=True)
    db.row_factory = sqlite3.Row
    db.execute('PRAGMA query_only=ON')
    return db

stage = load(INTEGRATION / 'stage-report.json')
initial = load(OUT / 'wave5-stage-independent-check.json')
assert initial['status'].startswith('PASS')
candidate = DATA / 'devices-candidate-wave5-2026-10-12.sqlite'
rebuilt = DATA / 'rebuilt-wave5-2026-10-12.sqlite'
manifest_path = DATA / 'import-manifest-candidate-wave5-2026-10-12.json'
comp_path = DATA / 'exports/wave5-2026-10-12-comparison.json'
base_path = DATA / 'exports/wave5-2026-10-12-validation.json'
suite_path = ROOT / 'research/batches/campaign-c2-2026-10-05/acceptance/discovery-2026-10-12-curated-wave3-suite-result.json'
paths = [candidate, rebuilt, manifest_path, DATA / 'devices.sqlite', DATA / 'import-manifest.json',
         comp_path, base_path, suite_path, ROOT / 'scripts/compare_rebuild.py',
         BATCH / 'review/wave5-stage-review.md', OUT / 'wave5-stage-independent-check.py',
         OUT / 'wave5-stage-independent-check.json', OUT / 'wave5-initial-review.md',
         Path(__file__).resolve(),
         *[INTEGRATION / name for name in ('stage-report.json', 'stage.py', 'promote.py',
             'refresh_exports.py', 'code-under-test.json', 'check_queries.py', 'check_inventory.py',
             'model-query-check.json', 'inventory-check.json', 'rebuild.log', 'compare.log')]]
hashes = {str(p.relative_to(ROOT)): sha(p) for p in paths}
assert sha(candidate) == stage['candidate_database_sha256']
assert sha(manifest_path) == stage['candidate_manifest_sha256']
assert all(sha(ROOT / p) == digest for p, digest in initial['hashes'].items())
held = load(INTEGRATION / 'code-under-test.json')['files']
assert held == stage['accepted_code_hashes'] and len(held) == 209
assert {p: sha(ROOT / p) for p in held} == held
assert {n: sha(DATA / n) for n in stage['live_unchanged']} == stage['live_unchanged']
suite = load(suite_path)
assert suite['tests'] == 791 and suite['passed'] is True
assert all(suite[k] == 0 for k in ('failures', 'errors', 'skipped', 'blocked_network_or_external_process_calls'))
manifest = load(manifest_path)
rebuild.read_manifest(manifest_path)
assert len(manifest['imports']) == 152 and len(manifest['files']) == 3984
release = review_queue.gate_inputs(DATA, manifest)
comp = load(comp_path)
baseline_proof = load(base_path)
assert Path(baseline_proof['database']).resolve() == rebuilt.resolve()
assert Path(baseline_proof['reference_database']).resolve() == candidate.resolve()
assert baseline_proof['matches_reference_baseline'] is True
assert baseline_proof['summary'] == {'total': 10, 'hits': 10, 'gaps': 0}
compare_log = (INTEGRATION / 'compare.log').read_text()
assert 'All 52 tables/indexes and baseline results match.' in compare_log
last_rebuild_line = json.loads((INTEGRATION / 'rebuild.log').read_text().splitlines()[-1])
assert Path(last_rebuild_line['database']).resolve() == rebuilt.resolve()
assert last_rebuild_line['verified_files'] == 3984
for key in ('devices', 'records', 'sources', 'documents', 'document_pages'):
    assert last_rebuild_line[key] == stage['counts'][key]

# Inspect the comparator itself: its SQL explicitly groups projected fields with
# count(*) and includes rowid for both FTS tables. Then independently use Counters.
comparator = (ROOT / 'scripts/compare_rebuild.py').read_text()
ast.parse(comparator)
assert "cols.insert(0,'rowid')" in comparator and 'count(*) AS multiplicity' in comparator
assert 'GROUP BY {fields}' in comparator and 'EXCEPT' in comparator
logical_tables = {}
with closing(ro(candidate)) as a, closing(ro(rebuilt)) as b, closing(ro(DATA / 'devices.sqlite')) as old:
    def tables(db):
        return {r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table'")
                if not r[0].startswith(('sqlite_', 'search_index_', 'document_search_'))}
    actual_tables = tables(a)
    assert tables(b) == actual_tables == set(comp) and len(actual_tables) == 52
    assert {'search_index', 'document_search'} <= actual_tables
    assert a.execute('PRAGMA user_version').fetchone()[0] == b.execute('PRAGMA user_version').fetchone()[0]
    for name in sorted(actual_tables):
        a_columns = [tuple(r) for r in a.execute(f'PRAGMA table_info("{name}")')]
        assert a_columns == [tuple(r) for r in b.execute(f'PRAGMA table_info("{name}")')]
        cols = [r[1] for r in a_columns if r[1] not in ('observed_at', 'fetched_at', 'cache_path')]
        if name in ('search_index', 'document_search'):
            cols.insert(0, 'rowid')
        fields = ','.join('"' + x + '"' for x in cols)
        left = Counter(tuple(r) for r in a.execute(f'SELECT {fields} FROM "{name}"'))
        right = Counter(tuple(r) for r in b.execute(f'SELECT {fields} FROM "{name}"'))
        assert left == right, name
        count = sum(left.values())
        assert comp[name] == {'original': count, 'rebuilt': count, 'only_original': 0, 'only_rebuilt': 0}
        logical_tables[name] = {'rows': count, 'multiplicities_equal': True,
                               'rowid_compared': name in ('search_index', 'document_search')}
    integrity = {label: [r[0] for r in db.execute('PRAGMA integrity_check')]
                 for label, db in [('candidate', a), ('rebuilt', b)]}
    assert all(rows == ['ok'] for rows in integrity.values())
    fk = {label: len(db.execute('PRAGMA foreign_key_check').fetchall())
          for label, db in [('candidate', a), ('rebuilt', b)]}
    assert fk == {'candidate': 0, 'rebuilt': 0}
    left_base = validation.run(a, ROOT / 'validation/owned-hardware.json')
    right_base = validation.run(b, ROOT / 'validation/owned-hardware.json')
    for key in ('summary', 'results', 'search_summary'):
        assert left_base[key] == right_base[key]
    assert left_base['summary'] == baseline_proof['summary']
    assert validation.run(b, ROOT / 'validation/vendor-examples.json')['summary'] == {'total': 2, 'hits': 0, 'gaps': 2}
    review_queue.gate_projections(b, release)
    assert dict(b.execute('SELECT id,records_seen FROM sources')) == manifest['expected']['sources']
    inventory = load(INTEGRATION / 'inventory-check.json')
    assert inventory['manufacturer_unchanged'] is True
    assert inventory['manufacturer'] == vendor_inventory.run(a, limit=0) == vendor_inventory.run(old, limit=0)
    assert inventory['all_target_associations'] == vendor_inventory.run(b, scope='all', limit=0)
    assert inventory['hyperscaler'] == {'rows': 22, 'identities': 18}
    assert inventory['vendor_examples'] == {'total': 2, 'hits': 0, 'gaps': 2}
    assert inventory['lineage_table_counts'] == {name: b.execute('SELECT count(*) FROM ' + name).fetchone()[0] for name in lineage.TABLES}
    assert inventory['database_hashes_unchanged'] == {str(p.relative_to(ROOT)): sha(p) for p in (candidate, DATA / 'devices.sqlite')}
    query = load(INTEGRATION / 'model-query-check.json')
    assert query['candidate_sha256_unchanged'] == sha(candidate)
    assert len(query['all_new_records_checked']) == 41 and sum(r['facts'] for r in query['all_new_records_checked']) == 235
    assert len(query['queries']) == 10

assert hashes == {p: sha(ROOT / p) for p in hashes}
assert held == {p: sha(ROOT / p) for p in held}
result = dict(status='PASS final independent pre-promotion integration audit',
              checked_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(), hashes=hashes,
              logical_tables=logical_tables, integrity=integrity, foreign_key_violations=fk,
              owned_baseline=left_base['summary'], owned_results_and_search_equal=True,
              manifest_jobs=152, manifest_pins=3984, frozen_code_files=209,
              accepted_suite=suite, suite_rerun=False, campaign_gate='inputs and rebuilt projections pass',
              counts=stage['counts'], query_records=41, query_facts=235, useful_model_queries=10,
              manufacturer_pairs=inventory['manufacturer']['identity_target_pairs'],
              all_target_pairs=inventory['all_target_associations']['identity_target_pairs'],
              all_target_identities=inventory['all_target_associations']['unique_identities'],
              unchanged_read_only_inputs=True,
              limits='No network, imports, shared writes, promotion/export execution or post-promotion acceptance.')
with (OUT / 'wave5-final-independent-check.json').open('x') as f:
    json.dump(result, f, indent=2, ensure_ascii=False)
    f.write('\n')
print(result['status'])
