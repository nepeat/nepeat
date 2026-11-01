"""Read-only SmartNIC/QCT candidate audit; writes only this batch's audit proof.

Old live -> candidate preservation (duplicate-sensitive), candidate -> clean
rebuild equality, FTS reconstruction, literal source rows, owned baseline,
ByteDance gaps, inventory/hyperscaler invariants and lookup equality probes.
Run from the project root with PYTHONPATH=.
"""
import collections
import hashlib
import json
import re
import sqlite3
from pathlib import Path

from devicehunt import lookup, validation, vendor_inventory
from devicehunt.lineage import TABLES as LINEAGE

ROOT = Path.cwd()
B = ROOT / 'research/batches/smartnic-qct-2026-10-04'
O = B / 'integration'
LIVE = ROOT / 'data/devices.sqlite'
CANDIDATE = ROOT / 'data/candidate-with-smartnic-qct-2026-10-05.sqlite'
REBUILT = ROOT / 'data/rebuilt-with-smartnic-qct-2026-10-05.sqlite'


def sha(path):
    h = hashlib.sha256()
    with open(path, 'rb') as stream:
        for block in iter(lambda: stream.read(1 << 20), b''):
            h.update(block)
    return h.hexdigest()


def connect(path):
    c = sqlite3.connect(Path(path).as_uri() + '?mode=ro', uri=True)
    c.row_factory = sqlite3.Row
    return c


def count(c, table, cols=None):
    fields = ','.join('"' + x + '"' for x in cols) if cols else '*'
    return collections.Counter(tuple(r) for r in c.execute('SELECT ' + fields + ' FROM ' + table))


stage = json.loads((O / 'stage-result.json').read_text())
assert sha(CANDIDATE) == stage['candidate_sha256'] and sha(LIVE) == stage['live_sha256']
script = (ROOT / 'scripts/compare_rebuild.py').read_text()
tables = re.search(r"tables='(.*?)'\.split\(\)", script)[1].split() + ['inspur_support_entries'] + list(LINEAGE)
old, new = connect(LIVE), connect(CANDIDATE)
reb = connect(REBUILT) if REBUILT.exists() else None
bundle = json.loads((O / 'old-complete-bundle.json').read_text())
allowed = {'reference-pdf-' + e['id'] for e in bundle['documents']}
assert len(allowed) == 47
preservation, comparison = {}, {}
for t in tables:
    cols = [r['name'] for r in old.execute('PRAGMA table_info(' + t + ')')]
    if t == 'sources':
        oldrows = {r['id']: dict(r) for r in old.execute('SELECT * FROM sources')}
        newrows = {r['id']: dict(r) for r in new.execute('SELECT * FROM sources')}
        changes = {}
        for sid, row in oldrows.items():
            changed = {k for k in row if row[k] != newrows[sid][k]}
            assert changed <= ({'sha256', 'cache_path', 'fetched_at'} if sid in allowed else set()), (sid, changed)
            if changed:
                changes[sid] = sorted(changed)
        preservation[t] = dict(old=len(oldrows), preserved=len(oldrows), new=len(newrows) - len(oldrows),
                               allowed_reference_metadata_changes=len(changes))
    elif t == 'search_index':
        oldkeys = {r[0] for r in old.execute('SELECT device_id FROM search_index')}
        newkeys = {r[0] for r in new.execute('SELECT device_id FROM search_index')}
        assert oldkeys <= newkeys
        preservation[t] = dict(old_device_keys_preserved=len(oldkeys), new_keys=len(newkeys - oldkeys))
    else:
        a, b = count(old, t), count(new, t)
        assert not a - b, (t, list((a - b).items())[:2])
        preservation[t] = dict(old=sum(a.values()), new=sum(b.values()), missing_old_multiplicity=0)
    if reb:
        logical = [x for x in cols if x not in ('observed_at', 'fetched_at', 'cache_path')]
        if t in ('search_index', 'document_search'):
            logical = ['rowid'] + logical
        a, b = count(new, t, logical), count(reb, t, logical)
        assert a == b, (t, sum((a - b).values()), sum((b - a).values()))
        comparison[t] = dict(candidate=sum(a.values()), rebuilt=sum(b.values()), equal_duplicate_sensitive=True)

fts = {}
for label, c in [('old', old), ('candidate', new)] + ([('rebuild', reb)] if reb else []):
    expected = []
    for d in c.execute('SELECT * FROM devices ORDER BY id').fetchall():
        parts = [d['title'], d['brand'], d['model'], d['kind'], d['revision']]
        for table, expr in [('affiliations', "company || ' ' || role"), ('identifiers', 'value'), ('facts', 'value'),
                            ('hyperscaler_evidence', "operator || ' ' || relationship || ' ' || detail"),
                            ('fcc_exhibits', "title || ' ' || document_type || ' ' || availability")]:
            parts.extend(r[0] for r in c.execute(f'SELECT {expr} FROM {table} x JOIN records r ON r.id=x.record_id '
                                                 'WHERE r.device_id=? ORDER BY r.id,1', (d['id'],)))
        text = ' '.join(parts)
        text += ' ' + ' '.join(re.findall(r'\d+[x×](\d+(?:\.\d+)?[kmg](?:be?)?)', text, re.I))
        expected.append((d['id'], text))
    assert collections.Counter(expected) == count(c, 'search_index'), label
    assert count(c, 'document_search') == count(c, 'document_pages', ['document_key', 'page_number', 'text']), label
    fts[label] = dict(device_search_reconstructs=True, document_search_reconstructs=True)

oldfts = dict(old.execute('SELECT device_id,text FROM search_index'))
newfts = dict(new.execute('SELECT device_id,text FROM search_index'))
changed = sorted(tuple(new.execute('SELECT brand,model FROM devices WHERE id=?', (k,)).fetchone())
                 for k, v in oldfts.items() if newfts[k] != v)
added = sorted(tuple(new.execute('SELECT brand,model FROM devices WHERE id=?', (k,)).fetchone())
               for k in set(newfts) - set(oldfts))
assert changed == [('Inventec', 'FPGA SmartNIC C5020X'), ('QCT', 'QuantaPlex T42S-2U')], changed
# The English IPU is its own printed identity, never an alias of the SmartNIC.
assert added == [('Inventec', 'FPGA IPU C5020X')], added

merged = json.loads((ROOT / 'data/smartnic-qct-facts-2026-10-04.json').read_text())
sourceproof = {}
for source in merged['sources']:
    sid = source['id']
    rows = [r for r in merged['devices'] if r['source'] == sid]
    assert new.execute('SELECT records_seen FROM sources WHERE id=?', (sid,)).fetchone()[0] == len(rows)
    for r in rows:
        rid = hashlib.sha256(json.dumps([sid, r['key']]).encode()).hexdigest()[:24]
        actual = new.execute('SELECT * FROM records WHERE id=?', (rid,)).fetchone()
        assert json.loads(actual['raw_json']) == r
        facts = collections.Counter((x[0], x[1]) for x in new.execute('SELECT name,value FROM facts WHERE record_id=?', (rid,)))
        assert facts == collections.Counter(set((n, str(v)) for n, v in r['facts'] if v is not None and str(v)))
        roles = {(x[0], x[1]) for x in new.execute('SELECT company,role FROM affiliations WHERE record_id=?', (rid,))}
        assert roles == set(map(tuple, r['affiliations']))
        # No manufacture/deployment role is inferred from QCT or Inventec branding.
        assert not roles & {('Quanta', 'manufacturer'), ('QCT', 'manufacturer'), ('Inventec', 'manufacturer')}
    sourceproof[sid] = dict(records=len(rows), literal_raw_facts_roles_equal=True)
new_facts = sum(new.execute('SELECT count(*) FROM facts f JOIN records r ON r.id=f.record_id WHERE r.source_id=?', (s['id'],))
                .fetchone()[0] for s in merged['sources'])
assert new_facts == 56
tpm = [r[0] for r in new.execute("SELECT f.value FROM facts f JOIN records r ON r.id=f.record_id JOIN devices d ON d.id=r.device_id "
                                 "WHERE d.model='FPGA IPU C5020X' AND f.value LIKE '%TPM%'")]

base = validation.run(new, ROOT / 'validation/owned-hardware.json')
previous = validation.run(old, ROOT / 'validation/owned-hardware.json')
for key in ('summary', 'results', 'search_summary'):
    assert base[key] == previous[key], key
byte = validation.run(new, ROOT / 'validation/vendor-examples.json')
assert byte['summary'] == {'total': 2, 'hits': 0, 'gaps': 2}
for c in [new] + ([reb] if reb else []):
    assert [r[0] for r in c.execute('PRAGMA integrity_check')] == ['ok']
    assert not c.execute('PRAGMA foreign_key_check').fetchall()

for row in old.execute('SELECT * FROM documents'):
    assert dict(new.execute('SELECT * FROM documents WHERE document_key=?', (row['document_key'],)).fetchone()) == dict(row)
for entry in json.loads((ROOT / 'data/raw/reference-documents-with-smartnic-qct-2026-10-05.json').read_text())['documents']:
    for key in ('pdf', 'text'):
        assert sha(ROOT / 'data/raw' / entry[key]['path']) == entry[key]['sha256'], entry[key]['path']

inventory = {}
for scope, expected in (('manufacturer', None), ('all', None)):
    before = vendor_inventory.run(old, scope=scope, limit=-1)
    after = vendor_inventory.run(new, scope=scope, limit=-1)
    inventory[scope] = dict(before=(before['identity_target_pairs'], before['unique_identities']),
                            after=(after['identity_target_pairs'], after['unique_identities']))
assert inventory['manufacturer']['before'] == inventory['manufacturer']['after'] == (2142, 2141), inventory
assert inventory['all']['before'] == (4227, 4089) and inventory['all']['after'] == (4228, 4090), inventory
assert count(old, 'hyperscaler_evidence') == count(new, 'hyperscaler_evidence')
assert sum(count(new, 'hyperscaler_evidence').values()) == 22

probes = [(q, limit) for q in ['C5020X', 'FPGA IPU C5020X', 'FPGA SmartNIC C5020X', 'T42S-2U', 'QuantaPlex T42S',
                               'TPM', '75W', 'ＦＰＧＡ', 'c5000x', 'Quanta'] for limit in (0, 1, 20, -1)]
assert lookup.run_many(new, probes) == [lookup.run(new, *p) for p in probes]
c5000x = lookup.run(new, 'C5000X', -1)['datasets'][0]['matches']
# The only C5000X text is the reviewed explicit no-alias disclaimer.
c5000x_text = [r[0] for r in new.execute("SELECT value FROM facts WHERE value LIKE '%C5000X%'")]
assert c5000x == 1 and len(c5000x_text) == 1 and 'C5000X alias' in c5000x_text[0] and 'No ' in c5000x_text[0]
assert not new.execute("SELECT 1 FROM identifiers WHERE value LIKE '%C5000X%'").fetchone()

proof = dict(
    phase='candidate/rebuild audited; not promoted',
    integrator='/root-claude',
    preservation=preservation,
    duplicate_sensitive_table_comparison=comparison if reb else 'rebuild pending',
    tables_compared=len(tables), fts=fts,
    intentionally_changed_old_device_fts=changed, new_identities=added,
    source_rows_literal_proof=sourceproof, new_facts=new_facts,
    english_ipu_tpm_rows_literal=tpm, c5000x_catalog_matches=c5000x,
    owned_baseline=base['summary'], bytedance_examples=byte['summary'],
    inventory=inventory, hyperscaler_assertions=22, integrity_and_foreign_keys='pass',
    lookup_single_vs_batch_probes=len(probes),
    candidate_sha256=sha(CANDIDATE), rebuild_sha256=sha(REBUILT) if reb else None,
    live_sha256=sha(LIVE), live_manifest_sha256=sha(ROOT / 'data/import-manifest.json'),
    staged_manifest_sha256=sha(ROOT / 'data/import-manifest-smartnic-qct-2026-10-05.json'))
(O / ('candidate-verification.json' if reb else 'candidate-verification-prerebuild.json')).write_text(
    json.dumps(proof, indent=2, ensure_ascii=False) + '\n')
print(json.dumps({k: proof[k] for k in ('new_identities', 'new_facts', 'owned_baseline', 'bytedance_examples', 'inventory',
                                         'english_ipu_tpm_rows_literal', 'c5000x_catalog_matches')}, ensure_ascii=False))
print('rebuild compared' if reb else 'rebuild pending')
