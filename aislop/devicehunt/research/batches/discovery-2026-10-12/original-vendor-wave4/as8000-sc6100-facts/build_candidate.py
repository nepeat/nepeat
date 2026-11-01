"""Stage exactly the 26 reviewed observations; never import or use the network."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import re
import sqlite3

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
ORIGINALS = HERE.parent / 'as8000-sc6100'
FINDINGS_SHA = '397420189911c5991318be6c04cc9698bd65970cb14800112f0301300fc9e06e'
PINS = {
    'as8000-r02-qsg.pdf': '0cc5631b82ffd5e57c2750baea33f4203a1714d633bcccb73f6ebd4ff473fdcd',
    'as8000-pages.json': 'd826784d7e402dc7f9e74df449d26231cd4373a5ceae11421d2f24d79a936d4b',
    'sc6100-datasheet.pdf': '5dc67c8d2976c3dfe36ef813b919d0000f0b5f6bf1eb034d04040cae3254ddba',
    'sc6100-pages.json': '973cb631b49945513bca216f0de25aa48e65728023078a03fa6410acff1c2271',
    'requests.json': '1f8b82c7c790e607c75ca070f194a9c56b1aa137291cc68a2d911451b31b383b',
    'render-provenance.json': 'e5c3ab8297e6e9186f1e14aca9986bdd16527bbfff73afdc1881193654e4455d',
}
LIMITS = ('Only the 26 independently reviewed vendor-document observations, source-scoped and conditioned as printed. '
          'Documentation-vendor affiliations only; no manufacturer, factory, hyperscaler or deployment claim. '
          'No bridge or alias between brands, product-index long names, order-code suffixes, chassis, SCM, PIM, '
          'FAN or rail identities. No current support, stock, installed auction-unit configuration, PSU-model '
          'fitment or safe wiring/standalone-operation inference. Complete extraction is not whole-document visual review.')
NAMES = {
    'model_revision': 'document_revision_as_printed',
    'rack_rails': 'physical_description',
    'installation_restrictions': 'physical_description',
    'chassis_mounting_and_air_baffles': 'physical_description',
    'psu_quantity_and_redundancy': 'power_description',
    'power_circuit_constraint': 'power_description',
    'management_module_scope': 'management_module_scope_as_printed',
    'module_service_constraint': 'module_service_constraint_as_printed',
    'fan_tray_service': 'cooling_description',
    'failed_psu_service_limit': 'power_description',
    'dimensions_configured_weight': 'physical_description',
    'environmental_conditions': 'physical_description',
    'maximum_power_consumption': 'power_description',
    'system_input_rating_lines': 'power_description',
    'model_chassis_nodes': 'physical_description',
    'storage_capacity_scope': 'storage_capacity_scope_as_printed',
    'node_management_interfaces': 'node_management_interfaces_as_printed',
    'bbu_option': 'power_description',
    'psu_configuration': 'power_description',
    'fan_module_condition': 'cooling_description',
    'physical_dimensions': 'physical_description',
    'operating_environment': 'physical_description',
    'non_operating_environment': 'physical_description',
    'sound_power': 'cooling_description',
    'application_capacity_condition': 'application_capacity_condition_as_printed',
    'revision': 'document_revision_as_printed',
}
ROUTES = [
    {'brand': 'Edge-Core', 'model': 'Minipack-AS8000', 'kind': 'chassis switch',
     'source_id': 'edgecore-as8000-r02-practical-facts-2026-10-12',
     'company': 'Edgecore Networks Corporation', 'pdf': 'as8000-r02-qsg.pdf',
     'sidecar': 'as8000-pages.json', 'page_count': 12, 'visual_pages': [1, 2, 3, 4],
     'fact_count': 14, 'existing_id': None},
    {'brand': 'Celestica', 'model': 'SC6100', 'kind': 'storage controller',
     'source_id': 'celestica-sc6100-datasheet-practical-facts-2026-10-12',
     'company': 'Celestica Inc.', 'pdf': 'sc6100-datasheet.pdf',
     'sidecar': 'sc6100-pages.json', 'page_count': 2, 'visual_pages': [1, 2],
     'fact_count': 12, 'existing_id': '28d8c44c9c82eff65231'},
]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + '\n').encode()


def identity(brand, model):
    normalized = re.sub(r'[^a-z0-9]', '', brand.lower())
    return hashlib.sha256(json.dumps([normalized, model.casefold().strip(), '']).encode()).hexdigest()[:20]


def write_once_or_equal(path, data, check):
    if check or path.exists():
        assert path.read_bytes() == data, 'staged bytes differ: ' + str(path)
    else:
        with path.open('xb') as handle:
            handle.write(data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='verify staged bytes without writing')
    args = parser.parse_args()
    from devicehunt.documents import read_capture
    assert sha(ORIGINALS / 'findings.json') == FINDINGS_SHA
    for name, expected in PINS.items():
        assert sha(ORIGINALS / name) == expected, name
    findings = json.loads((ORIGINALS / 'findings.json').read_text())
    ledger = json.loads((ORIGINALS / 'requests.json').read_text())
    renders = json.loads((ORIGINALS / 'render-provenance.json').read_text())
    assert len(findings['sources']) == 2 and len(ledger) == 4
    for image in renders['images']:
        assert sha(ORIGINALS / image['path']) == image['sha256']
    assert len(renders['images']) == 6
    live = ROOT / 'data/devices.sqlite'
    before = sha(live)
    connection = sqlite3.connect(live.resolve().as_uri() + '?mode=ro', uri=True)
    connection.row_factory = sqlite3.Row
    sources, devices, artifacts, anchors = [], [], [], []
    try:
        connection.execute('BEGIN')
        neighbors = [dict(row) for row in connection.execute(
            "SELECT * FROM devices WHERE lower(model) LIKE '%as8000%' OR (brand='Celestica' AND model='SC6100') ORDER BY brand,model")]
        for route, observed in zip(ROUTES, findings['sources'], strict=True):
            assert observed['model_as_printed'] == route['model'] and len(observed['facts']) == route['fact_count']
            did = identity(route['brand'], route['model'])
            existing = connection.execute('SELECT * FROM devices WHERE id=?', (did,)).fetchone()
            if route['existing_id']:
                assert did == route['existing_id'] and existing
                assert existing['brand'] == route['brand'] and existing['model'] == route['model']
            else:
                assert existing is None
                assert not connection.execute('SELECT 1 FROM devices WHERE brand=? AND model=?', (route['brand'], route['model'])).fetchone()
            assert not connection.execute('SELECT 1 FROM sources WHERE id=?', (route['source_id'],)).fetchone()
            anchors.append({'brand': route['brand'], 'model': route['model'], 'device_id': did,
                            'routing': 'existing exact identity' if existing else 'proposed new exact system identity',
                            'existing_device': dict(existing) if existing else None,
                            'existing_source_ids': [r[0] for r in connection.execute(
                                'SELECT source_id FROM records WHERE device_id=? ORDER BY source_id', (did,))],
                            'proposed_source_id_absent': True})
            request = next(r for r in ledger if r['body_path'] == route['pdf'])
            assert request['status'] == 200 and request['sha256'] == observed['pdf_sha256'] == PINS[route['pdf']]
            pages, metadata = read_capture((ORIGINALS / route['sidecar']).read_bytes(), request['sha256'])
            assert len(pages) == route['page_count']
            assert [p['page'] for p in pages] == list(range(1, route['page_count'] + 1))
            assert route['model'] in pages[0]['text']
            for name in (route['pdf'], route['sidecar']):
                target = HERE / 'snapshots' / name
                if not args.check:
                    target.parent.mkdir(exist_ok=True)
                write_once_or_equal(target, (ORIGINALS / name).read_bytes(), args.check)
                artifacts.append({'snapshot': target.relative_to(HERE).as_posix(),
                                  'original': (ORIGINALS / name).relative_to(ROOT).as_posix(),
                                  'sha256': PINS[name], 'bytes': (ORIGINALS / name).stat().st_size,
                                  'type': 'raw PDF capture' if name.endswith('.pdf') else 'complete Poppler text sidecar; no OCR'})
            exclusions = {k: observed.get(k, []) for k in ('absences', 'scope_limits', 'extraction_quality', 'conflicts')}
            sources.append({'id': route['source_id'], 'url': request['requested_url'], 'tier': 'vendor',
                            'license': 'Public factual documentation; upstream rights retained.',
                            'coverage': LIMITS + ' ' + ' '.join(v for values in exclusions.values() for v in values),
                            'snapshot': 'snapshots/' + route['pdf'], 'original_sha256': request['sha256'],
                            'text_sidecar': 'snapshots/' + route['sidecar'], 'captured_at': request['finished_at_utc'],
                            'extraction_metadata': metadata})
            facts, evidence = [], []
            for observation in observed['facts']:
                page = observation['pdf_page']
                assert observation['visually_reviewed'] is True and page in route['visual_pages']
                name = NAMES[observation['name']]
                value = observation['value'] + ' [PDF p. ' + str(page) + ']'
                facts.append([name, value])
                image_name = 'as8000-%02d.png' % page if route['brand'] == 'Edge-Core' else 'sc6100-%d.png' % page
                image = next(i for i in renders['images'] if i['path'] == image_name)
                evidence.append({'fact_index': len(facts) - 1, 'selected_observation': observation,
                                 'pdf_page': page, 'pdf_sha256': request['sha256'],
                                 'sidecar_sha256': PINS[route['sidecar']],
                                 'raw_page_text': pages[page - 1]['text'],
                                 'visual_original': (ORIGINALS / image_name).relative_to(ROOT).as_posix(),
                                 'visual_sha256': image['sha256']})
            devices.append({'source': route['source_id'], 'key': route['model'], 'brand': route['brand'],
                            'model': route['model'], 'title': route['brand'] + ' ' + route['model'] + ' source-scoped document statements',
                            'kind': existing['kind'] if existing else route['kind'],
                            'affiliations': [[route['company'], 'documentation_vendor']], 'identifiers': [],
                            'facts': facts, 'identity_routing': anchors[-1], 'evidence_per_observation': evidence,
                            'source_exclusions': exclusions, 'scope_limitations': LIMITS})
    finally:
        connection.close()
    assert sha(live) == before, 'live catalog changed during read-only staging'
    candidate = {'sources': sources, 'devices': devices, 'status': 'staged; independent candidate review required',
                 'scope_limitations': LIMITS, 'identifiers_policy': 'empty; no aliases or code bridges',
                 'reference_document_scope': 'Both complete PDFs retained: 12 AS8000 pages and 2 SC6100 pages. Only AS8000 pages 1–4 and SC6100 pages 1–2 visually reviewed; AS8000 page-12 garbling preserved and unused.'}
    assert len(sources) == len(devices) == 2 and sum(len(d['facts']) for d in devices) == 26
    provenance = {'artifacts': artifacts, 'reviewed_findings_sha256': FINDINGS_SHA,
                  'original_request_ledger': str((ORIGINALS / 'requests.json').relative_to(ROOT)),
                  'original_request_ledger_sha256': PINS['requests.json'],
                  'original_render_provenance': str((ORIGINALS / 'render-provenance.json').relative_to(ROOT)),
                  'original_render_provenance_sha256': PINS['render-provenance.json'],
                  'visual_originals': renders['images'], 'complete_pages': 14, 'visual_pages': 6,
                  'new_network_requests': 0, 'new_extraction_or_rendering': False}
    write_once_or_equal(HERE / 'candidate.json', encoded(candidate), args.check)
    write_once_or_equal(HERE / 'provenance.json', encoded(provenance), args.check)
    if not args.check:
        context = {'observed_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                   'catalog_path': 'data/devices.sqlite', 'catalog_sha256_before': before,
                   'catalog_sha256_after': sha(live), 'read_only': True,
                   'anchors': anchors, 'nearby_identities_kept_separate': neighbors,
                   'no_bridge_policy': 'Exact brand/model routing only; no Accton, longname, suffix, chassis/module or rail alias.'}
        write_once_or_equal(HERE / 'routing.json', encoded(context), False)
    else:
        saved = json.loads((HERE / 'routing.json').read_text())
        assert saved['anchors'] == anchors
    print('PASS: exact 2 sources / 2 records / 26 observations; 14 complete pages / 6 visual pages; live unchanged; no import')


if __name__ == '__main__':
    main()
