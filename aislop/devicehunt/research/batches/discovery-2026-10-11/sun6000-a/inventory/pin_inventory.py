#!/usr/bin/env python3
"""Finite offline source pinning. stdlib only; no import, capture, or DB writes.

build writes only --output; verify is read-only; self-test writes tiny /tmp fixtures.
Canonical hashes use UTF-8, sorted keys, compact separators, no trailing newline.
File hashes bind exact saved bytes, including JSON whitespace.
"""
import argparse
from collections import defaultdict
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import sys
import tempfile

sys.dont_write_bytecode = True
CAMPAIGN = 'rra-ised-saved-2026-10-04'
RELATIVE_OUTPUT = 'research/batches/discovery-2026-10-11/sun6000-a/inventory'
MANIFEST = 'data/import-manifest-pinbase-sun6000-2026-10-11.json'  # gate-less pin base; see its description
RELEASE = 'data/archive-release-2026-10-04.json'
CHECKPOINTS = [MANIFEST, RELEASE, 'data/devices.sqlite']
REFERENCES = [
    'AGENTS.md', 'GOAL.md', 'docs/evidence-policy.md', 'docs/agent-workflow.md',
    'docs/campaign-plan.md', 'devicehunt/rra.py', 'devicehunt/rra_company_names.py',
    'devicehunt/rra_celestica_historical.py', 'devicehunt/campaign_sources.py', 'devicehunt/ised.py',
    'devicehunt/catalog.py', 'devicehunt/documents.py', 'devicehunt/fcc_index.py',
    'devicehunt/importers.py', 'devicehunt/oracle_evidence.py',
    'research/capture_rra_query.py', 'research/capture_rra_details.py',
    'research/capture_ised.py', 'tests/test_rra.py', 'tests/test_rra_empty.py',
    'tests/test_rra_capture.py', 'tests/test_rra_company_names.py',
    'tests/test_rra_accton_company.py', 'tests/test_rra_celestica_historical.py',
    'tests/test_ised.py', 'tests/test_ised_paged.py', 'tests/test_ised_capture.py',
    RELATIVE_OUTPUT + '/pin_inventory.py',
]
NOTES = [
    'research/korean-rra-official.md', 'research/korean-rra-targets.md',
    'research/rra-edge-core-spellings.md', 'research/rra-accton-company-name.md',
    'research/rra-celestica-company-names.md', 'research/rra-main-model-lists.md',
    'research/ised-radio-equipment.md',
]
HASH = re.compile(r'[0-9a-f]{64}')


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(',', ':'), allow_nan=False).encode('utf-8')


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def safe_file(root, relative):
    """Reject noncanonical paths, symlinks in any component, absent/nonfiles."""
    if not isinstance(relative, str) or not relative or '\\' in relative:
        raise ValueError('invalid relative path')
    p = PurePosixPath(relative)
    if p.is_absolute() or p.as_posix() != relative or any(x in ('..', '.') for x in p.parts):
        raise ValueError('unsafe relative path: ' + relative)
    candidate = root
    for component in p.parts:
        candidate = candidate / component
        if candidate.is_symlink():
            raise ValueError('symlink dependency: ' + relative)
    if not candidate.is_file():
        raise ValueError('missing dependency: ' + relative)
    if not candidate.resolve().is_relative_to(root.resolve()):
        raise ValueError('dependency escapes root: ' + relative)
    return candidate


def pin(root, relative, expected=None):
    path = safe_file(root, relative)
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(chunk)
    result = {'path': relative, 'sha256': h.hexdigest(), 'byte_count': path.stat().st_size}
    if expected is not None and (not isinstance(expected, str) or not HASH.fullmatch(expected)
                                 or result['sha256'] != expected):
        raise ValueError('checksum mismatch: ' + relative)
    return result


def load(root, relative):
    return json.loads(safe_file(root, relative).read_text())


def declared_entries(value, pointer=''):
    if isinstance(value, dict):
        if 'path' in value:
            if 'sha256' not in value:
                raise ValueError('declared path without hash at ' + pointer)
            yield pointer, value
        for key, child in value.items():
            yield from declared_entries(child, pointer + '/' + key.replace('~', '~0').replace('/', '~1'))
    elif isinstance(value, list):
        for n, child in enumerate(value):
            yield from declared_entries(child, pointer + '/' + str(n))


def closure(root, start, accepted_hashes):
    """Discover every path/hash entry, recursively in sibling JSON inputs."""
    files, edges, seen = {}, [], set()

    def visit(relative, expected, role, ancestors=()):
        if relative in ancestors:
            raise ValueError('cyclic dependency: ' + relative)
        manifest_key = str(PurePosixPath(relative).relative_to('data'))
        if manifest_key not in accepted_hashes or accepted_hashes[manifest_key] != expected:
            raise ValueError('accepted manifest disagrees: ' + relative)
        item = pin(root, relative, expected)
        if relative in files and files[relative]['sha256'] != item['sha256']:
            raise ValueError('inconsistent dependency hash: ' + relative)
        files.setdefault(relative, {**item, 'roles': []})
        if role not in files[relative]['roles']:
            files[relative]['roles'].append(role)
        if relative in seen:
            return
        seen.add(relative)
        if not relative.endswith('.json'):
            return
        for pointer, entry in declared_entries(load(root, relative)):
            name = entry['path']
            if not isinstance(name, str) or PurePosixPath(name).name != name or name in ('', '.', '..'):
                raise ValueError('dependency must be a sibling filename: ' + str(name))
            child = str(PurePosixPath(relative).parent / name)
            child_role = 'company_evidence' if role == 'company_evidence' or pointer == '/company_evidence' else 'saved_response'
            edges.append({'parent': relative, 'pointer': pointer, 'child': child,
                          'declared_sha256': entry['sha256']})
            visit(child, entry['sha256'], child_role, ancestors + (relative,))

    key = str(PurePosixPath(start).relative_to('data'))
    visit(start, accepted_hashes[key], 'source_manifest')
    return files, edges


def occurrence_groups(items, key):
    groups = defaultdict(list)
    for n, row in enumerate(items, 1):
        groups[key(row)].append(n)
    return [{'value': k, 'ordinals': v} for k, v in sorted(groups.items()) if len(v) > 1]


def replay(root, source, relative):
    # These established source validators read files only. Never call import_*.
    sys.path.insert(0, str(root))
    from devicehunt import ised, rra, rra_company_names
    from devicehunt.catalog import normalize, target_for
    path = safe_file(root, relative)
    data = load(root, relative)
    if source == 'rra':
        data, ready = rra.validate(path)
        sid = 'rra-official-query-' + hashlib.sha256(json.dumps(data['query'], sort_keys=True).encode()).hexdigest()[:12]
        reviewed = rra_company_names.load(path.parent, data.get('company_evidence'))
        claims = rra.composite_claims(path.parent, data, ready)
        originals = [{'csv_cells': cells, 'detail_entry': entry, 'labelled_fields': row}
                     for cells, entry, row in ready]
        ordinals = [{'ordinal': n, 'certificate': row['certificate_number'],
                     'detail_path': str(PurePosixPath(relative).parent / entry['path']),
                     'logical_sha256': digest(original),
                     'projected_source_keys': [row['certificate_number'] + ':' + model for _, model in rra.models(row)]
                         if rra.claim_target(row, reviewed, claims) else []}
                    for n, ((_, entry, row), original) in enumerate(zip(ready, originals), 1)]
        expectations = {'raw_registry_rows': len(ready), 'detail_pages': len(ready),
                        'saved_html_files': 1 + len(ready) + int('first_result_page' in data) + int('empty_result' in data),
                        'csv_rows_before': len(ready), 'csv_rows_after': len(ready),
                        'model_assertions': sum(len(x['projected_source_keys']) for x in ordinals),
                        'eligible_manufacturer_rows': sum(bool(rra.claim_target(row, reviewed, claims)) for _, _, row in ready),
                        'comparison_mode': data.get('csv_comparison', 'byte_identical'),
                        'validated_original_records_sha256': digest(originals),
                        'canonical_record_format': 'ordered [{csv_cells,detail_entry,labelled_fields}]',
                        'occurrences': ordinals}
        scope = {'exact_query': data['query'], 'form_url': data['form_url'], 'csv_url': data['csv_url'],
                 'upstream_total': None, 'coverage': 'complete returned query CSV with every detail; not full regulator coverage',
                 'date_semantics': 'approval-date window; not a change feed'}
        special = {'explicit_zero': not ready, 'zero_proof': data.get('empty_result'),
                   'company_evidence': data.get('company_evidence'),
                   'within_query_duplicate_certificates': [],
                   'manufacturer_applicant_differences': [n for n, (_, _, row) in enumerate(ready, 1)
                                                           if row['manufacturer'] != row['applicant']]}
    else:
        if data['schema_version'] == 1:
            number, company, exported = ised.validate_capture(path)
            sid = 'ised-rel-' + number
            rows = []
            for entry in data['responses']:
                text = (path.parent / entry['path']).read_text(encoding='utf-8-sig')
                if 'id="equipmentSearchResultTable"' in text:
                    _, _, labelled = ised.html_rows(text)
                    rows = [{'group_number': number, 'company': company, 'cells': cells} for cells in labelled]
                    break
            # Version 1 validates a duplicate-sensitive exact multiset.
            stats = {'query_records': len(rows), 'csv_rows': len(exported),
                     'html_without_exact_csv': 0, 'unmatched_csv_rows': 0}
            selected = []
            for entry in data['responses']:
                text = (path.parent / entry['path']).read_text(encoding='utf-8-sig')
                if 'id="equipmentSearchResultTable"' in text:
                    selected.append({'path': entry['path'], 'total': len(rows), 'start': 1, 'end': len(rows), 'row_count': len(rows)})
            repeated_result_pages = selected[1:]
            selected = selected[:1]
            comparison = 'exact_html_csv_identity_multiset'
            matched = set(range(1, len(exported) + 1))
        else:
            query, rows, exported, stats = ised.validate_paged_capture(path)
            sid = 'ised-rel-query-' + normalize(query)
            selected = []
            for name in data['result_pages']:
                page = ised.labelled_page((path.parent / name).read_text(encoding='utf-8-sig'))
                selected.append({'path': name, **{k: page[k] for k in ('total', 'start', 'end')}, 'row_count': len(page['rows'])})
            comparison = 'contiguous_html_ranges_stable_totals_exact_csv_matches_only'
            matched = {m['row_number'] for row in rows for m in row['csv_matches']}
        html_ordinals = [{'ordinal': n, 'projected_source_key': str(n) if target_for(row['company']) == target_for(data['company_query']) else None,
                          'certificate': row['cells'][4], 'hvin': row['cells'][0], 'logical_sha256': digest(row),
                          'exact_csv_ordinals': [m['row_number'] for m in row.get('csv_matches', [])] if data['schema_version'] == 2 else None}
                         for n, row in enumerate(rows, 1)]
        csv_ordinals = [{'ordinal': n, 'certificate': row[7], 'hvin': row[2],
                         'logical_sha256': digest(row), 'matches_html_identity': n in matched}
                        for n, row in enumerate(exported, 1)]
        expectations = {**stats, 'imported_target_assertions': sum(x['projected_source_key'] is not None for x in html_ordinals),
                        'comparison_mode': comparison, 'selected_pages': selected,
                        'saved_response_count': len(data['responses']),
                        'saved_html_files': sum('<html' in (path.parent / x['path']).read_bytes().decode('utf-8-sig')[:1000].lower()
                                                for x in data['responses']),
                        'validated_html_records_sha256': digest(rows), 'validated_csv_records_sha256': digest(exported),
                        'canonical_record_format': 'ordered validator HTML row dictionaries; ordered original CSV cell arrays',
                        'html_occurrences': html_ordinals, 'csv_occurrences': csv_ordinals}
        if data['schema_version'] == 1:
            expectations['projection_ordinal_origin'] = 'CSV row ordinal; HTML multiset validates identities, not record order'
            expectations['projected_source_keys_in_csv_order'] = [str(n) for n in range(1, len(exported) + 1)]
            expectations['repeated_result_pages'] = repeated_result_pages
            for item in html_ordinals:
                item.pop('projected_source_key')
        scope = {'exact_company_query': data['company_query'], 'date_range': None,
                 'date_range_status': 'not declared; do not infer from returned approval dates',
                 'endpoint': ised.URL, 'reported_query_total': stats['query_records'], 'upstream_total': None,
                 'saved_response_context': data['responses'],
                 'coverage': 'complete returned company-name query HTML; not full REL coverage',
                 'session_scope': relative, 'legacy_session_id': None, 'live_cross_session_reuse': False}
        special = {'explicit_zero': not rows, 'zero_proof_result_pages': data.get('result_pages') if not rows else None,
                   'repeated_html_certificates': occurrence_groups(rows, lambda row: row['cells'][4]),
                   'repeated_csv_certificates': occurrence_groups(exported, lambda row: row[7]),
                   'byte_equivalent_csv_record_duplicates': occurrence_groups(exported, digest),
                   'html_without_exact_csv_ordinals': [n for n, row in enumerate(rows, 1) if data['schema_version'] == 2 and not row['csv_matches']],
                   'unmatched_csv_ordinals': sorted(set(range(1, len(exported) + 1)) - matched),
                   'blank_html_certificate_ordinals': [n for n, row in enumerate(rows, 1) if not row['cells'][4]],
                   'non_target_html_ordinals': [n for n, row in enumerate(rows, 1) if target_for(row['company']) != target_for(data['company_query'])],
                   'company_heading_number_disagreement_ordinals': [n for n, row in enumerate(rows, 1)
                       if any(m['cells'][0] != row['group_number'] for m in row.get('csv_matches', []))]}
    return {'unit_id': sid, 'source': source, 'source_manifest': relative,
            'source_manifest_schema': data['schema_version'], 'scope': scope,
            'legacy_metadata': {'capture_time': None, 'capture_time_status': 'unknown in input manifest; source-note dates are contextual claims',
                                'collector_version': None, 'collector_version_status': 'unknown; present collector code hash is not legacy version',
                                'mtime_used_as_evidence': False},
            'validated_outcome': 'explicit_zero' if special['explicit_zero'] else 'validated_nonempty',
            'expectations': expectations, 'special_cases': special}


def restrictions(root, units, context_files):
    items = [
        {'restriction_id': 'accton-2013-original-pdf', 'kind': 'access_denied', 'status_code': 402,
         'resource_url': 'https://www.accton.com/wp-content/uploads/2017/09/2013-Consolidated-Report.pdf',
         'input': 'data/raw/rra-accton-official/accton-singapore-2013.json', 'selector': '/scope'},
        {'restriction_id': 'celestica-2025-original-filing', 'kind': 'access_denied', 'status_code': 403,
         'resource_url': 'https://celestica.gcs-web.com/node/17171/html',
         'input': 'data/raw/rra-celestica-official/celestica-subsidiaries-2025.json', 'selector': '/scope'},
        {'restriction_id': 'celestica-archived-original-exhibit', 'kind': 'access_denied', 'status_code': 403,
         'resource_url': 'https://corporate.celestica.com/static-files/aae92c39-4634-4e66-805e-6fb4adca1376',
         'input': 'data/raw/rra-celestica-official/celestica-archived-21.1-reviewed.json', 'selector': '/retrieval_limit'},
        {'restriction_id': 'ised-edge-core-form-error', 'kind': 'failed_query_not_zero', 'status_code': None,
         'source_query': 'Edge-Core', 'resource_url': None,
         'input': 'research/ised-radio-equipment.md', 'selector': 'Expanded queries and source discrepancies'},
        {'restriction_id': 'rra-emsit-api-failure', 'kind': 'failed_endpoint', 'status_code': 400,
         'resource_url': None, 'input': 'research/korean-rra-official.md', 'selector': 'Relationship to the previous mirror capture'},
    ]
    for item in items:
        item['input_sha256'] = pin(root, item['input'])['sha256']
        item['action'] = 'no requests; preserve prior outcome; no automatic retry or alternate route'
        item['restriction_time'] = None
        item['restriction_time_status'] = 'unknown exact attempt time'
    inspections = []
    for unit in units:
        parent = PurePosixPath(unit['source_manifest']).parent
        found = [x['path'] for x in context_files if PurePosixPath(x['path']).parent == parent]
        inspections.append({'unit_id': unit['unit_id'], 'directory': str(parent), 'saved_reports_read': found,
                            'dedicated_transport_failure_report': None,
                            'restriction_state': 'unknown beyond named saved restrictions',
                            'absence_semantics': 'no dedicated report found is unknown; not unrestricted'})
    return {'inputs_read': NOTES + [x['path'] for x in context_files] +
                           [u['source_manifest'] for u in units] +
                           sorted({x['input'] for x in items if x['input'].startswith('data/')}), 'known_outcomes': items,
            'unit_inspections': inspections, 'global_policy_input': 'docs/evidence-policy.md',
            'current_upstream_access': 'unknown; not probed', 'throttle_state': 'unknown; no current attempt',
            'scope_rule': 'denied resource identity persists across URL changes; broader saved restrictions remain authoritative'}


def review_samples(units, release):
    samples = []
    certificates = defaultdict(list)
    for unit in units:
        sid, source, special = unit['unit_id'], unit['source'], unit['special_cases']
        if special['explicit_zero']:
            samples.append({'case': 'explicit_zero', 'unit_id': sid, 'selector': special.get('zero_proof') or special.get('zero_proof_result_pages')})
        if source == 'rra':
            if unit['scope']['exact_query']['maker'] == 'Celestica':
                samples.append({'case': 'saved_rra_query_stability', 'unit_id': sid,
                                'source_manifest': unit['source_manifest'],
                                'manifest_selectors': ['/csv_before', '/csv_after'],
                                'comparison_mode': unit['expectations']['comparison_mode'],
                                'limit': 'saved comparison only; distinct legacy HTTP attempts/times unverified'})
            for row in unit['expectations']['occurrences']:
                certificates[row['certificate']].append({'unit_id': sid, 'ordinal': row['ordinal'], 'detail_path': row['detail_path']})
                if any(key.endswith(':AZ9074-64X-DC-42') for key in row['projected_source_keys']):
                    samples.append({'case': 'shared_literal_model_separate_manufacturer_claims', 'unit_id': sid,
                                    'raw_ordinal': row['ordinal'], 'detail_path': row['detail_path'],
                                    'literal_model': 'AZ9074-64X-DC-42',
                                    'limit': 'preserve both source manufacturer associations; no winning role, supplier contract or deployment inference'})
            if special['company_evidence']:
                samples.append({'case': 'reviewed_company_evidence_dependency', 'unit_id': sid, 'selector': special['company_evidence']})
            if special['manufacturer_applicant_differences']:
                samples.append({'case': 'separate_applicant_manufacturer', 'unit_id': sid,
                                'raw_ordinals': special['manufacturer_applicant_differences'][:1]})
        else:
            for field in ['html_without_exact_csv_ordinals', 'unmatched_csv_ordinals', 'blank_html_certificate_ordinals',
                          'non_target_html_ordinals', 'company_heading_number_disagreement_ordinals']:
                if special[field]:
                    samples.append({'case': field, 'unit_id': sid, 'raw_ordinals': special[field]})
            if special['repeated_html_certificates']:
                samples.append({'case': 'repeated_html_certificate', 'unit_id': sid, 'selector': special['repeated_html_certificates'][0]})
            if special['byte_equivalent_csv_record_duplicates']:
                samples.append({'case': 'duplicate_original_csv_records', 'unit_id': sid,
                                'selector': special['byte_equivalent_csv_record_duplicates'][0]})
    overlaps = [{'certificate': k, 'occurrences': v, 'independent_corroboration': False}
                for k, v in sorted(certificates.items()) if len(v) > 1]
    for overlap in overlaps:
        samples.append({'case': 'cross_query_certificate_overlap', **overlap})
    # Select four already accepted statements, not new pilot PDF evidence.
    for link in release['links']:
        samples.append({'case': 'existing_accepted_pdf_lineage', 'statement_id': link['statement_id'],
                        'artifact_sha256': link['artifact_sha256'], 'receipt_id': link['receipt_id'],
                        'locator': link['locator'], 'release_id': release['release_id'],
                        'scope': 'existing release regression only; outside pilot source bytes'})
    samples.append({'case': 'failed_query_not_zero', 'restriction_id': 'ised-edge-core-form-error',
                    'input': 'research/ised-radio-equipment.md', 'scope': 'context only; no extra import job'})
    return samples, overlaps


def build(root):
    manifest = load(root, MANIFEST)
    selected = [(n, j) for n, j in enumerate(manifest['imports']) if j.get('source') in ('rra', 'ised')]
    units, all_files, all_edges = [], {}, []
    context = {}
    for ordinal, job in selected:
        relative = 'data/' + job['path']
        files, edges = closure(root, relative, manifest['files'])
        unit = replay(root, job['source'], relative)
        unit.update(import_ordinal_zero_based=ordinal, accepted_job=job, dependency_paths=sorted(files))
        units.append(unit)
        for name, item in files.items():
            if name not in all_files:
                all_files[name] = {**item, 'unit_ids': []}
            all_files[name]['unit_ids'].append(unit['unit_id'])
        all_edges.extend({**edge, 'unit_id': unit['unit_id']} for edge in edges)
        parent = safe_file(root, relative).parent
        for report in sorted(parent.glob('*progress*.json')):
            report_relative = report.relative_to(root).as_posix()
            report_data = load(root, report_relative)
            context[report_relative] = {**pin(root, report_relative), 'classification': 'saved_collection_status_context',
                                        'declared_complete': report_data.get('complete'),
                                        'declared_entry_count': len(report_data.get('entries', [])),
                                        'status_keys': [k for k in report_data if k != 'entries']}
    release = load(root, RELEASE)
    samples, overlaps = review_samples(units, release)
    files = [all_files[k] for k in sorted(all_files)]
    hashes = {x['sha256']: x['byte_count'] for x in files}
    fixture_hashes = {l['artifact_sha256'] for l in release['links']} | {l['locator']['text_sha256'] for l in release['links']}
    fixtures = [pin(root, 'data/' + a['paths'][0], a['sha256'])
                for a in release['artifacts'] if a['sha256'] in fixture_hashes]
    inventory = {'schema_version': 1, 'campaign_id': CAMPAIGN, 'definition_version': 1,
                 'hash_semantics': 'canonical JSON hashes use sorted keys, compact separators, UTF-8, no newline; file hashes bind exact bytes',
                 'network_requests': 0, 'scope_counts': {'rra_jobs': sum(j['source'] == 'rra' for _, j in selected),
                                                       'ised_jobs': sum(j['source'] == 'ised' for _, j in selected)},
                 'pilot_source_files': files, 'dependency_edges': all_edges, 'units': units,
                 'accepted_checkpoints_not_pilot_evidence': [pin(root, p) for p in CHECKPOINTS],
                 'code_policy_references_not_evidence': [pin(root, p) for p in REFERENCES],
                 'source_context_inputs_not_pilot_evidence': [pin(root, p) for p in NOTES] + list(context.values()),
                 'existing_review_fixture_files_not_pilot_evidence': fixtures,
                 'source_restriction_inputs': restrictions(root, units, list(context.values())),
                 'review_samples': samples, 'cross_query_certificate_overlaps': overlaps,
                 'pilot_source_storage': {'file_paths': len(files), 'unique_sha256': len(hashes),
                                          'logical_bytes_by_path': sum(x['byte_count'] for x in files),
                                          'unique_content_bytes': sum(hashes.values()),
                                          'source_copy_bytes_created': 0},
                 'unknowns': ['legacy exact capture times and collector versions', 'current upstream access and throttle state',
                              'upstream full-database totals', 'ISED original session ids and precise submitted form fields beyond saved company query',
                              'no dedicated historical transport-failure reports found in pinned source directories'],
                 'stop': '12 accepted source jobs and recursively declared dependencies pinned and validated; no collection/import'}
    return inventory


def definition(inventory):
    return {'schema_version': 1, 'campaign_id': CAMPAIGN, 'version': 1, 'stage': 'c0',
            'mode': 'offline_saved_inputs_only', 'input_inventory_sha256': digest(inventory),
            'inventory_file_sha256': hashlib.sha256(pretty(inventory)).hexdigest(),
            'source_content_set_sha256': digest(inventory['pilot_source_files']),
            'job_scope': [{'unit_id': u['unit_id'], 'source': u['source'], 'source_manifest': u['source_manifest'],
                           'import_ordinal_zero_based': u['import_ordinal_zero_based'], 'scope': u['scope']}
                          for u in inventory['units']],
            'pinned_adapter_recipe_policy': inventory['code_policy_references_not_evidence'],
            'budgets': {'network_requests': 0, 'network_bytes': 0, 'new_source_bytes': 0,
                        'max_source_jobs': len(inventory['units']),
                        'max_referenced_source_paths': inventory['pilot_source_storage']['file_paths'],
                        'new_metadata_and_candidates_bytes_cap': 64 * 1024 * 1024,
                        'free_space_floor_bytes': 2 * 1024 * 1024 * 1024},
            'storage_policy': 'register existing files in place; no catalog/source copies; overflow stops/checkpoints without dropping mandatory dependencies; never delete evidence/history to make room; integration DB space is outside this metadata-only cap and requires a separate assignment',
            'source_restrictions': inventory['source_restriction_inputs'],
            'outcome_vocabulary': ['planned', 'observed', 'validated_nonempty', 'explicit_zero', 'unknown', 'failed', 'blocked', 'throttled', 'budget_exhausted'],
            'coverage_rules': ['explicit zero needs validated source proof; unavailable/error/unknown never means zero',
                               'accounted-for terminal work is separate from successful observed coverage',
                               'duplicates, ordinal occurrences and conflict statements remain separate',
                               'snapshot positions are not durable reviewed-claim identities'],
            'refresh_policy': {'enabled': False, 'next_eligibility': None,
                               'rra': 'future reviewed complete refresh must retain exact query/date scope; before/after are distinct HTTP attempts; comparison mode retained',
                               'ised': 'saved replay only; live session/form/cache contract unimplemented; no cross-session live reuse'},
            'exclusions': ['network collection', 'source import', 'accepted catalog changes', 'releasing/publishing data',
                           'extra spellings/date windows', 'scheduler', 'seller/provider contact'],
            'review_sample_selectors': inventory['review_samples'],
            'stop_condition': inventory['stop']}


def pretty(value):
    return json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True, allow_nan=False).encode('utf-8') + b'\n'


def verify_pins(root, pins):
    for item in pins:
        found = pin(root, item['path'], item['sha256'])
        if found['byte_count'] != item['byte_count']:
            raise ValueError('byte count mismatch: ' + item['path'])


def verify(root, output):
    inventory = load(output, 'inventory.json')
    campaign = load(output, 'campaign-definition.json')
    verify_pins(root, [x for key in ['pilot_source_files', 'accepted_checkpoints_not_pilot_evidence',
                                  'code_policy_references_not_evidence', 'source_context_inputs_not_pilot_evidence',
                                  'existing_review_fixture_files_not_pilot_evidence'] for x in inventory[key]])
    regenerated = build(root)
    if canonical(regenerated) != canonical(inventory):
        raise ValueError('inventory differs from deterministic current validation')
    if campaign != definition(inventory):
        raise ValueError('definition disagrees with inventory')
    if safe_file(output, 'inventory.sha256').read_text().strip() != hashlib.sha256(safe_file(output, 'inventory.json').read_bytes()).hexdigest():
        raise ValueError('inventory file hash receipt disagrees')
    return {'verified': True, 'units': len(inventory['units']),
            'source_paths': len(inventory['pilot_source_files']), 'canonical_inventory_sha256': digest(inventory),
            'inventory_file_sha256': campaign['inventory_file_sha256'], 'network_requests': 0}


def safe_directory(path):
    """Reject symlink roots/output ancestors before any resolve hides them."""
    path = path.absolute()
    if '..' in path.parts:
        raise ValueError('parent traversal in root/output directory')
    current = Path(path.anchor)
    for part in path.parts[1:]:
        current /= part
        if current.is_symlink():
            raise ValueError('symlink root/output directory: ' + str(current))
        if current.exists() and not current.is_dir():
            raise ValueError('root/output ancestor is not a directory: ' + str(current))
    return path


def output_bytes(output):
    total = 0
    for directory, subdirs, files in os.walk(output, followlinks=False):
        for name in subdirs + files:
            path = Path(directory) / name
            if path.is_symlink():
                raise ValueError('symlink in output directory: ' + str(path))
        for name in files:
            path = Path(directory) / name
            if not path.is_file():
                raise ValueError('nonfile output entry: ' + str(path))
            total += path.stat().st_size
    return total


def publication_growth(output, payloads):
    """Preflight every destination; a changed bundle writes nothing."""
    growth = 0
    for name, body in payloads.items():
        target = output / name
        if target.is_symlink():
            raise ValueError('symlink publication destination: ' + name)
        if target.exists():
            if not target.is_file() or target.read_bytes() != body:
                raise ValueError('frozen output differs: ' + name + '; use a new output directory/version')
        else:
            growth += len(body)
    return growth


def publish(output, payloads):
    """Atomic exclusive per-file publication, with exact-byte idempotence.

    A crash may leave a partial bundle; same-byte rerun completes it. There is
    no overwrite or false bundle-transaction claim. fsync before and after link.
    """
    publication_growth(output, payloads)
    for name, body in payloads.items():
        target = output / name
        if target.exists() or target.is_symlink():
            if safe_file(output, name).read_bytes() != body:
                raise ValueError('frozen output changed during publication: ' + name)
            continue
        temporary_path = None
        try:
            with tempfile.NamedTemporaryFile(prefix='.pending-', dir=output, delete=False) as temporary:
                temporary_path = Path(temporary.name)
                temporary.write(body)
                temporary.flush()
                os.fsync(temporary.fileno())
            try:
                os.link(temporary_path, target)
            except FileExistsError:
                # Another publisher cannot replace content; only exact bytes
                # are a compatible completion of this pending publication.
                if safe_file(output, name).read_bytes() != body:
                    raise ValueError('competing frozen publication differs: ' + name)
            descriptor = os.open(output, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
        finally:
            if temporary_path is not None:
                temporary_path.unlink(missing_ok=True)


def self_test():
    results = []
    with tempfile.TemporaryDirectory(prefix='campaign-pins-') as temporary:
        root = Path(temporary)
        (root / 'data').mkdir()
        (root / 'data/body.bin').write_bytes(b'original')
        expected = pin(root, 'data/body.bin')['sha256']
        manifest = {'dep': {'path': 'body.bin', 'sha256': expected}}
        (root / 'data/unit.json').write_bytes(pretty(manifest))
        accepted = {'body.bin': expected, 'unit.json': pin(root, 'data/unit.json')['sha256']}
        first = closure(root, 'data/unit.json', accepted)
        assert first == closure(root, 'data/unit.json', accepted)
        assert len(first[0]) == 2 and len(first[1]) == 1
        results.append('deterministic recursive closure')
        def fails(label, operation):
            try:
                operation()
            except (ValueError, FileNotFoundError):
                results.append(label)
            else:
                raise AssertionError('negative case unexpectedly passed: ' + label)
        (root / 'data/body.bin').rename(root / 'data/body.saved')
        fails('missing declared dependency rejected', lambda: closure(root, 'data/unit.json', accepted))
        (root / 'data/body.saved').rename(root / 'data/body.bin')
        (root / 'data/body.bin').write_bytes(b'changed')
        fails('wrong dependency bytes rejected', lambda: closure(root, 'data/unit.json', accepted))
        (root / 'data/body.bin').write_bytes(b'original')
        fails('manifest hash disagreement rejected', lambda: closure(root, 'data/unit.json', {**accepted, 'body.bin': '0' * 64}))
        for relative in ['../escape', '/absolute', 'data/../body.bin', 'data//body.bin', './data/body.bin', 'data\\body.bin']:
            fails('unsafe path rejected: ' + relative, lambda relative=relative: safe_file(root, relative))
        (root / 'data/link.bin').symlink_to('body.bin')
        fails('file symlink rejected', lambda: pin(root, 'data/link.bin'))
        (root / 'linked').symlink_to('data', target_is_directory=True)
        fails('parent symlink rejected', lambda: pin(root, 'linked/body.bin'))
        # Recursive JSON company evidence, not merely top-level files.
        (root / 'data/company.json').write_bytes(pretty({'base': {'path': 'body.bin', 'sha256': expected}}))
        h = pin(root, 'data/company.json')['sha256']
        (root / 'data/unit.json').write_bytes(pretty({'company_evidence': {'path': 'company.json', 'sha256': h}}))
        accepted.update({'company.json': h, 'unit.json': pin(root, 'data/unit.json')['sha256']})
        files, edges = closure(root, 'data/unit.json', accepted)
        assert len(files) == 3 and len(edges) == 2 and files['data/body.bin']['roles'] == ['company_evidence']
        results.append('recursive company dependency retained')
        output = root / 'output'
        output.mkdir()
        publish(output, {'inventory.json': b'first immutable bytes'})
        first_bytes = (output / 'inventory.json').read_bytes()
        publish(output, {'inventory.json': b'first immutable bytes'})
        assert first_bytes == (output / 'inventory.json').read_bytes()
        results.append('exclusive publication exact-byte reuse')
        fails('changed existing frozen output rejected', lambda: publish(output, {'inventory.json': b'changed', 'other.json': b'not published'}))
        assert (output / 'inventory.json').read_bytes() == first_bytes and not (output / 'other.json').exists()
        assert output_bytes(output) == len(first_bytes)
        results.append('publication refusal preserves original and publishes no sibling')
        fails('symlink root argument rejected', lambda: safe_directory(root / 'linked'))
        fails('symlink output argument rejected', lambda: safe_directory(root / 'linked/output'))
    return {'passed': len(results), 'checks': results, 'network_requests': 0, 'database_copies': 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['build', 'verify', 'self-test'])
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[4])
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    root = safe_directory(args.root)
    if not root.is_dir():
        raise ValueError('missing source root')
    output = safe_directory(args.output or root / RELATIVE_OUTPUT)
    if args.command == 'self-test':
        result = self_test()
    elif args.command == 'verify':
        result = verify(root, output)
    else:
        output.mkdir(parents=True, exist_ok=True)
        inventory = build(root)
        campaign = definition(inventory)
        usage = shutil.disk_usage(root)
        payloads = {'inventory.json': pretty(inventory), 'campaign-definition.json': pretty(campaign),
                    'inventory.sha256': (campaign['inventory_file_sha256'] + '\n').encode()}
        receipt = {'measured_at': datetime.now(timezone.utc).isoformat(), 'measurement_kind': 'local registration/validation; not capture time',
                   'canonical_inventory_sha256': digest(inventory), 'inventory_file_sha256': campaign['inventory_file_sha256'],
                   'filesystem': {'total_bytes': usage.total, 'used_bytes': usage.used, 'free_bytes': usage.free},
                   'capacity_recommendation': campaign['budgets'], 'pilot_source_storage': inventory['pilot_source_storage'],
                   'source_allocated_bytes': sum(safe_file(root, x['path']).stat().st_blocks * 512 for x in inventory['pilot_source_files']),
                   'generated_inventory_definition_bytes': len(pretty(inventory)) + len(pretty(campaign)),
                   'network_requests': 0}
        if (output / 'measurement-receipt.json').exists() or (output / 'measurement-receipt.json').is_symlink():
            # Preserve the first measurement. Reuse only if it belongs to this
            # exact frozen inventory, never attach an old receipt to new pins.
            original_receipt = load(output, 'measurement-receipt.json')
            if original_receipt['inventory_file_sha256'] != campaign['inventory_file_sha256']:
                raise ValueError('existing measurement belongs to different inventory')
            payloads['measurement-receipt.json'] = safe_file(output, 'measurement-receipt.json').read_bytes()
        else:
            payloads['measurement-receipt.json'] = pretty(receipt)
        growth = publication_growth(output, payloads)
        if output_bytes(output) + growth > campaign['budgets']['new_metadata_and_candidates_bytes_cap']:
            raise ValueError('aggregate output-directory cap exceeded; checkpoint rather than dropping mandatory dependencies')
        if usage.free - growth < campaign['budgets']['free_space_floor_bytes']:
            raise ValueError('free-space floor reached; stop without deleting evidence')
        publish(output, payloads)
        result = verify(root, output)
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == '__main__':
    main()
