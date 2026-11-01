"""Local archive operations, independent of the accepted hardware catalog."""
import argparse
import json
from pathlib import Path

from .artifacts import Archive, canonical, digest, file_digest
from .derivations import Derivations
from .reference_documents import prepare
from . import lineage


def stable_receipt(archive, kind, payload):
    return archive.receipt(kind, payload, kind + '-' + digest(canonical(payload)))


def register_reference(archive, inventory_path):
    """Register exactly a frozen complete collection; preserve legacy unknowns."""
    inventory = json.loads(Path(inventory_path).read_bytes())
    pins = [inventory['pins']['complete_bundle'], *inventory['dependencies'],
            *inventory['supporting_provenance']]
    # Validate the entire finite set before any registration.
    for pin in pins:
        path = lineage.local(archive.workspace, pin['path'])
        lineage.require(path.stat().st_size == pin['byte_count'] and
                        file_digest(path) == pin['sha256'], 'Pinned inventory changed')
    bundle = archive.workspace / inventory['pins']['complete_bundle']['path']
    _, ready = prepare(bundle)
    # Inventory pins describe the raw bundle, not the validated metadata that
    # prepare attaches to its entry copies. Keep legacy receipt identities.
    raw_entries = json.loads(bundle.read_bytes())['documents']
    lineage.require(len(raw_entries) == len(ready)
                    and raw_entries == [d['entry'] for d in inventory['documents']],
                    'Inventory is not the complete selected bundle')
    for pin in pins:
        archive.register_file(archive.workspace / pin['path'], pin['sha256'])
    derivations = Derivations(archive)
    document_ids = []
    for ordinal, ((entry, _), raw_entry) in enumerate(zip(ready, raw_entries)):
        capture = dict(source_url=entry['url'], bundle_sha256=inventory['pins']['complete_bundle']['sha256'],
                       locator='/documents/' + str(ordinal), started_at=None, ended_at=None,
                       collector_version=None, http_status=None, requested_url=None, final_url=None,
                       metadata_status='legacy_provenance_unknown', artifact_sha256=entry['pdf']['sha256'])
        if entry['id'] == inventory['special_lineage']['document_id']:
            special = inventory['special_lineage']
            capture['artifact_sha256'] = special['original_body_sha256']
            capture['representation'] = 'malformed_saved_response'
            capture_id = stable_receipt(archive, 'legacy-capture', capture)
            fresh = dict(capture, saved_capture_ledger=special['saved_capture_ledger_entry'],
                         original_location=special['input_locations'][1],
                         http_status=special['saved_capture_ledger_entry']['status'])
            fresh_id = stable_receipt(archive, 'legacy-capture', fresh)
            reconstruction_id = derivations.register_saved(
                [{'role': 'malformed-original', 'sha256': special['original_body_sha256']}],
                entry['pdf']['sha256'], special['saved_provenance']['transform'],
                locator=special['transformation'])
            ancestor_id = stable_receipt(archive, 'reconstruction-lineage', {
                'capture_ids': [capture_id, fresh_id], 'derivation_id': reconstruction_id,
                'artifact_sha256': entry['pdf']['sha256'], 'original_sha256': special['original_body_sha256'],
                'provenance': special['saved_provenance']})
        else:
            capture['representation'] = 'legacy_pdf_bytes; acquisition representation unknown'
            capture_id = stable_receipt(archive, 'legacy-capture', capture)
            ancestor_id = capture_id
        saved_id = derivations.register_saved([{'role': 'pdf', 'sha256': entry['pdf']['sha256']}],
                    entry['text']['sha256'], entry['extraction_method'],
                    locator={'bundle_sha256': capture['bundle_sha256'], 'pointer': capture['locator']})
        document_ids.append(stable_receipt(archive, 'reference-document', {
            'entry': raw_entry, 'capture_id': ancestor_id, 'derivation_id': saved_id,
            'artifact_sha256': entry['pdf']['sha256'], 'text_sha256': entry['text']['sha256']}))
    return {'documents': len(document_ids), 'document_receipts': document_ids, 'audit': archive.audit()}


def receipt_references(receipt, all_ids, all_hashes):
    """Only explicit data/reference fields; never treat a code/config hash as bytes."""
    artifacts, receipts = set(), set()
    artifact_keys = {'sha256', 'artifact_sha256', 'text_sha256', 'response_sha256',
                     'original_sha256', 'bundle_sha256'}
    receipt_keys = {'recipe_id', 'run_id', 'capture_id', 'prior_capture_id', 'attempt_id',
                    'derivation_id', 'capture_ids'}
    def visit(value, key=None):
        if isinstance(value, dict):
            for k, v in value.items():
                visit(v, k)
        elif isinstance(value, list):
            for v in value:
                visit(v, key)
        elif isinstance(value, str):
            if key in artifact_keys and value in all_hashes:
                artifacts.add(value)
            if key in receipt_keys and value:
                lineage.require(value in all_ids, 'Missing durable receipt dependency')
                receipts.add(value)
    visit(receipt['payload'])
    payload, kind = receipt['payload'], receipt['kind']
    required = []
    if kind == 'artifact-registration':
        required.append(payload['sha256'])
    elif kind in ('saved-derivation', 'derivation-run'):
        required.extend(i['sha256'] for i in payload['inputs'] + payload['outputs'])
    elif kind == 'legacy-capture':
        required.extend(payload[k] for k in ('artifact_sha256', 'bundle_sha256'))
    elif kind == 'reference-document':
        required.extend([payload['artifact_sha256'], payload['text_sha256'],
                         payload['entry']['pdf']['sha256'], payload['entry']['text']['sha256']])
    elif kind == 'reconstruction-lineage':
        required.extend([payload['artifact_sha256'], payload['original_sha256']])
    elif kind in ('capture_result', 'capture_reuse'):
        required.extend(payload[k] for k in ('artifact_sha256', 'response_sha256') if payload.get(k))
    lineage.require(set(required) <= all_hashes, 'Missing core receipt artifact dependency')
    artifacts.update(required)
    return sorted(artifacts), sorted(receipts)


def freeze(archive, destination, statements=(), reviews=(), links=()):
    """Snapshot durable metadata and bytes; no dependency on later registry state."""
    destination = Path(destination).absolute()
    root = destination.parent
    lineage.require(not destination.exists(), 'Release path already exists')
    audit = archive.audit()
    lineage.require(audit['ok'], 'Cannot freeze missing or corrupt archive locations')
    # Accepted releases use data-relative paths. Supplemental research receipts are
    # copied into the object store without changing their registered original path.
    for row in list(archive.db.execute('SELECT DISTINCT sha256 FROM artifacts')):
        path = archive.resolve(row[0])
        if not path.is_relative_to(root):
            archive.store_bytes(path.read_bytes())
    artifacts = []
    for obj in archive.db.execute('SELECT sha256,size FROM artifacts ORDER BY sha256'):
        paths = []
        for loc in archive.db.execute('SELECT scope,path FROM locations WHERE sha256=? ORDER BY scope,path', (obj[0],)):
            path = (archive.root if loc[0] == 'archive' else archive.workspace) / loc[1]
            if path.is_relative_to(root):
                paths.append(path.relative_to(root).as_posix())
        lineage.require(bool(paths), 'No release-relative artifact location')
        artifacts.append({'sha256': obj[0], 'byte_count': obj[1], 'paths': sorted(set(paths))})
    receipts = archive.receipts()
    all_ids, all_hashes = {r['id'] for r in receipts}, {a['sha256'] for a in artifacts}
    for receipt in receipts:
        refs, parents = receipt_references(receipt, all_ids, all_hashes)
        receipt.update(artifact_refs=refs, receipt_refs=parents)
    result = {'schema_version': 1, 'scope': 'Frozen finite evidence release; candidate extraction is not a hardware claim',
              'artifacts': artifacts, 'receipts': receipts, 'statements': list(statements),
              'reviews': list(reviews), 'links': list(links)}
    result['release_id'] = lineage.identity(result)
    # Validate an unpublished temporary sibling before exclusive publication.
    import os
    import tempfile
    fd, name = tempfile.mkstemp(prefix='.release-', suffix='.json', dir=root)
    temporary = Path(name)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(canonical(result) + b'\n')
            stream.flush()
            os.fsync(stream.fileno())
        lineage.prepare(temporary)
        os.link(temporary, destination)
        directory = os.open(root, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        temporary.unlink()
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path('data/archive'))
    parser.add_argument('--workspace', type=Path, default=Path.cwd())
    commands = parser.add_subparsers(dest='command', required=True)
    commands.add_parser('init')
    commands.add_parser('audit')
    for name in ('upgrade','recover','reindex'):
        operation = commands.add_parser(name)
        operation.add_argument('--decision',required=True)
        if name != 'recover':
            operation.add_argument('--quiesced',action='store_true',required=True)
    register = commands.add_parser('register-reference')
    register.add_argument('inventory', type=Path)
    derive = commands.add_parser('derive')
    derive.add_argument('recipe', choices=['page-text-flatten', 'xml-locations'])
    derive.add_argument('sha256')
    derive.add_argument('--consumer', required=True)
    args = parser.parse_args(argv)
    if args.command in ('upgrade','recover','reindex'):
        operation = getattr(Archive,'rebuild' if args.command=='reindex' else args.command)
        keywords = {'decision':args.decision}
        if args.command != 'recover':
            keywords['quiesced'] = args.quiesced
        result = operation(args.root,args.workspace,**keywords)
        print(json.dumps(result,indent=2))
        return 0
    archive = Archive(args.root, args.workspace, create=args.command == 'init',
                      readonly=args.command == 'audit')
    try:
        if args.command == 'register-reference':
            result = register_reference(archive, args.inventory)
        elif args.command == 'derive':
            derivations = Derivations(archive)
            result = derivations.run(derivations.recipe(args.recipe, '1'),
                    [{'role': 'source', 'sha256': args.sha256}], consumer=args.consumer)
        else:
            result = archive.audit()
        print(json.dumps(result, indent=2))
        return 0 if result.get('ok', True) else 1
    finally:
        archive.close()


if __name__ == '__main__':
    raise SystemExit(main())
