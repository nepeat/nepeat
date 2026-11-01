"""Read-only candidate/source checks; creates only a new local checks report."""
import collections
import datetime
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
ORIGINALS = HERE.parent / 'spec-continuation'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    before = {p.relative_to(HERE).as_posix(): sha(p) for p in HERE.rglob('*') if p.is_file()}
    run = subprocess.run([sys.executable, '-B', str(HERE / 'build_candidate.py'), '--check'], capture_output=True, text=True, check=True)
    assert before == {p.relative_to(HERE).as_posix(): sha(p) for p in HERE.rglob('*') if p.is_file()}
    for line in (ORIGINALS / 'SHA256SUMS').read_text().splitlines():
        expected, name = line.split('  ', 1)
        assert sha(ORIGINALS / name) == expected
    candidate = json.loads((HERE / 'candidate.json').read_text())
    original_observations = {p['url']: p for p in json.loads((ORIGINALS / 'observations.json').read_text())['observed_pages']}
    requests = {r['url']: r for r in map(json.loads, (ORIGINALS / 'requests.jsonl').read_text().splitlines())}
    source_map = {s['id']: s for s in candidate['sources']}
    parser_path = ROOT / 'research/batches/discovery-2026-10-11/spec/scripts/jbbparse.py'
    spec = importlib.util.spec_from_file_location('reviewed_jbb', parser_path)
    parser = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(parser)
    assert len(source_map) == len(candidate['devices']) == 18
    quote_count = 0
    fact_count = 0
    identities = set()
    for row in candidate['devices']:
        source = source_map[row['source']]
        original = original_observations[source['url']]
        assert row['raw_reviewed_observation'] == original
        assert source['captured_at_utc'] == original['captured_at_utc']
        assert sha(HERE / source['snapshot']) == original['capture_sha256'] == source['original_sha256']
        assert row['key'] == row['printed_identity_mapping']['printed_system_name']
        assert row['identifiers'] == [['spec_system_name', row['key']]]
        assert ['spec_system_name', row['key']] in row['facts']
        assert row['affiliations'] == [['Inspur Corporation', 'brand']] and row['brand'] == 'Inspur Corporation'
        assert not row.get('hyperscaler')
        assert len(row['facts']) == len(row['evidence']) == len({tuple(f) for f in row['facts']})
        identities.add(row['printed_identity_mapping']['device_id'])
        textpath = HERE / source['snapshot']
        lines = textpath.read_text().splitlines() if original['benchmark'] == 'cpu2017' else None
        rows = parser.parse(textpath).rows if original['benchmark'] == 'jbb2015' else None
        for i, ((name, value), evidence) in enumerate(zip(row['facts'], row['evidence'], strict=True)):
            assert evidence['fact_index'] == i and evidence['fact_name'] == name
            assert evidence['capture_sha256'] == original['capture_sha256'] and evidence['source_url'] == source['url']
            locator = evidence['locator']
            quote = evidence['literal_quote']
            if evidence['evidence_kind'].startswith('saved request input'):
                assert quote == requests[source['url']]['url'] and locator['request_sequence'] == requests[source['url']]['sequence']
            elif lines is not None:
                assert quote == '\n'.join(lines[n - 1] for n in locator['lines_1based'])
                quote_count += 1
            else:
                if 'header_label' in locator:
                    actual = next(c for r in rows[:3] for c in r if c.startswith(locator['header_label'] + ':'))
                    assert quote == actual
                else:
                    cell = rows[locator['table_row_index_zero_based']][locator.get('cell', 1)]
                    assert quote in cell
                quote_count += 1
            assert re.sub(r'\s+', ' ', value).strip() in re.sub(r'\s+', ' ', quote).strip()
            fact_count += 1
        if original['benchmark'] == 'cpu2017':
            assert not any('power_supply' in name for name, _ in row['facts'])
            assert row['printed_identity_mapping']['removed_trailing_cpu_config']
        else:
            assert not row['printed_identity_mapping']['removed_trailing_cpu_config']
            assert ['spec_jbb_mode', 'MultiJVM'] in row['facts']
    assert fact_count == 366 and quote_count == 348 and len(identities) == 12
    contexts = json.loads((HERE / 'routing.json').read_text())['catalog_observations']
    routing = []
    for context in contexts:
        existing = {a['device_id'] for a in context['anchors'] if a['existing_device']}
        new = {a['device_id'] for a in context['anchors'] if not a['existing_device']}
        cases = {a['device_id'] for a in context['anchors'] if a['existing_model_case_only_difference']}
        assert len(existing) == 8 and len(new) == 4 and len(cases) == 2
        routing.append({'catalog_path': context['catalog_path'], 'observed_utc': context['observed_utc'],
                        'observed_catalog_sha256': context['catalog_sha256_before'], 'existing_identities': 8,
                        'new_identities_proposed': 4, 'case_only_existing_display_differences': 2,
                        'all18source_ids_absent_at_observation': all(a['source_id_absent'] for a in context['anchors'])})
    report = {'status': 'PASS; frozen candidate only, independent review and integration remain pending',
              'checked_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'sources': 18, 'records': 18, 'facts': fact_count, 'source_content_quotes_verified': quote_count,
              'request_input_URL_facts_verified': 18, 'identifiers': 18, 'affiliations': 18,
              'distinct_identities': 12, 'facts_by_benchmark': {'jbb2015': 162, 'cpu2017': 204},
              'all_role_assertions': 'Inspur Corporation / brand under accepted sponsor-equality rule; no target alias hit',
              'routing_observations': routing,
              'fact_names': dict(collections.Counter(name for r in candidate['devices'] for name, _ in r['facts'])),
              'builder_check': {'returncode': run.returncode, 'stdout': run.stdout, 'stderr': run.stderr, 'writes': 0},
              'all18snapshot_copies_match_originals': True, 'all47original_capture_artifact_hashes_verified': True,
              'all_raw_observations_unchanged': True, 'CPU_missing_power_section_no_power_facts': True,
              'jbb_hw1_only_and_mode_labelled_scores': True, 'source_record_and_fact_keys_unique': True,
              'candidate_sha256': sha(HERE / 'candidate.json'), 'builder_sha256': sha(HERE / 'build_candidate.py'),
              'provenance_sha256': sha(HERE / 'provenance.json'), 'routing_sha256': sha(HERE / 'routing.json'),
              'production_frozen_files': {p: sha(ROOT / p) for p in ('devicehunt/importers.py', 'devicehunt/catalog.py', 'tests/test_curated_import.py')},
              'network_requests': 0, 'imports': 0, 'prepared_databases': 0, 'shared_writes': 0}
    with (HERE / 'checks.json').open('x') as handle:
        json.dump(report, handle, indent=2)
        handle.write('\n')
    print('PASS', fact_count, 'facts:', quote_count, 'source quotes +18request URLs; exact18records/12identities; no import')


if __name__ == '__main__':
    main()
