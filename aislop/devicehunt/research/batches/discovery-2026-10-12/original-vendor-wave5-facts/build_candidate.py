"""Finite wave5 facts: five printed bases and 35 literal hardware codes; no import."""
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
from devicehunt.documents import read_capture

ORIGINALS = HERE.parent / 'original-vendor-wave5'
PINS = {
    'findings.json': '95838a2586022a9b1e3dab7b00a3d82f4102a0a41fef0e444d8fb0208ae3a1f0',
    'handoff.md': '91a32cc775d0caad30fc43c0cc5c1c297b3d80e0ba2c9f43677ebc5293b250a5',
    'checks.json': '1fd1d17996c3814b1650859a679b5d685eb09dd4c14ebf2e04f3d94c624c4c67',
}
REVIEW_PINS = {
    'original-vendor-wave5-review.md': '8a86e01a701509845ec8b60cefd5576b100eb3e9c5494668506afef5c9d96383',
    'wave5-candidate-boundary-note.md': 'bdb63a61400bd84137b4e793a121aa42426fc629a3cad072fea67a06693cedeb',
}
GROUPS = [
    {'file': 'oracle7420-specifications.html', 'sid': 'oracle-sun-zfs-storage-7420-specifications-2026-10-12',
     'brand': 'Oracle', 'company': 'Oracle Corporation', 'bases': ['Sun ZFS Storage 7420'], 'kind': 'storage controller'},
    {'file': 'as6700-quick-start.pdf', 'sidecar': 'as6700-pages.json',
     'sid': 'edgecore-as6700-as6701-qsg-2026-10-12', 'brand': 'Edge-Core',
     'company': 'Edgecore Networks Corporation', 'bases': ['AS6700-32X', 'AS6701-32X'], 'kind': 'network switch'},
    {'file': 'as5710-as5712-ds-r09.pdf', 'sidecar': 'as5712-pages.json',
     'sid': 'edgecore-as5710-as5712-ds-r09-2026-10-12', 'brand': 'Edge-Core',
     'company': 'Edgecore Networks Corporation', 'bases': ['AS5710-54X', 'AS5712-54X'], 'kind': 'network switch'},
]
LIMITS = ('Source-scoped historical document statements only. Documentation-vendor affiliation only; no '
          'manufacturer, factory, OEM, deployment, operator or hyperscaler inference. Printed base names and '
          'full ordering codes remain separate identities; no aliases, added AS prefix, suffix/wildcard expansion '
          'or merge with Unattributed accessory. No current certification/support/stock, image entitlement, '
          'individual-unit configuration, universal fitment or safe wiring/standalone-operation promise. '
          'Ordering records receive only their own literal row cells and explicit heading context; no family '
          'weight, electrical limit or software compatibility is cloned by prefix. Preserve source contradictions.')
CATEGORIES = {'identity_literal': 'model_heading_as_printed', 'physical': 'physical_description',
              'cooling': 'cooling_description', 'power': 'power_description',
              'environment': 'physical_description', 'software': 'software_installation_description',
              'service_constraint': 'service_constraint_as_printed', 'revision_literal': 'document_revision_as_printed',
              'model_configuration': 'model_configuration_as_printed',
              'physical_cooling_environment': 'physical_description',
              'literal_company_context': 'company_context_as_printed', 'rating_scope_note': 'power_rating_scope_note_as_printed'}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + '\n').encode()


def save(path, body, check):
    if check or path.exists():
        assert path.read_bytes() == body, str(path)
    else:
        with path.open('xb') as handle:
            handle.write(body)


def identity(brand, model):
    norm = re.sub(r'[^a-z0-9]', '', brand.lower())
    return hashlib.sha256(json.dumps([norm, model.casefold().strip(), '']).encode()).hexdigest()[:20]


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--check', action='store_true', help='verify frozen bytes and routing without writes')
    args = ap.parse_args()
    for name, expected in PINS.items():
        assert sha(ORIGINALS / name) == expected
    for name, expected in REVIEW_PINS.items():
        assert sha(HERE.parent / 'review' / name) == expected
    findings = json.loads((ORIGINALS / 'findings.json').read_text())
    assert len(findings['observations']) == 36 and len(findings['ordering_rows']) == 38
    renders = json.loads((ORIGINALS / 'render-provenance.json').read_text())
    images = {i['path']: i['sha256'] for run in renders['renders'] for i in run['images']}
    for image, expected in images.items():
        assert sha(ORIGINALS / image) == expected
    excluded_rows = [r for r in findings['ordering_rows'] if r.get('model_text_render_conflict') or r['raw_cells_from_embedded_text'][0] == '57xx-54X-SVC-RTF']
    assert len(excluded_rows) == 3
    sources, records, copies, selections = [], [], [], []
    page_lines = {}
    for group in GROUPS:
        file = group['file']
        source = findings['sources'][file]
        assert sha(ORIGINALS / file) == source['sha256']
        metadata = None
        if group.get('sidecar'):
            pages, metadata = read_capture((ORIGINALS / group['sidecar']).read_bytes(), source['sha256'])
            page_lines[file] = {p['page']: p['text'].splitlines() for p in pages}
        for name in (file, group['sidecar']) if group.get('sidecar') else (file,):
            original = ORIGINALS / name
            target = HERE / 'snapshots' / name
            if not args.check:
                target.parent.mkdir(exist_ok=True)
            save(target, original.read_bytes(), args.check)
            if not args.check:
                st = original.stat()
                os.utime(target, ns=(st.st_atime_ns, st.st_mtime_ns))
            copies.append({'snapshot': target.relative_to(HERE).as_posix(), 'original': original.relative_to(ROOT).as_posix(),
                           'sha256': sha(original), 'bytes': original.stat().st_size,
                           'type': 'raw PDF capture' if name.endswith('.pdf') else 'complete Poppler text sidecar' if name.endswith('.json') else 'raw HTML capture',
                           'original_mtime_ns': original.stat().st_mtime_ns})
        specific = [c for c in findings['conflicts'] if c['source'] == file or (file == 'as6700-quick-start.pdf' and 'product-as6700' in c['source'])]
        negatives = [n for n in findings['negative_findings'] if n['source'] == file]
        coverage = LIMITS + ' ' + ' '.join(c['issue'] for c in specific) + ' ' + ' '.join(n['absent'] for n in negatives)
        sources.append({'id': group['sid'], 'url': source['url'], 'tier': 'vendor',
                        'license': 'Public factual vendor documentation; original upstream rights retained; local research only.',
                        'coverage': coverage, 'snapshot': 'snapshots/' + file, 'original_sha256': source['sha256'],
                        'captured_at_utc': source['captured_at_utc'],
                        **({'text_sidecar': 'snapshots/' + group['sidecar'], 'extraction_metadata': metadata} if metadata else {})})

        def new_record(model, kind, record_scope):
            return {'source': group['sid'], 'key': model, 'brand': group['brand'], 'model': model,
                    'title': group['brand'] + ' ' + model, 'kind': kind,
                    'affiliations': [[group['company'], 'documentation_vendor']], 'identifiers': [],
                    'hyperscaler': [], 'facts': [], 'evidence': [], 'record_scope': record_scope,
                    'scope_limitations': LIMITS, 'source_conflicts': specific, 'source_absences': negatives}

        def append(record, name, value, raw, locator, extra=None):
            index = len(record['facts'])
            record['facts'].append([name, value])
            evidence = {'fact_index': index, 'fact_name': name, 'source_url': source['url'],
                        'capture_sha256': source['sha256'], 'literal_quote': raw,
                        'locator': locator, 'selected_scope': record['record_scope']}
            if group.get('sidecar'):
                evidence['complete_sidecar_sha256'] = sha(ORIGINALS / group['sidecar'])
            if extra:
                evidence.update(extra)
            record['evidence'].append(evidence)

        base_observations = [o for o in findings['observations'] if o['source'] == file]
        for model in group['bases']:
            record = new_record(model, group['kind'], 'Printed base-model document scope; joint statements apply only where this source explicitly names both bases.')
            record['original_discovery_observations'] = []
            for observed in base_observations:
                if observed['id'] == 'as5712-21' and model != 'AS5712-54X':
                    continue
                page = observed.get('pdf_page')
                locator = {'pdf_page': page, 'lines_1based': observed['lines_1based']} if page else observed['locator']
                raw = '\n'.join(observed['raw_lines']) if page else observed.get('raw_block', observed.get('raw_text'))
                value = observed['summary']
                fragment = None
                if observed['id'] == 'as5712-22':
                    # The reviewed block has two explicit named CPU/memory
                    # groups in its left column; don't transfer the other group.
                    start = 0 if model == 'AS5710-54X' else 5
                    selected_lines = observed['raw_lines'][start:start + 5]
                    fragment = [re.split(r'\s{2,}', line.strip())[0] for line in selected_lines]
                    assert fragment[0].startswith(model + ':')
                    value = '\n'.join(fragment)
                    locator = {'pdf_page': page, 'lines_1based': observed['lines_1based'][start:start + 5], 'column_scope': 'left model-specific CPU/memory/storage group'}
                    raw = '\n'.join(selected_lines)
                suffix = ' [PDF p. ' + str(page) + ']' if page else ' [Original HTML article; ' + model + ' scope]'
                extra = {'original_observation_id': observed['id'], 'original_discovery_observation': observed,
                         'value_method': 'Reviewed scoped summary' if fragment is None else 'Exact model-specific left-column fragments; whitespace separation only'}
                if page:
                    extra.update(visual_original=(ORIGINALS / observed['visual_page']).relative_to(ROOT).as_posix(),
                                 visual_sha256=images[observed['visual_page']])
                if observed['id'] in ('oracle-33', 'oracle-34', 'oracle-35'):
                    note = next(o for o in base_observations if o['id'] == 'oracle-rating-note')
                    extra['mandatory_qualifying_note'] = note['raw_text']
                    if observed['id'] == 'oracle-35':
                        value += ' Power-dissipation note describes maximum PSU rating, not actual appliance consumption.'
                append(record, CATEGORIES[observed['category']], value + suffix, raw, locator, extra)
                record['original_discovery_observations'].append(observed)
                selections.append({'record_key': model, 'source': group['sid'], 'original_observation_id': observed['id'],
                                   'fact_index': len(record['facts']) - 1, 'scope_split': fragment})
            if file == 'as5710-as5712-ds-r09.pdf':
                record['excluded_ordering_rows'] = excluded_rows
                record['excluded_row_policy'] = 'Service wildcard and unresolved embedded/render fan spellings retained raw; no hardware identity, alias or fitment inferred.'
            records.append(record)

        if file != 'as5710-as5712-ds-r09.pdf':
            continue
        for row in findings['ordering_rows']:
            if row in excluded_rows:
                continue
            cells = row['raw_cells_from_embedded_text']
            code = cells[0]
            if row['row_scope'] == 'family_configuration_order_row':
                kind = 'switch configuration'
            elif code == 'PSU-12V-400-1':
                kind = 'power input unit FRU'
            elif code.startswith('PSU-'):
                kind = 'power supply FRU'
            elif code == 'RKIT-85-6712':
                kind = 'rack mount kit'
            elif code == 'ORSA-1U':
                kind = 'rack adapter'
            else:
                assert code.startswith('CBL-PWR-')
                kind = 'power cable'
            record = new_record(code, kind, 'Own exact printed ordering row and explicit heading context only; source-listed descriptions, not general physical fitment or unit inventory.')
            record['identifiers'] = [['part_number', code]]
            record['original_discovery_ordering_row'] = row
            locator = {'pdf_page': row['pdf_page'], 'lines_1based': [row['line_1based']], 'row_scope': row['row_scope']}
            extra = {'original_discovery_ordering_row': row,
                     'visual_original': (ORIGINALS / row['visual_page']).relative_to(ROOT).as_posix(),
                     'visual_sha256': images[row['visual_page']], 'value_method': 'Own literal row/cell only'}
            citation = ' [PDF p. ' + str(row['pdf_page']) + ']'
            append(record, 'ordering_row_as_printed', row['raw_line'].strip() + citation, row['raw_line'], locator, extra)
            if row['row_scope'] == 'family_configuration_order_row':
                assert len(cells) in (4, 5) and not code.startswith('AS')
                heading = row['base_model_line_as_printed']
                heading_line = next(i for i, line in enumerate(page_lines[file][3], 1) if line.strip() == heading)
                append(record, 'base_model_heading_as_printed', heading + citation, heading,
                       {'pdf_page': 3, 'lines_1based': [heading_line]}, {'value_method': 'Literal table heading, not an alias or family-spec transfer'})
                for column, name in ((1, 'cpu_module_as_printed'), (2, 'power_description'), (3, 'cooling_description')):
                    append(record, name, cells[column] + citation, row['raw_line'], {**locator, 'column': row['table_headers_as_embedded_text'][column]}, extra)
                if len(cells) == 5:
                    append(record, 'power_cord_region_as_printed', cells[4] + citation, row['raw_line'], {**locator, 'column': row['table_headers_as_embedded_text'][4]}, extra)
            else:
                category = 'physical_description' if code in ('RKIT-85-6712', 'ORSA-1U') else 'power_description'
                append(record, category, cells[1] + citation, row['raw_line'], {**locator, 'column': row['table_headers_as_embedded_text'][1]}, extra)
                if code.startswith('PSU-') and len(cells) == 4:
                    append(record, 'cooling_description', cells[2] + citation, row['raw_line'], {**locator, 'column': 'Airflow'}, extra)
                    append(record, 'power_cord_scope_as_printed', cells[3] + citation, row['raw_line'], {**locator, 'column': 'Region (power cord)'}, extra)
                if code == 'PSU-12V-400-1':
                    record['specific_absence'] = 'Description says12VDC power input unit FRU;400 in the code is not an output rating. Airflow/power-cord cells blank.'
            records.append(record)
    assert len(sources) == 3 and len(records) == 40
    assert len({(r['source'], r['key']) for r in records}) == 40
    assert {s['original_observation_id'] for s in selections} == {o['id'] for o in findings['observations']}
    assert not any(r['model'] in {row['raw_cells_from_embedded_text'][0] for row in excluded_rows} for r in records)
    contexts = []
    for filename in ('data/devices.sqlite', 'data/devices-candidate-wave4-2026-10-12.sqlite'):
        path = ROOT / filename
        before = sha(path)
        connection = sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True)
        connection.row_factory = sqlite3.Row
        try:
            connection.execute('BEGIN')
            for source in sources:
                assert not connection.execute('SELECT 1 FROM sources WHERE id=?', (source['id'],)).fetchone()
            anchors = []
            for record in records:
                did = identity(record['brand'], record['model'])
                existing = connection.execute('SELECT * FROM devices WHERE id=?', (did,)).fetchone()
                if existing:
                    assert existing['brand'] == record['brand'] and existing['model'].casefold().strip() == record['model'].casefold().strip()
                others = [dict(r) for r in connection.execute('SELECT id,brand,model,kind FROM devices WHERE model=? COLLATE NOCASE AND brand<>? ORDER BY brand', (record['model'], record['brand']))]
                anchors.append({'source': record['source'], 'key': record['key'], 'device_id': did,
                                'existing_same_namespace': dict(existing) if existing else None,
                                'other_namespace_matches_kept_separate': others})
            assert sha(path) == before
            contexts.append({'catalog_path': filename, 'observed_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                             'catalog_sha256_before': before, 'catalog_sha256_after': sha(path),
                             'read_only': True, 'all3source_ids_absent': True, 'anchors': anchors})
        finally:
            connection.close()
    candidate = {'sources': sources, 'devices': records, 'status': 'staged; independent candidate review required',
                 'scope_limitations': LIMITS, 'excluded_ordering_rows': excluded_rows,
                 'excluded_row_policy': 'Three original rows retained, not hardware identities/aliases: one57xx service pattern and two unresolved FAN/AN rows.',
                 'complete_document_scope': 'Both raw PDFs plus complete2+4-page sidecars and original Oracle HTML retained; all six PDF pages visually reviewed at discovery, with one additional page4 rerender.'}
    save(HERE / 'candidate.json', encoded(candidate), args.check)
    save(HERE / 'mapping.json', encoded({'base_observation_selections': selections, 'ordering_records': [r['model'] for r in records if 'original_discovery_ordering_row' in r], 'excluded_rows': excluded_rows}), args.check)
    save(HERE / 'provenance.json', encoded({'copies': copies, 'findings_sha256': PINS['findings.json'],
                                          'accepted_review_and_boundary': REVIEW_PINS, 'requests_sha256': sha(ORIGINALS / 'requests.jsonl'),
                                          'render_provenance_sha256': sha(ORIGINALS / 'render-provenance.json'),
                                          'original_root': ORIGINALS.relative_to(ROOT).as_posix(), 'source_conflicts': findings['conflicts'],
                                          'source_absences': findings['negative_findings'], 'original_limits': findings['limits'],
                                          'new_network_requests': 0, 'new_extraction_or_rendering': False}), args.check)
    if not args.check:
        save(HERE / 'routing.json', encoded({'catalog_observations': contexts, 'scope': 'Timestamped read-only observations, not a permanent baseline; no other-brand merge or alias.'}), False)
    print('PASS', len(sources), 'sources /', len(records), 'records /', sum(len(r['facts']) for r in records), 'facts; no import')


if __name__ == '__main__':
    main()
