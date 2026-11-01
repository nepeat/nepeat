"""Two reviewed family records, source-scoped facts only; no imports or network."""
import datetime
import hashlib
import json
from pathlib import Path
import shutil
import sqlite3

HERE = Path(__file__).resolve().parent
ORIGINALS = HERE.parent / 'psu-edge'
ROOT = HERE.parents[4]
LIMITS = ('Exact existing family namespace only; all statements belong to this source record and '
          'its printed document conditions. No manufacturer/factory affiliation is transferred from '
          'other records or generic about-company prose. Ordering rows are literal discovery facts, '
          'not aliases, part-number identifiers, standalone FRUs or new device identities. '
          'No current support/certification, software entitlement, installed auction-unit components, '
          'safe wiring/standalone-operation or individual-order-code power/airflow fitment inferred.')
NAMES = {
    'model_heading': 'model_heading_as_printed',
    'drive_and_power_scope': 'power_description',
    'esm_configuration': 'esm_configuration_as_printed',
    'esm_ports': 'esm_interfaces_as_printed',
    'psu_redundancy_as_printed': 'power_description',
    'input_voltage_as_printed': 'power_description',
    'fan_description': 'cooling_description',
    'dimensions': 'physical_description',
    'environmental_conditions': 'physical_description',
    'sound_power_as_printed': 'cooling_description',
    'revision_and_heading_conflict': 'document_revision_and_conflict',
    'psu_and_input': 'power_description',
    'power_input_option': 'power_description',
    'typical_power_consumption': 'power_description',
    'dimensions_and_weight': 'physical_description',
    'fan_airflow': 'cooling_description',
    'environmental': 'physical_description',
    'software_as_printed': 'software_installation_description',
    'revision': 'document_revision_as_printed',
}
ROUTING = [
    {'brand': 'Celestica', 'model': 'SD6200', 'device_id': 'd1daa8e8c3ad705b1118',
     'source_id': 'celestica-sd6200-datasheet-2026-10-12',
     'affiliations': [['Celestica Inc.', 'documentation_vendor']],
     'snapshot': 'sd6200-datasheet.pdf', 'sidecar': 'sd6200-pages.json'},
    {'brand': 'Edge-Core', 'model': 'Wedge 100-32X', 'device_id': '9287d01bdc56adda4517',
     'source_id': 'edgecore-wedge100-32x-r04-2026-10-12',
     'affiliations': [['Edgecore Networks Corporation', 'documentation_vendor']],
     'snapshot': 'wedge100-32x-r04.pdf', 'sidecar': 'wedge100-32x-r04-pages.json'},
]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    from devicehunt.documents import read_capture
    findings = json.loads((ORIGINALS / 'findings.json').read_text())
    assert sha(ORIGINALS / 'findings.json') == '437ac50ef27031bb6373d3005090d3678ba6f8667901b77d6c1923bc20f34211'
    ledger = json.loads((ORIGINALS / 'requests.json').read_text())
    sources, devices, provenance = [], [], []
    (HERE / 'snapshots').mkdir(exist_ok=True)
    db_path = ROOT / 'data/devices.sqlite'
    connection = sqlite3.connect(db_path.resolve().as_uri() + '?mode=ro', uri=True)
    connection.row_factory = sqlite3.Row
    connection.execute('BEGIN')
    query_started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    anchors = []
    try:
        for route, observed in zip(ROUTING, findings['sources']):
            device = connection.execute('SELECT * FROM devices WHERE id=?', (route['device_id'],)).fetchone()
            assert device and device['brand'] == route['brand'] and device['model'] == route['model']
            assert not connection.execute('SELECT 1 FROM sources WHERE id=?', (route['source_id'],)).fetchone()
            anchors.append({'device': dict(device), 'source_ids': [r[0] for r in connection.execute(
                'SELECT source_id FROM records WHERE device_id=? ORDER BY source_id', (route['device_id'],))]})
            request = next(r for r in ledger if r['body_path'] == route['snapshot'])
            assert request['status'] == 200 and observed['pdf_sha256'] == sha(ORIGINALS / route['snapshot'])
            pdf_sha = observed['pdf_sha256']
            pages, metadata = read_capture((ORIGINALS / route['sidecar']).read_bytes(), pdf_sha)
            assert route['model'] in pages[0]['text'], 'Keep the exact source heading spelling'
            for file in (route['snapshot'], route['sidecar']):
                copied = HERE / 'snapshots' / file
                if copied.exists():
                    assert copied.read_bytes() == (ORIGINALS / file).read_bytes()
                else:
                    shutil.copy2(ORIGINALS / file, copied)
                provenance.append({'snapshot': copied.relative_to(HERE).as_posix(),
                                   'original': str((ORIGINALS / file).relative_to(ROOT)),
                                   'sha256': sha(copied), 'bytes': copied.stat().st_size})
            sources.append({'id': route['source_id'], 'url': request['requested_url'],
                            'tier': 'vendor', 'license': 'Public factual documentation; upstream rights retained.',
                            'coverage': LIMITS + ' ' + ' '.join(observed['absences'] + observed['conflicts']),
                            'snapshot': 'snapshots/' + route['snapshot'], 'original_sha256': pdf_sha,
                            'text_sidecar': 'snapshots/' + route['sidecar'],
                            'captured_at': request['finished_at_utc'], 'extraction_metadata': metadata})
            facts = []
            for item in observed['facts']:
                value = item['value']
                page = str(item['pdf_page'])
                if item['name'] == 'software_as_printed':
                    value = ('Historical EC-DS-0617-04 document statement (©2016 footer; publication date '
                             'not independently verified): ' + value)
                    page = '2–3'
                elif item['name'] == 'revision':
                    page = '2–3'
                facts.append([NAMES[item['name']], value + ' [PDF p. ' + page + ']'])
            if route['brand'] == 'Edge-Core':
                for order in observed['ordering_rows']:
                    assert order['code'] in pages[2]['text'] and order['description'] in pages[2]['text']
                    facts.append(['ordering_row_as_printed', json.dumps(
                        {'code': order['code'], 'description': order['description'], 'classification': 'unresolved; literal family ordering-table row only'},
                        ensure_ascii=False, sort_keys=True) + ' [PDF p. 3]'])
                facts.append(['power_option_source_conflict',
                              'Page 1 says hot-swappable, load-sharing, redundant AC PSUs or 12 V DC PSUs; '
                              'page 2 separately prints AC or -54 VDC PSUs, 40–72 VDC input and 12 VDC input option. '
                              'Option statements stay separate; no wiring or per-SKU rating inferred. [PDF pp. 1–2]'])
                facts.append(['company_context_as_printed',
                              'About-company paragraph describes Edgecore Networks Corporation as a subsidiary '
                              'of Accton Technology Corporation and says Edgecore Data Center switches are developed '
                              'and manufactured by Accton. Generic context only; no new per-model manufacturer/factory '
                              'affiliation or physical provenance inferred. [PDF p. 2]'])
            devices.append({'source': route['source_id'], 'key': route['model'], 'brand': route['brand'],
                            'model': route['model'], 'title': route['brand'] + ' ' + route['model'] + ' datasheet statements',
                            'kind': device['kind'], 'affiliations': route['affiliations'], 'identifiers': [],
                            'facts': facts, 'existing_identity_id': route['device_id'], 'scope_limitations': LIMITS})
    finally:
        connection.close()
    candidate = {'sources': sources, 'devices': devices, 'status': 'staged; independent candidate review required',
                 'scope_limitations': LIMITS}
    (HERE / 'candidate.json').write_text(json.dumps(candidate, ensure_ascii=False, indent=2) + '\n')
    (HERE / 'provenance.json').write_text(json.dumps({'artifacts': provenance, 'reviewed_findings_sha256': sha(ORIGINALS / 'findings.json')}, indent=2) + '\n')
    before = db_path.stat()
    hash_started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    database_sha = sha(db_path)
    after = db_path.stat()
    context = {'query_started_utc': query_started, 'hash_started_utc': hash_started,
               'hash_finished_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
               'catalog_hash_observed': database_sha,
               'catalog_file_stat_stable_while_hashing': (before.st_ino, before.st_size, before.st_mtime_ns) == (after.st_ino, after.st_size, after.st_mtime_ns),
               'scope': 'timestamped read-only observations; coordinating integration may change live state afterward',
               'anchors': anchors}
    (HERE / 'catalog-context.json').write_text(json.dumps(context, ensure_ascii=False, indent=2) + '\n')
    print('staged', len(sources), 'sources /', len(devices), 'existing identities /', sum(len(r['facts']) for r in devices), 'facts; no import')


if __name__ == '__main__':
    main()
