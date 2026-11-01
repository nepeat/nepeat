"""Independent, bounded review probes against a byte-identical private overlay."""
import collections
import contextlib
import csv
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from unittest.mock import patch

import devicehunt
from devicehunt import specifications, __main__ as cli
from devicehunt.catalog import Catalog

OWN = Path(__file__).resolve().parent
assert Path.cwd() == OWN
assert Path(os.environ['PYTHONPATH']).resolve() == OWN / 'overlay'
assert all(Path(m.__file__).resolve().is_relative_to(OWN / 'overlay')
           for m in (devicehunt, specifications, cli))
spec = importlib.util.spec_from_file_location('devicehunt._review_original', OWN / 'originals/specifications.py')
legacy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(legacy)
proof = {'module_paths': {m.__name__: m.__file__ for m in (devicehunt, specifications, cli)}, 'cases': []}

def capture(groups, fmt):
    stream = io.StringIO()
    cli.write_specification_groups(groups, fmt, stream)
    return stream.getvalue()

with tempfile.TemporaryDirectory(dir=OWN / 'tmp') as temp:
    path = Path(temp) / 'fixture.sqlite'
    c = Catalog(path)
    records = {}
    for sid in ('a', 'b'):
        c.source(sid, 'https://example.invalid/' + sid, 'vendor', 'fixture', 'fixture')
    for sid, key, brand, revision, company in [
            ('a', 'one', 'Celestica', '', 'Celestica'),
            ('a', 'two', 'Celestica', '', 'Oracle'),
            ('b', 'one', 'Celestica', '', 'Celestica'),
            ('b', 'brand', 'Oracle', '', 'Oracle'),
            ('b', 'revision', 'Celestica', 'R2', 'Celestica')]:
        identifiers = [('alias', 'ONLY-TWO'), ('part_number', 'PN-TWO')] if key == 'two' else []
        facts = [('power_description', 'same'), ('spec_power_supply', 'same'),
                 ('psu_configuration_1', 'same'), ('psu_configuration_2', 'same'),
                 ('spec_form_factor', 'unknown'), ('unreviewed_name', 'same')]
        c.add(sid, key, 'https://example.invalid/' + sid + '/' + key, brand, 'REVIEW-1',
              'review fixture', 'server', {}, [(company, 'documentation_vendor')],
              identifiers=identifiers, facts=facts, revision=revision)
    c.finish('a', 2)
    c.finish('b', 3)
    for row in c.db.execute('SELECT id,source_id,source_key FROM records'):
        records[(row['source_id'], row['source_key'])] = row['id']
    all_records = set(records.values())
    primary = {records[('a', 'one')], records[('a', 'two')], records[('b', 'one')]}
    celestica = all_records - {records[('a', 'two')], records[('b', 'brand')]}
    oracle = all_records - celestica
    cases = [
        ('discovery', 'REVIEW-1', {}, all_records),
        ('literal-model', 'REVIEW-1', {'exact_model': True}, all_records),
        ('alias', 'only-two', {'exact_model': True}, {records[('a', 'two')]}),
        ('part', 'pn-two', {'exact_part_number': True}, {records[('a', 'two')]}),
        ('vendor', 'REVIEW-1', {'target': 'Celestica'}, celestica),
        ('alias-wrong-vendor', 'only-two', {'exact_model': True, 'target': 'Celestica'}, set()),
        ('part-vendor', 'pn-two', {'exact_part_number': True, 'target': 'Oracle'}, {records[('a', 'two')]}),
        ('no-suffix-match', 'REVIEW-1-X', {'exact_model': True}, set()),
    ]
    before = hashlib.sha256(path.read_bytes()).hexdigest()
    with patch('urllib.request.urlopen', side_effect=AssertionError('network forbidden')), \
         patch('socket.create_connection', side_effect=AssertionError('network forbidden')), \
         patch('subprocess.Popen', side_effect=AssertionError('subprocess forbidden')):
        for label, query, filters, eligible in cases:
            for include in (False, True):
                kwargs = filters | {'include_source_facts': include, 'limit': -1}
                grouped = specifications.search(c, query, group_identical=True, **kwargs)
                members = [member for group in grouped for member in group['evidence']]
                expected = collections.Counter(
                    (row['record_id'], row['name'], row['value'])
                    for row in c.db.execute('SELECT record_id,name,value FROM facts')
                    if row['record_id'] in eligible and row['name'] in
                    ({'power_description', 'spec_power_supply', 'psu_configuration_1', 'psu_configuration_2'}
                     if include else {'power_description'}))
                actual = collections.Counter((m['record_id'], m['raw_fact']['name'], m['raw_fact']['value']) for m in members)
                assert actual == expected, (label, include, actual, expected)
                assert specifications.search(c, query, **kwargs) == legacy.search(c, query, **kwargs)
                assert specifications.search(c, query, group_identical=True, **(kwargs | {'limit': 1})) == grouped[:1]
                for group in grouped:
                    assert group['evidence_count'] == len(group['evidence'])
                    assert group['record_count'] == len({m['record_id'] for m in group['evidence']})
                    assert group['source_count'] == len({m['source_id'] for m in group['evidence']})
                if grouped:
                    encoded = list(csv.DictReader(io.StringIO(capture(grouped, 'csv'))))
                    assert [json.loads(row['evidence_json']) for row in encoded] == [g['evidence'] for g in grouped]
                    assert json.loads(capture(grouped, 'json')) == grouped
                    assert capture(grouped, 'table').count('  evidence ') == len(members)
                proof['cases'].append({'name': label, 'include_source_facts': include,
                                       'groups': len(grouped), 'evidence_rows': len(members),
                                       'raw_multiset_default_parity_counts_limit_formats': True})
    assert hashlib.sha256(path.read_bytes()).hexdigest() == before
    proof['fixture_sha256_before'] = proof['fixture_sha256_after'] = before

    # Two raw facts on the SAME record reduce to an identical displayed statement.
    # The raw path in these values is not repeated by this fixture's record URL.
    target_record = records[('a', 'one')]
    raw_values = ['Rack Mount Chassis (a/0123456789ab:1)', 'Rack Mount Chassis (b/0123456789ab:2)']
    c.db.executemany('INSERT INTO facts VALUES (?,?,?)', [(target_record, 'dmi_chassis_type', value) for value in raw_values])
    c.db.commit()
    groups = specifications.search(c, 'REVIEW-1', exact_model=True, include_source_facts=True,
                                  group_identical=True, limit=-1)
    dmi = [g for g in groups if g['source_fact_name'] == 'dmi_chassis_type']
    assert len(dmi) == 2
    assert dmi[0]['statement'] == dmi[1]['statement']
    table_blocks = [capture([g], 'table') for g in dmi]
    assert table_blocks[0] == table_blocks[1]
    assert all(raw not in ''.join(table_blocks) for raw in raw_values)
    assert {g['evidence'][0]['raw_fact']['value'] for g in dmi} == set(raw_values)
    (OWN / 'dmi-raw-evidence.json').write_text(json.dumps(dmi, indent=2) + '\n')
    (OWN / 'dmi-table-output.txt').write_text(''.join(table_blocks))
    proof['table_raw_value_loss_reproduced'] = True
    proof['two_distinct_raw_groups_render_identically'] = True
    c.db.close()

proof['help_widths'] = []
for width in (40, 80, 120):
    env = os.environ | {'COLUMNS': str(width)}
    done = subprocess.run([sys.executable, '-B', '-m', 'devicehunt', 'specs', '--help'],
                          cwd=OWN, env=env, capture_output=True, text=True, check=True)
    (OWN / ('help-width-' + str(width) + '.txt')).write_text(done.stdout)
    normalized = ' '.join(done.stdout.split())
    assert '--group-identical' in normalized and 'Limits count complete groups' in normalized
    proof['help_widths'].append({'columns': width, 'contiguous_phrase': 'Limits count complete groups' in done.stdout,
                                 'whitespace_normalized_phrase': True})
(OWN / 'independent-checks.json').write_text(json.dumps(proof, indent=2) + '\n')
print(json.dumps(proof, indent=2))
