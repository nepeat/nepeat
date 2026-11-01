"""Immutable coverage manifests for offline campaign runs (c3a).

Offline validation proves receipts, artifacts, unit summaries and counts against the
archive. Run/definition metadata (identities, budgets, unknowns) is bound by the
content identity but can only be re-checked against the operational store, which
this module deliberately does not require.

A coverage manifest freezes what one terminal campaign run accounted for: per-unit
outcomes, declared scope and policy, the selected result receipts (exact wrappers)
and the artifact closure those receipts require. Its identity is the hash of its
canonical content. It makes no hardware claims: candidate bundles stay candidates,
failed/blocked/pending units are accounted for but not observed coverage, and an
unavailable or failed unit never establishes device absence. All-failed or
all-pending runs freeze honestly with zero receipts and zero artifacts.

Selection follows only the chosen run's roots, so unrelated archive content (even a
damaged pending object) neither enters the manifest nor blocks freezing.
"""
from __future__ import annotations
import argparse
import json
import os
from pathlib import Path
import sys
import tempfile

from .artifacts import _safe, canonical, digest, file_digest

SCHEMA = 'campaign-coverage/1'
CLAIMS_PREFIX = 'coverage only:'
# Wording may evolve, but these disclaimers must survive any edit.
CLAIM_INVARIANTS = ('not accepted catalog evidence', 'accounted for, not observed', 'nothing here asserts device absence')
CLAIMS = ('coverage only: candidate bundles are not accepted catalog evidence; failed, blocked, '
          'pending and unavailable units are accounted for, not observed; nothing here asserts '
          'device absence, regulator-wide coverage or a change feed')
UNKNOWNS = ['upstream regulator totals', 'legacy capture times and collector versions',
            'current upstream access and throttle state']
STAGE_REFS = {'campaign-bundle': 'validation_id', 'campaign-validate': 'registration_id'}
COUNTS = ('declared', 'inputs_registered', 'observed', 'validated_complete', 'explicit_zero',
          'failed', 'blocked', 'unknown', 'not_reached', 'budget_exhausted')
CHAIN = {'campaign-bundle': 'campaign-validate', 'campaign-validate': 'campaign-register'}
COMPLETE = ('validated_nonempty', 'explicit_zero')


class CoverageError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise CoverageError(message)


def identity(manifest):
    return 'coverage-' + digest(canonical({k: v for k, v in manifest.items() if k != 'manifest_id'}))


def _artifact_hashes(payload):
    return [item['sha256'] for item in payload.get('inputs', []) + payload.get('outputs', [])]


def build(runner, run_id):
    """Select one terminal run's roots and their exact closure. Read-only."""
    archive, state, definition = runner.archive, runner.state, runner.definition
    run = state.campaign_run(run_id)
    require(run['campaign_id'] == definition.campaign_id, 'run belongs to another campaign')
    require(run['state'] in ('complete', 'stopped'), 'only a terminal run can be frozen')
    require(run['definition_sha256'] == definition.sha256, 'run was made with a different definition')
    # Roots are what this run recorded, under the code identity it recorded; today's
    # code identity may differ and must not re-derive or re-target them.
    jobs = {j['id']: j for j in state.campaign_jobs(definition.campaign_id)}
    executed = {}
    for e in state.campaign_executions(run_id=run_id):
        job = jobs[e['job_id']]
        if job['stage'] == 'bundle':
            require(executed.get(job['unit_id'], e['result_id']) == e['result_id'], 'run recorded conflicting bundles')
            executed[job['unit_id']] = e['result_id']
    units, roots = [], []
    exhausted = bool(run['reason'] and run['reason'].startswith('budget_exhausted'))
    for unit in definition.units:
        entry = dict(unit_id=unit.unit_id, source=unit.source, unit_revision=unit.revision, scope=unit.scope)
        root = executed.get(unit.unit_id)
        if root is None:
            # Run-level accounting: a unit this run never reached is not claimed here,
            # even if an earlier run completed it (Runner.report is campaign-cumulative).
            units.append(dict(entry, outcome='budget_exhausted' if exhausted else 'not_reached', bundle_receipt=None,
                              coverage=None, comparison=None, candidate_sha256=None))
            continue
        payload = archive.verified_receipt(root)['wrapper']['payload']
        require(payload['unit_revision'] == unit.revision and payload['unit_id'] == unit.unit_id,
                'bundle receipt does not belong to this unit revision')
        roots.append(root)
        units.append(dict(entry, outcome=payload['outcome'], bundle_receipt=root, coverage=payload['coverage'],
                          comparison=payload.get('comparison'), candidate_sha256=payload.get('candidate_sha256')))
    # Closure: bundle -> validation -> registration, plus every referenced artifact.
    receipts, hashes, todo = {}, set(), list(roots)
    while todo:
        receipt_id = todo.pop()
        if receipt_id in receipts:
            continue
        proof = archive.verified_receipt(receipt_id)
        wrapper = proof['wrapper']
        receipts[receipt_id] = dict(id=receipt_id, kind=wrapper['kind'], wrapper_sha256=proof['wrapper_sha256'],
                                    wrapper=wrapper)
        hashes.update(_artifact_hashes(wrapper['payload']))
        parent = wrapper['payload'].get(STAGE_REFS.get(wrapper['kind'], ''))
        if parent:
            todo.append(parent)
    # Locations come from the frozen receipt payloads, never the mutable location
    # table, so registering another copy later cannot change this manifest.
    named = {}
    for receipt in receipts.values():
        payload = receipt['wrapper']['payload']
        for item in payload.get('inputs', []):
            named.setdefault(item['sha256'], set()).add(('workspace', item['path']))
        for item in payload.get('outputs', []):
            named.setdefault(item['sha256'], set()).add(
                ('archive', archive.object_path(item['sha256']).relative_to(archive.root).as_posix()))
    artifacts = []
    for sha in sorted(hashes):
        row = archive.db.execute('SELECT size FROM artifacts WHERE sha256=?', (sha,)).fetchone()
        require(row is not None, 'selected artifact is not registered: ' + sha)
        archive.resolve(sha)  # selected bytes must exist and verify now
        artifacts.append(dict(sha256=sha, byte_count=row['size'],
                              locations=[dict(scope=a, path=b) for a, b in sorted(named[sha])]))
    manifest = dict(
        schema=SCHEMA, campaign_id=definition.campaign_id,
        run=dict(id=run_id, state=run['state'], reason=run['reason'], identities=run['identities']),
        definition=dict(sha256=definition.sha256, inventory_sha256=definition.inventory_sha256,
                        version=definition.version, mode=definition.mode, budgets=definition.budgets),
        policy=dict(network_requests=0, acquisition='saved inputs only'),
        claims=CLAIMS, unknowns=UNKNOWNS, evidence_release=None,
        units=units, counts=_counts(units),
        receipts=[receipts[k] for k in sorted(receipts)], artifacts=artifacts)
    manifest['manifest_id'] = identity(manifest)
    return manifest


def _counts(units):
    counts = dict.fromkeys(COUNTS, 0)
    for unit in units:
        counts['declared'] += 1
        coverage = unit['coverage']
        if coverage is None:
            counts[unit['outcome']] += 1
            continue
        for key in ('inputs_registered', 'observed', 'validated_complete', 'explicit_zero'):
            counts[key] += bool(coverage.get(key))
        if unit['outcome'] in ('failed', 'blocked', 'unknown'):
            counts[unit['outcome']] += 1
    return counts


def validate(manifest, *, workspace=None, archive_root=None):
    """Self-contained checks; with roots given, every selected artifact must verify on disk."""
    require(manifest.get('schema') == SCHEMA, 'unsupported coverage manifest schema')
    require(manifest.get('manifest_id') == identity(manifest), 'coverage manifest identity mismatch')
    # Wording may evolve; the claim-free prefix and identity binding may not.
    require(workspace is None and archive_root is None or workspace is not None and archive_root is not None,
            'disk validation needs both the workspace and archive roots')
    require(isinstance(manifest.get('claims'), str) and manifest['claims'].startswith(CLAIMS_PREFIX)
            and all(text in manifest['claims'] for text in CLAIM_INVARIANTS)
            and manifest.get('evidence_release') is None and manifest.get('policy', {}).get('network_requests') == 0,
            'coverage manifest must stay claim-free and offline')
    receipts = {r['id']: r for r in manifest['receipts']}
    require(len(receipts) == len(manifest['receipts']), 'duplicate receipt')
    artifacts = {a['sha256']: a for a in manifest['artifacts']}
    require(len(artifacts) == len(manifest['artifacts']), 'duplicate artifact')
    referenced = set()
    for receipt_id, receipt in receipts.items():
        wrapper = receipt['wrapper']
        require(wrapper.get('id') == receipt_id and wrapper.get('kind') == receipt['kind'], 'receipt wrapper mismatch')
        require(digest(canonical(wrapper)) == receipt['wrapper_sha256'], 'receipt bytes changed: ' + receipt_id)
        hashes = _artifact_hashes(wrapper['payload'])
        require(set(hashes) <= set(artifacts), 'missing selected artifact for ' + receipt_id)
        referenced.update(hashes)
        parent = wrapper['payload'].get(STAGE_REFS.get(receipt['kind'], ''))
        if receipt['kind'] in STAGE_REFS:
            require(parent in receipts, 'missing selected receipt dependency for ' + receipt_id)
            parent_payload = receipts[parent]['wrapper']['payload']
            require(receipts[parent]['kind'] == CHAIN[receipt['kind']]
                    and parent_payload.get('unit_id') == wrapper['payload'].get('unit_id')
                    and parent_payload.get('unit_revision') == wrapper['payload'].get('unit_revision'),
                    'receipt chain crosses kinds or units at ' + receipt_id)
        else:
            require(receipt['kind'] == 'campaign-register', 'unexpected selected receipt kind: ' + receipt['kind'])
        if archive_root is not None:
            # The embedded wrapper must be the archive's exact immutable receipt bytes.
            try:
                stored = _safe(Path(archive_root), Path(archive_root) / 'receipts' / (receipt_id + '.json')).read_bytes()
            except (OSError, ValueError):
                stored = None
            require(stored == canonical(wrapper), 'embedded receipt differs from the archive: ' + receipt_id)
    require(referenced == set(artifacts), 'manifest carries unreferenced artifacts')
    # Stage references only point downstream -> upstream, so a walk must terminate.
    for start in receipts:
        seen, current = set(), start
        while current in receipts:
            require(current not in seen, 'cyclic receipt dependencies')
            seen.add(current)
            current = receipts[current]['wrapper']['payload'].get(STAGE_REFS.get(receipts[current]['kind'], ''))
    roots = set()
    for unit in manifest['units']:
        if unit['bundle_receipt'] is None:
            require(unit['outcome'] in ('not_reached', 'budget_exhausted') and unit['coverage'] is None,
                    'unit without a result must be not_reached or budget-exhausted')
            continue
        receipt = receipts.get(unit['bundle_receipt'])
        require(receipt is not None and receipt['kind'] == 'campaign-bundle', 'unit root is not a selected bundle')
        payload = receipt['wrapper']['payload']
        require(payload['unit_revision'] == unit['unit_revision'] and payload['outcome'] == unit['outcome']
                and payload['coverage'] == unit['coverage'] and payload.get('source') == unit['source']
                and payload.get('scope') == unit['scope'] and payload.get('comparison') == unit['comparison'],
                'unit summary disagrees with its receipt')
        require(payload['unit_id'] == unit['unit_id'] and payload['outcome'] in COMPLETE + ('failed', 'blocked', 'unknown'),
                'unit identity or outcome is not a recognised result')
        complete = unit['outcome'] in COMPLETE
        cov = unit['coverage']
        # Honesty rules: only complete results are observed/validated; zero only when proven.
        require(cov.get('declared') is True and cov.get('inputs_registered') is True
                and cov.get('observed') is complete and cov.get('validated_complete') is complete
                and cov.get('explicit_zero') is (unit['outcome'] == 'explicit_zero')
                and cov.get('upstream_total_known') is False and cov.get('upstream_total') is None,
                'coverage flags overclaim for ' + unit['unit_id'])
        require(unit['candidate_sha256'] == payload.get('candidate_sha256') and unit['candidate_sha256'] in artifacts,
                'candidate not selected')
        if workspace is not None and archive_root is not None:
            candidate = json.loads(_read_artifact(artifacts[unit['candidate_sha256']], workspace, archive_root))
            require(candidate.get('outcome') == unit['outcome'] and candidate.get('unit_revision') == unit['unit_revision'],
                    'candidate bundle disagrees with unit summary')
            if unit['outcome'] == 'explicit_zero':
                require(bool(candidate.get('zero_proof')) and not candidate.get('occurrence_ids'),
                        'explicit zero needs its saved proof and no occurrences')
        roots.add(unit['bundle_receipt'])
    reachable, todo = set(), list(roots)
    while todo:
        current = todo.pop()
        if current in reachable:
            continue
        reachable.add(current)
        parent = receipts[current]['wrapper']['payload'].get(STAGE_REFS.get(receipts[current]['kind'], ''))
        if parent:
            todo.append(parent)
    require(reachable == set(receipts), 'manifest carries receipts outside the selected closure')
    require(manifest['counts'] == _counts(manifest['units']), 'counts disagree with unit outcomes')
    if workspace is not None and archive_root is not None:
        for artifact in artifacts.values():
            _read_artifact(artifact, workspace, archive_root, verify_only=True)
    return dict(manifest_id=manifest['manifest_id'], receipts=len(receipts), artifacts=len(artifacts),
                counts=manifest['counts'])


def _read_artifact(artifact, workspace, archive_root, verify_only=False):
    for location in artifact['locations']:
        base = Path(archive_root) if location['scope'] == 'archive' else Path(workspace)
        try:
            path = _safe(base, location['path'])
            if path.is_file() and path.stat().st_size == artifact['byte_count'] and file_digest(path) == artifact['sha256']:
                return None if verify_only else path.read_bytes()
        except (OSError, ValueError):
            continue
    raise CoverageError('selected artifact missing or corrupt: ' + artifact['sha256'])


def load(path):
    """Strict reader: canonical bytes only, so duplicate keys or edits cannot hide."""
    raw = Path(path).read_bytes()
    def unique(pairs):
        keys = [k for k, _ in pairs]
        require(len(keys) == len(set(keys)), 'duplicate JSON key')
        return dict(pairs)
    def constant(name):
        raise CoverageError('non-finite JSON number: ' + name)
    try:
        manifest = json.loads(raw, object_pairs_hook=unique, parse_constant=constant)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise CoverageError('unreadable coverage manifest') from exc
    require(raw == canonical(manifest) + b'\n', 'coverage manifest file is not canonical')
    return manifest


def publish(manifest, directory, *, workspace, archive_root):
    """Validate an unpublished sibling, then exclusively link it as <manifest_id>.json."""
    directory = Path(directory)
    if not directory.is_dir():
        directory.mkdir(parents=True)
        parent = os.open(directory.parent, os.O_RDONLY)
        try:
            os.fsync(parent)
        finally:
            os.close(parent)
    destination = directory / (manifest['manifest_id'] + '.json')
    require(not destination.exists(), 'coverage manifest already published: ' + destination.name)
    fd, name = tempfile.mkstemp(prefix='.coverage-', suffix='.json', dir=directory)
    temporary = Path(name)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(canonical(manifest) + b'\n')
            stream.flush()
            os.fsync(stream.fileno())
        validate(load(temporary), workspace=workspace, archive_root=archive_root)
        os.chmod(temporary, 0o444)  # immutable once published, like archive objects
        os.link(temporary, destination)
        handle = os.open(directory, os.O_RDONLY)
        try:
            os.fsync(handle)
        finally:
            os.close(handle)
    finally:
        temporary.unlink()
    return destination


def main(argv=None):
    from .campaign import DEFAULT_DEFINITION, DEFAULT_INVENTORY, open_runner
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workspace', type=Path, default=Path.cwd())
    parser.add_argument('--archive', type=Path, default=Path('data/archive'))
    parser.add_argument('--definition', default=DEFAULT_DEFINITION)
    parser.add_argument('--inventory', default=DEFAULT_INVENTORY)
    commands = parser.add_subparsers(dest='command', required=True)
    freeze = commands.add_parser('freeze', help='freeze one terminal run; writes only the manifest file')
    freeze.add_argument('run_id')
    freeze.add_argument('--output', type=Path, required=True, help='directory for <manifest_id>.json')
    check = commands.add_parser('validate', help='validate a published manifest and its selected bytes')
    check.add_argument('manifest', type=Path)
    args = parser.parse_args(argv)
    workspace = args.workspace.absolute()
    archive_root = args.archive if args.archive.is_absolute() else workspace / args.archive
    if args.command == 'validate':
        # Always verify against both roots; there is no silent structural-only mode here.
        manifest = load(args.manifest)
        require(args.manifest.name == manifest['manifest_id'] + '.json', 'manifest file name differs from its identity')
        result = validate(manifest, workspace=workspace, archive_root=archive_root)
    else:
        runner = open_runner(workspace, archive_root, args.definition, args.inventory, readonly=True)
        try:
            manifest = build(runner, args.run_id)
            path = publish(manifest, args.output, workspace=workspace, archive_root=runner.archive.root)
        finally:
            runner.archive.close()
        result = dict(validate(manifest), path=str(path))
    json.dump(result, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write('\n')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
