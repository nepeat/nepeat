"""Finite offline milestone-0 audit; no collector, importer or archive implementation.

Run from the workspace with PYTHONPATH=. and -B. Default creates inventory.json
exclusively; --check verifies the existing inventory without writing any file.
"""
import argparse
import hashlib
import json
import sqlite3
from pathlib import Path

from devicehunt.reference_documents import PREFIX, prepare

ROOT = Path(__file__).resolve().parents[3]
DEST = Path(__file__).with_name('inventory.json')


def digest(body):
    return hashlib.sha256(body).hexdigest()


def pin(path):
    body = path.read_bytes()
    return {'path': path.relative_to(ROOT).as_posix(),
            'sha256': digest(body), 'byte_count': len(body)}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def inventory():
    manifest = ROOT / 'data/import-manifest.json'
    m = json.loads(manifest.read_bytes())
    jobs = [j for j in m['imports'] if j['source'] == 'reference-documents']
    require(len(jobs) == 1, 'Expected exactly one complete reference collection')
    bundle = manifest.parent / jobs[0]['path']
    pins = {'import_manifest': pin(manifest), 'complete_bundle': pin(bundle),
            'live_checkpoint': pin(ROOT / 'data/devices.sqlite')}
    require(m['files'][jobs[0]['path']] == pins['complete_bundle']['sha256'],
            'Manifest bundle hash mismatch')
    _, ready = prepare(bundle)
    artifacts, dependencies, documents = {}, [], []
    for ordinal, (entry, pages) in enumerate(ready):
        for role in ('pdf', 'text'):
            path = bundle.parent / entry[role]['path']
            obj = pin(path)
            require(obj['sha256'] == entry[role]['sha256'] ==
                    m['files'][path.relative_to(manifest.parent).as_posix()],
                    'Dependency hash or manifest membership mismatch')
            dependencies.append(obj)
            artifact = artifacts.setdefault(obj['sha256'], {
                'sha256': obj['sha256'], 'byte_count': obj['byte_count'], 'locations': []})
            if obj['path'] not in artifact['locations']:
                artifact['locations'].append(obj['path'])
        envelope = json.loads((bundle.parent / entry['text']['path']).read_bytes())
        require(set(envelope) == {'pdf_sha256', 'extraction_method', 'pages'},
                'Additional legacy metadata needs inspection')
        special = entry['id'] == 'ingrasys-servers-reconstructed-2026-10-04'
        documents.append({
            'inventory_key': pins['complete_bundle']['sha256'] + f':/documents/{ordinal}',
            'bundle_locator': f'/documents/{ordinal}', 'entry': entry,
            'resource_proposal': {
                'namespace': 'public-device-references-v1', 'upstream_key': None,
                'local_document_id': entry['id'], 'source_url': entry['url'],
                'request_fingerprint': None},
            'capture_proposal': {
                'record_kind': 'legacy_registration_only',
                'provenance_basis': 'bundle association; not a newly observed request',
                'artifact_sha256': None if special else entry['pdf']['sha256'],
                'artifact_representation': 'reconstructed_derivative' if special else
                    'legacy_pdf_bytes; acquisition byte representation unknown',
                'requested_url': None, 'final_url': None, 'request_method': None,
                'collector_version': None, 'started_at': None, 'ended_at': None,
                'http_status': None, 'response_headers': None, 'network_capture_id': None},
            'derivation_proposal': {
                'record_kind': 'legacy_saved_output',
                'input_artifacts': [{'role': 'pdf', 'ordinal': 0,
                                     'sha256': entry['pdf']['sha256']}],
                'output_artifacts': [{'role': 'page_text_json', 'ordinal': 0,
                                      'sha256': entry['text']['sha256']}],
                'method_label': envelope['extraction_method'], 'extractor_version': None,
                'code_hash': None, 'dependency_versions': None, 'configuration': None,
                'output_schema_version': None, 'started_at': None, 'ended_at': None,
                'historical_run_status': None,
                'reuse_policy': 'saved output reusable after hash/envelope verification; '
                    'label alone cannot identify a compatible new extraction recipe'},
            'document_version_proposal': {
                'document_id': entry['id'], 'pdf_sha256': entry['pdf']['sha256'],
                'text_sha256': entry['text']['sha256'], 'page_count': len(pages),
                'pages_with_text': sum(bool(p['text'].strip()) for p in pages),
                'page_locator_contract': {'pdf_page_number': 'one-based ordinal as recorded',
                    'json_pointer': '/pages/{pdf_page_number - 1}/text',
                    'printed_page_labels': None}},
            'limitations': [
                'bundle tier and scope copied literally; not an independent authenticity or role finding',
                'no accepted claims or capture history created'] + ([
                'reconstruction lineage in special_lineage; never a downloaded-original PDF']
                if special else [])})

    provenance_path = ROOT / ('research/batches/server-evidence-2026-10-04/'
                              'ingrasys/derivative-provenance.json')
    ledger_path = provenance_path.with_name('capture-ledger.json')
    provenance = json.loads(provenance_path.read_bytes())
    capture = json.loads(ledger_path.read_bytes())['captures'][0]
    support_paths = [provenance_path, ledger_path, provenance_path.with_name('handoff.md'),
                     ROOT / provenance['original_snapshot'], ROOT / capture['body'],
                     ROOT / capture['headers']]
    supporting = [pin(p) for p in support_paths]
    original = (ROOT / provenance['original_snapshot']).read_bytes()
    fresh = (ROOT / capture['body']).read_bytes()
    require(digest(original) == provenance['original_sha256'] ==
            digest(fresh) == capture['body_sha256'], 'Malformed body copy mismatch')
    start, end = provenance['source_span_half_open']
    recovered = original[start:end] + bytes.fromhex(provenance['appended_bytes_hex'])
    require(digest(recovered) == provenance['derivative_sha256'], 'Recovery hash mismatch')
    require(recovered == (bundle.parent /
            'ingrasys-servers-reconstructed-2026-10-04.pdf').read_bytes(),
            'Recovery bytes differ from complete-bundle sibling')

    with sqlite3.connect((ROOT / 'data/devices.sqlite').as_uri() + '?mode=ro', uri=True) as db:
        db.row_factory = sqlite3.Row
        rows = {r['source_id']: dict(r) for r in db.execute(
            "SELECT * FROM documents WHERE source_id GLOB 'reference-pdf-*'")}
        require(set(rows) == {PREFIX + e['id'] for e, _ in ready},
                'Live reference set differs from selected bundle')
        for entry, pages in ready:
            row = rows[PREFIX + entry['id']]
            require(row['pdf_sha256'] == entry['pdf']['sha256'] and
                    row['page_count'] == len(pages) and row['source_url'] == entry['url'],
                    'Live reference metadata mismatch')
            actual = [dict(r) for r in db.execute(
                'SELECT page_number,text,text_status FROM document_pages '
                'WHERE document_key=? ORDER BY page_number', (row['document_key'],))]
            expected = [{'page_number': p['page'], 'text': p['text'],
                         'text_status': 'embedded_text' if p['text'].strip()
                         else 'no_embedded_text'} for p in pages]
            require(actual == expected, 'Live reference page projection differs')
    for record in list(pins.values()) + dependencies + supporting:
        require(pin(ROOT / record['path']) == record, 'Input changed during inventory')
    return {
        'inventory_format_version': 1,
        'purpose': 'milestone 0 planning inventory; not an archive registry or accepted release manifest',
        'inventory_date': '2026-10-04', 'path_policy': 'workspace-relative', 'pins': pins,
        'counts': {'documents': len(ready), 'pdf_text_dependency_paths': len(dependencies),
                   'unique_dependency_artifacts': len(artifacts),
                   'pages': sum(len(p) for _, p in ready),
                   'pages_with_text': sum(bool(x['text'].strip()) for _, p in ready for x in p),
                   'supporting_provenance_paths': len(supporting)},
        'artifacts': list(artifacts.values()), 'dependencies': dependencies,
        'documents': documents, 'supporting_provenance': supporting,
        'special_lineage': {
            'document_id': 'ingrasys-servers-reconstructed-2026-10-04',
            'original_body_sha256': provenance['original_sha256'],
            'input_locations': [provenance['original_snapshot'], capture['body']],
            'output_pdf_sha256': provenance['derivative_sha256'],
            'transformation': {'span_half_open_zero_based': provenance['source_span_half_open'],
                               'append_hex': provenance['appended_bytes_hex']},
            'saved_provenance': provenance, 'saved_capture_ledger_entry': capture,
            'historical_capture_times': None, 'historical_reconstruction_tool_version': None,
            'validation': 'both malformed body copies match; exact span plus suffix reproduces sibling PDF'},
        'unknowns_scope': 'unknowns describe only the finite bundle/envelopes and named supporting notes; '
            'no claim metadata is unrecoverable elsewhere',
        'verification': {'complete_reference_prepare': 'passed',
            'all_declared_hashes_and_manifest_membership': 'passed',
            'live_reference_pdf_and_page_projection': 'exact equality',
            'ingrasys_reconstruction_byte_check': 'passed',
            'all_pinned_input_hashes_unchanged': 'passed', 'network_requests': 0,
            'scope_limit': 'reference subset only; no whole-catalog integrity/rebuild/FTS/baseline rerun'}}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    result = inventory()
    if args.check:
        require(json.loads(DEST.read_bytes()) == result, 'Frozen inventory differs from workspace')
    else:
        with DEST.open('x') as output:
            json.dump(result, output, indent=2, ensure_ascii=False)
            output.write('\n')
    print(json.dumps({'counts': result['counts'], 'inventory': pin(DEST),
                      'verification': result['verification']}, indent=2))
