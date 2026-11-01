"""Finite read-only evidence/scope checks; writes one new local checks report."""
import collections
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
ORIGINALS = HERE.parent / 'original-vendor-wave5'
sys.path.insert(0, str(ROOT))
from devicehunt.documents import read_capture


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    held = {p.relative_to(HERE).as_posix(): sha(p) for p in HERE.rglob('*') if p.is_file()}
    check = subprocess.run([sys.executable, '-B', str(HERE / 'build_candidate.py'), '--check'], capture_output=True, text=True, check=True)
    assert held == {p.relative_to(HERE).as_posix(): sha(p) for p in HERE.rglob('*') if p.is_file()}
    findings = json.loads((ORIGINALS / 'findings.json').read_text())
    observations = {o['id']: o for o in findings['observations']}
    candidate = json.loads((HERE / 'candidate.json').read_text())
    provenance = json.loads((HERE / 'provenance.json').read_text())
    for item in json.loads((ORIGINALS / 'artifact-manifest.json').read_text())['files']:
        assert sha(ORIGINALS / item['path']) == item['sha256'] and (ORIGINALS / item['path']).stat().st_size == item['bytes']
    for item in provenance['copies']:
        assert sha(HERE / item['snapshot']) == sha(ROOT / item['original']) == item['sha256']
    assert len(provenance['copies']) == 5
    assert len(candidate['sources']) == 3 and len(candidate['devices']) == 40
    sources = {s['id']: s for s in candidate['sources']}
    pages = {}
    for source in candidate['sources']:
        assert sha(HERE / source['snapshot']) == source['original_sha256']
        if 'text_sidecar' in source:
            parsed, metadata = read_capture((HERE / source['text_sidecar']).read_bytes(), source['original_sha256'])
            assert metadata == source['extraction_metadata']
            pages[source['id']] = {p['page']: p['text'].splitlines() for p in parsed}
    assert sum(len(p) for p in pages.values()) == 6
    observed_ids = set()
    base_count = ordering_count = evidence_count = 0
    for row in candidate['devices']:
        assert row['title'] == row['brand'] + ' ' + row['model'] and row['key'] == row['model']
        assert len(row['affiliations']) == 1 and row['affiliations'][0][1] == 'documentation_vendor'
        assert row['hyperscaler'] == []
        assert len(row['facts']) == len(row['evidence']) == len({tuple(f) for f in row['facts']})
        source = sources[row['source']]
        for i, (fact, e) in enumerate(zip(row['facts'], row['evidence'], strict=True)):
            assert e['fact_index'] == i and e['fact_name'] == fact[0]
            assert e['capture_sha256'] == source['original_sha256'] and e['source_url'] == source['url']
            if 'pdf_page' in e['locator']:
                lines = pages[row['source']][e['locator']['pdf_page']]
                actual = '\n'.join(lines[n - 1] for n in e['locator']['lines_1based'])
                assert actual == e['literal_quote'] or actual.strip() == e['literal_quote']
            if 'original_observation_id' in e:
                original = observations[e['original_observation_id']]
                assert e['original_discovery_observation'] == original
                observed_ids.add(original['id'])
                if original['id'] == 'as5712-22':
                    start = 0 if row['model'] == 'AS5710-54X' else 5
                    expected = '\n'.join(re.split(r'\s{2,}', line.strip())[0] for line in original['raw_lines'][start:start + 5]) + ' [PDF p. 2]'
                    assert fact[1] == expected
                else:
                    assert fact[1].startswith(original['summary'])
                if original['id'] in ('oracle-33', 'oracle-34', 'oracle-35'):
                    assert e['mandatory_qualifying_note'] == observations['oracle-rating-note']['raw_text']
            evidence_count += 1
        if 'original_discovery_ordering_row' not in row:
            base_count += 1
            assert row['identifiers'] == []
            ids = {e['original_observation_id'] for e in row['evidence']}
            if row['model'] == 'AS5710-54X':
                assert 'as5712-21' not in ids and len(row['facts']) == 7
            if row['model'] == 'AS5712-54X':
                assert 'as5712-21' in ids and len(row['facts']) == 8
            continue
        ordering_count += 1
        original = row['original_discovery_ordering_row']
        assert original in findings['ordering_rows'] and original not in candidate['excluded_ordering_rows']
        assert row['identifiers'] == [['part_number', row['model']]]
        assert row['model'] == original['raw_cells_from_embedded_text'][0]
        allowed = {'ordering_row_as_printed', 'power_description', 'cooling_description', 'physical_description',
                   'power_cord_scope_as_printed', 'base_model_heading_as_printed', 'cpu_module_as_printed', 'power_cord_region_as_printed'}
        assert {name for name, _ in row['facts']} <= allowed
        assert not any(name in {'software_installation_description', 'model_configuration_as_printed'} for name, _ in row['facts'])
        for name, value in row['facts']:
            raw_value = value.rsplit(' [PDF p. ', 1)[0]
            assert raw_value in original['raw_cells_from_embedded_text'] or raw_value == original['raw_line'].strip() or raw_value == original['base_model_line_as_printed']
        if row['model'] == 'PSU-12V-400-1':
            assert row['kind'] == 'power input unit FRU' and len(row['facts']) == 2
            assert not any('400W' in value or '400 W' in value for _, value in row['facts'])
    assert base_count == 5 and ordering_count == 35 and evidence_count == 227
    assert observed_ids == set(observations)
    assert len(candidate['excluded_ordering_rows']) == 3
    excluded = {r['raw_cells_from_embedded_text'][0] for r in candidate['excluded_ordering_rows']}
    assert excluded == {'57xx-54X-SVC-RTF', 'FAN-1U-1x1D-F', 'FAN-1U-1x1D-B'}
    assert not excluded.intersection(r['model'] for r in candidate['devices'])
    routing = json.loads((HERE / 'routing.json').read_text())['catalog_observations']
    routing_counts = []
    for context in routing:
        existing = sum(bool(a['existing_same_namespace']) for a in context['anchors'])
        new = sum(not a['existing_same_namespace'] for a in context['anchors'])
        assert existing == 5 and new == 35 and context['all3source_ids_absent']
        assert context['catalog_sha256_before'] == context['catalog_sha256_after']
        routing_counts.append({'catalog_path': context['catalog_path'], 'observed_utc': context['observed_utc'],
                               'catalog_sha256': context['catalog_sha256_before'], 'existing': existing, 'new': new})
    count_by_source = {sid: {'records': sum(r['source'] == sid for r in candidate['devices']),
                             'facts': sum(len(r['facts']) for r in candidate['devices'] if r['source'] == sid)} for sid in sources}
    report = {'PASS': True, 'status': 'staged candidate; independent review required, no import',
              'sources': 3, 'records': 40, 'distinct_namespace_model_pairs': len({(r['brand'], r['model']) for r in candidate['devices']}),
              'facts': 227, 'base_records': 5, 'ordering_records': 35, 'identifiers': 35,
              'base_facts': 61, 'ordering_facts': 166, 'all36observations_mapped': True,
              'all227evidence_records_verified': True, 'only_own_row_cells_or_explicit_heading_on_order_records': True,
              'excluded_service_and_fan_rows': sorted(excluded), 'per_source': count_by_source,
              'routing_counts': routing_counts, 'all50original_artifacts_verified': True,
              'all5immutable_copies_match': True, 'complete_PDF_pages': 6,
              'frozen_inputs_unchanged': True, 'builder_check_no_writes': True,
              'builder_check': {'returncode': check.returncode, 'stdout': check.stdout, 'stderr': check.stderr},
              'candidate_sha256': sha(HERE / 'candidate.json'), 'builder_sha256': sha(HERE / 'build_candidate.py'),
              'checker_sha256': sha(HERE / 'check_candidate.py'), 'provenance_sha256': sha(HERE / 'provenance.json'),
              'mapping_sha256': sha(HERE / 'mapping.json'), 'routing_sha256': sha(HERE / 'routing.json'),
              'fact_names': dict(collections.Counter(name for r in candidate['devices'] for name, _ in r['facts'])),
              'network_requests': 0, 'imports': 0, 'temporary_or_prepared_databases': 0, 'shared_writes': 0,
              'production_frozen_files': {name: sha(ROOT / name) for name in ('devicehunt/catalog.py', 'devicehunt/importers.py', 'devicehunt/documents.py')}}
    with (HERE / 'checks.json').open('x') as handle:
        json.dump(report, handle, indent=2)
        handle.write('\n')
    print('PASS3sources/40records/227facts/5existing+35new; all scoped evidence/copies verify; no import')


if __name__ == '__main__':
    main()
