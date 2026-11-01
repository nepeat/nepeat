"""Build the eighteen reviewed SPEC records under accepted revision-3 rules; no import."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
sys.path.insert(0, str(ROOT))
from devicehunt.catalog import normalize, target_for

ORIGINALS = HERE.parent / 'spec-continuation'
OBS_SHA = '17f9894fbb70530a521599d7a0d0a91afb74daafe3762a5b2e9a8c6047b79645'
REQ_SHA = '13f86f17d4c6d814a8ddf31a1c2dd326021f841719f3adb653d783250cca6570'
CPU_WORDS = re.compile(r'intel|amd|xeon|epyc|hygon|ampere|sparc|ghz|\bcores?\b|opteron|processor|\bcpu\b', re.I)
JBB_LABELS = ['Model', 'Form Factor', 'CPU Name', 'CPU Characteristics', 'CPU Frequency (MHz)',
              'Nodes Per System', 'Number of Systems', 'Chips Per System', 'Cores Per System',
              'Cores Per Chip', 'Threads Per System', 'Memory Amount (GB)', '# and size of DIMM(s)',
              'Memory Details', 'Power Supply Quantity and Rating (W)', 'Cabinet/Housing/Enclosure',
              'Shared Description', 'Shared Comment', 'Available']
LIMITS = ('Historical vendor-submitted test description published by SPEC, not manufacture, OEM, factory, '
          'operator, deployment, installed auction-unit inventory, current support or universal model capability. '
          'Test Sponsor/Tested by are facts; affiliation is only the old reviewed brand/reported_system_vendor rule. '
          'Full printed name remains in spec_system_name fact/identifier/raw data; CPU/config stripping does not '
          'bridge other brands, remove printed vendor prefixes or decode chassis/revision/order-code suffixes. '
          'Power quantities/ratings are submitted configurations, not summed available capacity, exact PSU '
          'identity, measured consumption, redundancy or general compatibility. Benchmark scores are historical '
          'results for the stated benchmark/mode and not hardware specifications.')


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + '\n').encode()


def save(path, value, check):
    body = encoded(value)
    if check or path.exists():
        assert path.read_bytes() == body, str(path)
    else:
        with path.open('xb') as handle:
            handle.write(body)


def label_name(label):
    label = label.replace('(s)', 's').replace('#', 'number')
    return 'spec_' + re.sub(r'[^a-z0-9]+', '_', label.lower()).strip('_')


def mapping(vendor, printed, sponsor):
    model = printed
    tail = re.search(r'\s*\(([^()]*)\)\s*$', printed)
    if tail and tail.start() > 0 and CPU_WORDS.search(tail[1]):
        model = printed[:tail.start()]
    brand = target_for(vendor) or vendor
    role = 'brand' if re.sub(r'\s+', ' ', sponsor).strip() == re.sub(r'\s+', ' ', vendor).strip() else 'reported_system_vendor'
    did = hashlib.sha256(json.dumps([normalize(brand), model.casefold().strip(), '']).encode()).hexdigest()[:20]
    return {'printed_hardware_vendor': vendor, 'brand': brand, 'printed_system_name': printed,
            'model': model, 'removed_trailing_cpu_config': printed[len(model):],
            'test_sponsor': sponsor, 'role': role, 'device_id': did}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='read-only verification of frozen staged bytes')
    args = parser.parse_args()
    assert sha(ORIGINALS / 'observations.json') == OBS_SHA
    assert sha(ORIGINALS / 'requests.jsonl') == REQ_SHA
    observations = json.loads((ORIGINALS / 'observations.json').read_text())['observed_pages']
    requests = {r['url']: r for r in map(json.loads, (ORIGINALS / 'requests.jsonl').read_text().splitlines())}
    sources, devices, artifacts, fieldmaps = [], [], [], []
    for observed in observations:
        request = requests[observed['url']]
        assert request['status'] == 200 and request['body_sha256'] == observed['capture_sha256']
        original = ORIGINALS / observed['body_path']
        assert sha(original) == observed['capture_sha256']
        snapshot = HERE / 'snapshots' / observed['body_path']
        if not args.check:
            snapshot.parent.mkdir(parents=True, exist_ok=True)
        if args.check or snapshot.exists():
            assert snapshot.read_bytes() == original.read_bytes()
        else:
            with snapshot.open('xb') as handle:
                handle.write(original.read_bytes())
            stat = original.stat()
            os.utime(snapshot, ns=(stat.st_atime_ns, stat.st_mtime_ns))
        artifacts.append({'snapshot': snapshot.relative_to(HERE).as_posix(),
                          'original': original.relative_to(ROOT).as_posix(), 'sha256': sha(original),
                          'bytes': original.stat().st_size, 'type': 'raw HTML capture' if original.suffix == '.html' else 'raw full-disclosure text capture',
                          'captured_at_utc': request['finished_at_utc'], 'original_mtime_ns': original.stat().st_mtime_ns})
        sid = 'spec-' + original.stem
        facts, evidence = [], []
        def add(name, value, quote, locator, *, kind='captured source content'):
            if value == 'None':
                return
            assert value and value not in ('--', 'Not Run')
            assert re.sub(r'\s+', ' ', value).strip() in re.sub(r'\s+', ' ', quote).strip()
            facts.append([name, value])
            evidence.append({'fact_index': len(facts) - 1, 'fact_name': name, 'literal_quote': quote,
                             'locator': locator, 'evidence_kind': kind, 'source_url': observed['url'],
                             'capture_sha256': observed['capture_sha256']})
        if observed['benchmark'] == 'jbb2015':
            assert [b['section_as_printed'] for b in observed['hardware_blocks']] == ['Hardware hw_1']
            block = {f['label_as_printed']: f for f in observed['hardware_blocks'][0]['fields']}
            header = {c.split(':', 1)[0]: c for r in observed['header_rows'][1:] for c in r['raw_cells_whitespace_normalized'] if ':' in c}
            vendor = block['Vendor']['value_as_printed_cell_text']
            printed = block['Name']['value_as_printed_cell_text']
            sponsor = header['Test Sponsor'].split(':', 1)[1].strip()
            route = mapping(vendor, printed, sponsor)
            title = observed['header_rows'][0]['raw_cells_whitespace_normalized'][0]
            metric = observed['header_rows'][0]['raw_cells_whitespace_normalized'][1]
            add('spec_benchmark', 'SPECjbb2015', 'SPECjbb2015', {'table_row_index_zero_based': 0, 'cell': 1})
            add('spec_system_title_line', title, title, {'table_row_index_zero_based': 0, 'cell': 0})
            for label, name in (('Vendor', 'spec_hardware_vendor'), ('Name', 'spec_system_name')):
                f = block[label]
                add(name, f['value_as_printed_cell_text'], f['value_as_printed_cell_text'], f['locator'])
            for label, name in (('Test Sponsor', 'spec_test_sponsor'), ('Tested by', 'spec_tested_by'), ('Hardware Availability', 'spec_hardware_availability')):
                quote = header[label]
                add(name, quote.split(':', 1)[1].strip(), quote, {'header_label': label})
            for label in JBB_LABELS:
                if label in block:
                    f = block[label]
                    add(label_name(label), f['value_as_printed_cell_text'], f['value_as_printed_cell_text'], f['locator'])
            match = re.fullmatch(r'(\d+) SPECjbb2015-(\S+) max-jOPS (\d+) SPECjbb2015-\2 critical-jOPS', metric)
            assert match
            add('spec_jbb_mode', match[2], 'SPECjbb2015-' + match[2], {'table_row_index_zero_based': 0, 'cell': 1})
            for label, n in (('max', 1), ('critical', 3)):
                value = match[n] + ' SPECjbb2015-' + match[2] + ' ' + label + '-jOPS'
                add('spec_result_' + label + '_jops', value, value, {'table_row_index_zero_based': 0, 'cell': 1})
            section_limits = ('Only Hardware hw_1 is attributed. No hw_2 block appears in these six captures. '
                              'Cell text uses the reviewed stdlib parser with whitespace normalization; raw HTML retained. '
                              'max-jOPS/critical-jOPS retain printed mode labels; modes are not cross-comparable. '
                              'Software/JVM/tuning/topology and RT-curve fields are not selected. Source None fields remain raw omissions.')
            title_difference = '' if title == vendor + ' ' + printed else ' Header title differs from hw_1 Vendor + Name; hw_1 fields control the namespace: ' + repr(title) + '.'
        else:
            lines = original.read_text(encoding='utf-8').splitlines()
            printed = observed['saved_index_row']['hw_model']
            title_lines = []
            for n, line in enumerate(lines[1:8], 2):
                if not line.strip() or line.strip().startswith('Test Sponsor:'):
                    break
                title_lines.append((n, line))
            system = re.sub(r'\s+', ' ', ' '.join(l.strip() for _, l in title_lines)).strip()
            assert system.endswith(printed) and len(system) > len(printed)
            vendor = system[:-len(printed)].rstrip()
            assert system == vendor + ' ' + printed
            sponsor_line = next((i + 1, line) for i, line in enumerate(lines[:10]) if 'Test sponsor:' in line)
            sponsor_match = re.search(r'Test sponsor:\s+(.*?)\s{2,}Hardware availability:\s+(.*?)\s*$', sponsor_line[1])
            assert sponsor_match
            sponsor = sponsor_match[1]
            route = mapping(vendor, printed, sponsor)
            add('spec_benchmark', lines[0].strip(), lines[0], {'lines_1based': [1]})
            quote = '\n'.join(l for _, l in title_lines)
            locator = {'lines_1based': [n for n, _ in title_lines]}
            add('spec_system_title_line', system, quote, locator)
            add('spec_hardware_vendor', vendor, quote, locator)
            add('spec_system_name', printed, quote, locator)
            add('spec_test_sponsor', sponsor, sponsor_line[1], {'lines_1based': [sponsor_line[0]]})
            add('spec_hardware_availability', sponsor_match[2], sponsor_line[1], {'lines_1based': [sponsor_line[0]]})
            tested_line = next((i + 1, line) for i, line in enumerate(lines[:10]) if 'Tested by:' in line)
            tested = re.search(r'Tested by:\s+(.*?)\s{2,}Software availability:', tested_line[1])
            assert tested
            add('spec_tested_by', tested[1], tested_line[1], {'lines_1based': [tested_line[0]]})
            assert not observed['power_section_present']
            for section in observed['hardware_sections']:
                assert section['section_as_printed'] == 'HARDWARE'
                for f in section['fields']:
                    if f['label_as_printed'] in ('CPU Name', 'Max MHz', 'Nominal', 'Enabled', 'Orderable', 'Memory', 'Storage'):
                        value = ' '.join(l.strip() for l in f['value_lines_as_printed'])
                        add(label_name(f['label_as_printed']), value, '\n'.join(f['raw_lines']),
                            {'section': 'HARDWARE', 'lines_1based': f['lines_1based']})
            for i, line in enumerate(lines, 1):
                match = re.match(r'^\s*(SPEC(?:rate|speed)\(R\)2017_\w+)\s+(\S.*?)\s*$', line)
                if match and 'Not Run' not in match[2] and match[2] != '--':
                    add('spec_result_' + label_name(match[1])[5:], match[2], line, {'lines_1based': [i]})
            section_limits = ('Hardware Vendor is split from the printed title block only by its exact saved-index hw_model suffix, '
                              'as in accepted revision3. Wrapped hardware values join with one space; raw lines retained. '
                              'No POWER-RELEVANT HARDWARE section occurs, so no power-supply fact is selected; this is not a claim '
                              'that the machine lacks PSUs or other power-related tuning text. CPU metrics retain exact rate/speed labels; '
                              'software/firmware/tuning and per-benchmark table rows are not selected as hardware capability.')
            title_difference = ''
        add('spec_result_url', observed['url'], observed['url'], {'request_sequence': request['sequence'], 'request_field': 'url'}, kind='saved request input, not document quote')
        rule = ("Revision3 identity rules: printed Hardware Vendor/hw_1 Vendor '" + vendor + "' -> brand '" + route['brand'] +
                "' using only reviewed target_for aliases; full printed system '" + printed + "' -> model '" + route['model'] +
                "' by removing only a trailing CPU/config parenthetical under the accepted CPU-word rule; no other string change. "
                "Affiliation is ['" + vendor + "', '" + route['role'] + "'] by Test Sponsor equality; Tested by creates no role. ")
        sources.append({'id': sid, 'url': observed['url'], 'tier': 'vendor',
                        'license': 'SPEC published vendor-submitted full disclosure; original SPEC/submitter rights retained; local factual extraction.',
                        'coverage': LIMITS + ' ' + rule + section_limits + title_difference,
                        'snapshot': snapshot.relative_to(HERE).as_posix(), 'original_sha256': observed['capture_sha256'],
                        'captured_at_utc': request['finished_at_utc']})
        devices.append({'source': sid, 'key': printed, 'brand': route['brand'], 'model': route['model'],
                        'title': route['brand'] + ' ' + route['model'], 'kind': 'server',
                        'affiliations': [[vendor, route['role']]], 'identifiers': [['spec_system_name', printed]],
                        'facts': facts, 'evidence': evidence, 'printed_identity_mapping': route,
                        'raw_reviewed_observation': observed, 'scope_limitations': LIMITS + ' ' + section_limits})
        fieldmaps.append({'source_id': sid, 'mapping': route, 'fact_names': [f[0] for f in facts], 'fact_count': len(facts)})
    assert len(sources) == len(devices) == 18
    assert len({s['id'] for s in sources}) == 18
    assert len({(d['source'], d['key']) for d in devices}) == 18
    contexts = []
    for filename in ('data/devices.sqlite', 'data/devices-candidate-wave3-2026-10-12.sqlite'):
        path = ROOT / filename
        before = sha(path)
        connection = sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True)
        connection.row_factory = sqlite3.Row
        try:
            connection.execute('BEGIN')
            anchors = []
            for item in fieldmaps:
                route = item['mapping']
                assert not connection.execute('SELECT 1 FROM sources WHERE id=?', (item['source_id'],)).fetchone()
                row = connection.execute('SELECT * FROM devices WHERE id=?', (route['device_id'],)).fetchone()
                if row:
                    assert row['brand'] == route['brand'] and row['model'].casefold().strip() == route['model'].casefold().strip()
                anchors.append({'source_id': item['source_id'], 'device_id': route['device_id'],
                                'existing_device': dict(row) if row else None, 'source_id_absent': True,
                                'existing_model_case_only_difference': bool(row and row['model'] != route['model']),
                                'identity_rule': 'Existing Catalog brand namespace and model casefold/strip; printed case retained in each record, full-name identifier/fact/raw evidence; no new alias.',
                                'existing_source_ids': [r[0] for r in connection.execute('SELECT source_id FROM records WHERE device_id=? ORDER BY source_id', (route['device_id'],))]})
            assert sha(path) == before
            contexts.append({'catalog_path': filename, 'observed_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                             'catalog_sha256_before': before, 'catalog_sha256_after': sha(path),
                             'read_only': True, 'anchors': anchors})
        finally:
            connection.close()
    save(HERE / 'candidate.json', {'sources': sources, 'devices': devices, 'status': 'staged; independent candidate review required', 'scope_limitations': LIMITS}, args.check)
    save(HERE / 'field-mapping.json', fieldmaps, args.check)
    save(HERE / 'provenance.json', {'artifacts': artifacts, 'observations_sha256': OBS_SHA, 'requests_sha256': REQ_SHA,
                                  'original_observations': str((ORIGINALS / 'observations.json').relative_to(ROOT)),
                                  'original_requests': str((ORIGINALS / 'requests.jsonl').relative_to(ROOT)),
                                  'new_requests': 0, 'historical_rules': 'Accepted SPEC revision3, waves9/10; historical builders read only, never executed.'}, args.check)
    if not args.check:
        save(HERE / 'routing.json', {'catalog_observations': contexts, 'scope': 'Timestamped read-only observations, not a permanent baseline; root alone integrates.'}, False)
    print('PASS', len(sources), 'sources /', len(devices), 'records /', sum(len(d['facts']) for d in devices), 'facts /', len({m['mapping']['device_id'] for m in fieldmaps}), 'exact identities; no import')


if __name__ == '__main__':
    main()
