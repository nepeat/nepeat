"""Review queue, selected evidence release and import gate for campaign units (c3b).

Whole-projection review: importers replace whole sources, so a reviewer accepts or
rejects one unit's complete candidate projection. Partial acceptance is a rejection.

A projection is everything one source contributes to the catalog: its `sources` row,
its records (deterministic device id, raw JSON, affiliations, identifiers, facts,
hyperscaler evidence) and its raw regulator rows. Environment metadata (fetched_at,
cache_path, observed_at) and the device row's printed brand/model/title/kind are
excluded: that row keeps the spelling of whichever source created the device first,
while the record's own literal stays in its raw JSON. Diffs pair records by content identity, never by the
positional source_key, so a reordered snapshot shows as moved, not changed.

The chain is immutable: queue item -> decision (binds the item id) -> release (binds
items and decisions). Rebuild reads only the release file, never this queue or the
archive. Units without an accepted decision keep their baseline inputs and projection.
"""
from __future__ import annotations
import argparse
import csv
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile

from .artifacts import canonical, digest, file_digest

ITEM_SCHEMA = 'campaign-review-item/1'
DECISION_SCHEMA = 'campaign-review-decision/1'
RELEASE_SCHEMA = 'campaign-evidence-release/1'
RAW_TABLES = {'rra': 'rra_equipment', 'ised': 'ised_equipment'}
REVIEWABLE = ('validated_nonempty', 'explicit_zero')
DECISIONS = ('accepted', 'rejected')
CLAIMS = '.claims'
RELEASE_CLAIMS = ('selected evidence release: accepted entries are reviewed whole-source projections; '
                  'baseline entries keep previously accepted inputs; nothing here asserts device '
                  'absence, regulator-wide coverage or independent corroboration across queries')
CERTIFICATE_KINDS = ('rra_certificate_id', 'ic_id')
RAW_CERTIFICATE_FIELDS = ('certificate_number', 'certification_number')


class ReviewError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise ReviewError(message)


def _with_id(document, key, prefix):
    body = {k: v for k, v in document.items() if k != key}
    return dict(body, **{key: prefix + digest(canonical(body))})


def _check_id(document, key, prefix):
    body = {k: v for k, v in document.items() if k != key}
    require(document.get(key) == prefix + digest(canonical(body)), key + ' does not match content')


def load(path):
    """Strict canonical reader: duplicate keys, NaN or reformatting are refused."""
    raw = Path(path).read_bytes()
    def unique(pairs):
        keys = [k for k, _ in pairs]
        require(len(keys) == len(set(keys)), 'duplicate JSON key')
        return dict(pairs)
    def constant(name):
        raise ReviewError('non-finite JSON number: ' + name)
    try:
        value = json.loads(raw, object_pairs_hook=unique, parse_constant=constant)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ReviewError('unreadable review document: ' + Path(path).name) from exc
    require(raw == canonical(value) + b'\n', 'review document is not canonical: ' + Path(path).name)
    return value


def _publish(document, directory, name):
    """Exclusive, read-only publication; identical bytes may already exist."""
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    destination = directory / name
    data = canonical(document) + b'\n'
    if destination.exists():
        require(destination.read_bytes() == data, 'conflicting document already published: ' + name)
        return destination
    fd, temporary = tempfile.mkstemp(prefix='.review-', suffix='.json', dir=directory)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, 0o444)
        try:
            os.link(temporary, destination)
        except FileExistsError:
            require(destination.read_bytes() == data, 'conflicting document already published: ' + name)
    finally:
        os.unlink(temporary)
    handle = os.open(directory, os.O_RDONLY)
    try:
        os.fsync(handle)
    finally:
        os.close(handle)
    return destination


# -- projections ---------------------------------------------------------------------

def projection(db, source_id, raw_table):
    """Canonical projection of one source, or None when the catalog lacks it."""
    require(raw_table in RAW_TABLES.values(), 'unsupported raw table: ' + str(raw_table))
    row = db.execute('SELECT url,tier,license,coverage,sha256,upstream_revision,records_seen '
                     'FROM sources WHERE id=?', (source_id,)).fetchone()
    if row is None:
        return None
    source = dict(zip(('url', 'tier', 'license', 'coverage', 'sha256', 'upstream_revision', 'records_seen'), row))
    records = []
    for rid, key, url, raw, device in db.execute(
            'SELECT id,source_key,source_url,raw_json,device_id FROM records WHERE source_id=? ORDER BY source_key',
            (source_id,)):
        records.append(dict(
            source_key=key, source_url=url, raw=json.loads(raw), device_id=device,
            affiliations=sorted([list(r) for r in db.execute(
                'SELECT company,target,role FROM affiliations WHERE record_id=?', (rid,))], key=canonical),
            identifiers=sorted([list(r) for r in db.execute(
                'SELECT kind,value FROM identifiers WHERE record_id=?', (rid,))]),
            facts=sorted([list(r) for r in db.execute('SELECT name,value FROM facts WHERE record_id=?', (rid,))]),
            hyperscaler=sorted([list(r) for r in db.execute(
                'SELECT operator,relationship,detail FROM hyperscaler_evidence WHERE record_id=?', (rid,))])))
    cursor = db.execute('SELECT * FROM "' + raw_table + '" WHERE source_id=? ORDER BY row_number', (source_id,))
    names = [d[0] for d in cursor.description]
    rows = [{k: v for k, v in zip(names, values) if k != 'source_id'} for values in cursor]
    return dict(source=source, records=records, raw_rows=rows)


def projection_sha256(value):
    return None if value is None else digest(canonical(value))


def catalog_projection(path, source_id, raw_table):
    with closing(sqlite3.connect(Path(path).resolve().as_uri() + '?mode=ro', uri=True)) as db:
        return projection(db, source_id, raw_table)


def candidate_projection(source, manifest_path, source_id):
    """Run the existing importer into an isolated empty scratch catalog."""
    from .catalog import Catalog
    from .pipeline import import_source
    with tempfile.TemporaryDirectory(prefix='.review-scratch-') as work:
        scratch = Catalog(Path(work) / 'candidate.sqlite')
        try:
            import_source(scratch, source, Path(manifest_path))
            scratch.db.commit()
            sources = [r[0] for r in scratch.db.execute('SELECT id FROM sources')]
            require(sources == [source_id], 'candidate import produced unexpected sources: ' + repr(sources))
            return projection(scratch.db, source_id, RAW_TABLES[source])
        finally:
            scratch.db.close()


def _identity(record):
    return canonical([record['device_id'], record['identifiers']])


def _content(record):
    return {k: v for k, v in record.items() if k != 'source_key'}


def diff(baseline, candidate):
    """Content-matched changes. Duplicate identities pair exact content first, then in order."""
    base = (baseline or {}).get('records', [])
    cand = (candidate or {}).get('records', [])
    groups = {}
    for side, records in (('base', base), ('cand', cand)):
        for record in records:
            groups.setdefault(_identity(record), dict(base=[], cand=[]))[side].append(record)
    added, removed, changed, moved, unchanged = [], [], [], [], 0
    for identity in sorted(groups):
        b, c = list(groups[identity]['base']), list(groups[identity]['cand'])
        for record in list(c):
            match = next((x for x in b if _content(x) == _content(record)), None)
            if match is None:
                continue
            same = [x for x in b if _content(x) == _content(record) and x['source_key'] == record['source_key']]
            match = same[0] if same else match
            b.remove(match)
            c.remove(record)
            if match['source_key'] == record['source_key']:
                unchanged += 1
            else:
                moved.append(dict(identity=json.loads(identity), from_key=match['source_key'], to_key=record['source_key']))
        for old, new in zip(b, c):
            fields = sorted(k for k in set(_content(old)) | set(_content(new)) if old.get(k) != new.get(k))
            changed.append(dict(identity=json.loads(identity), from_key=old['source_key'], to_key=new['source_key'],
                                fields={k: dict(before=old.get(k), after=new.get(k)) for k in fields}))
        removed += b[len(c):]
        added += c[len(b):]
    def rows(value):
        return [canonical({k: v for k, v in row.items() if k != 'row_number'}) for row in (value or {}).get('raw_rows', [])]
    old_rows, new_rows = rows(baseline), rows(candidate)
    remaining = list(old_rows)
    raw_added = []
    for row in new_rows:
        if row in remaining:
            remaining.remove(row)
        else:
            raw_added.append(json.loads(row))
    source_fields = sorted(k for k in set((baseline or {}).get('source', {})) | set((candidate or {}).get('source', {}))
                           if (baseline or {}).get('source', {}).get(k) != (candidate or {}).get('source', {}).get(k))
    return dict(records_added=added, records_removed=removed, records_changed=changed, records_moved=moved,
                records_unchanged=unchanged,
                raw_rows_added=raw_added, raw_rows_removed=[json.loads(r) for r in remaining],
                raw_rows_reordered=old_rows != new_rows and sorted(old_rows) == sorted(new_rows),
                raw_row_numbers_changed=[r['row_number'] for r in (baseline or {}).get('raw_rows', [])]
                                        != [r['row_number'] for r in (candidate or {}).get('raw_rows', [])],
                source_fields_changed={k: dict(before=(baseline or {}).get('source', {}).get(k),
                                               after=(candidate or {}).get('source', {}).get(k)) for k in source_fields})


def no_changes(changes):
    return not any(changes[k] for k in ('records_added', 'records_removed', 'records_changed', 'records_moved',
                                         'raw_rows_added', 'raw_rows_removed', 'raw_rows_reordered',
                                         'raw_row_numbers_changed', 'source_fields_changed'))


def _certificates(value):
    found = {}
    for record in (value or {}).get('records', []):
        for kind, ident in record['identifiers']:
            if kind in CERTIFICATE_KINDS:
                found.setdefault(ident, set()).add(record['device_id'])
    return found


def _lineage_statements(db, source_id):
    try:
        rows = db.execute('SELECT statement_id,source_key,projection_kind,projection_json FROM evidence_statements '
                          'WHERE source_id=? ORDER BY statement_id', (source_id,)).fetchall()
    except sqlite3.OperationalError:
        return []
    return [dict(statement_id=r[0], source_key=r[1], kind=r[2], projection=json.loads(r[3])) for r in rows]


def _lineage_conflicts(statements, candidate):
    """Accepted lineage links by (source_key, value); a moved or dropped key would dangle."""
    conflicts = []
    by_key = {r['source_key']: r for r in (candidate or {}).get('records', [])}
    for s in statements:
        record = by_key.get(s['source_key'])
        p = s['projection']
        present = record is not None and (
            [p.get('name'), p.get('value')] in record['facts'] if s['kind'] == 'fact'
            else any(a[0] == p.get('company') and a[2] == p.get('role') for a in record['affiliations']))
        if not present:
            conflicts.append(dict(kind='accepted_lineage_would_dangle', statement_id=s['statement_id'],
                                  source_key=s['source_key']))
    return conflicts


# -- queue -----------------------------------------------------------------------------

def _duplicate_occurrence_conflicts(candidate):
    """Records sharing one identity (device_id + identifiers) stay separate records but are
    repeated listings of the same thing, never independent corroboration."""
    groups = {}
    for record in (candidate or {}).get('records', []):
        groups.setdefault(_identity(record), []).append(record)
    return [dict(kind='duplicate_occurrence', device_id=group[0]['device_id'], identifiers=group[0]['identifiers'],
                 source_keys=[r['source_key'] for r in group], count=len(group),
                 note='preserved as separate records; not independent corroboration')
            for _, group in sorted(groups.items()) if len(group) > 1]


def accepted_release(accepted_manifest, accepted):
    """The accepted manifest's gate release (hash-verified, validated) or None without a gate.

    Resolved exactly like rebuild.read_manifest resolves manifest files: relative to the
    manifest's directory, never escaping it, hashed in the manifest's `files`.
    """
    gate = accepted.get('campaign_gate')
    if gate is None:
        return None
    from .rebuild import local_path
    require(isinstance(gate, dict) and isinstance(gate.get('release'), str), 'accepted manifest campaign_gate names no release')
    name = gate['release']
    require(name in accepted['files'], 'accepted manifest does not hash its campaign release')
    path = local_path(Path(accepted_manifest).resolve().parent, name)
    require(file_digest(path) == accepted['files'][name], 'accepted campaign release differs from its manifest hash')
    # The accepted manifest must itself pass its own gate (release/coverage pairing, imported jobs,
    # input hashes). The pairing is the accepted release's own, not the new queue's coverage manifest.
    release = gate_inputs(Path(accepted_manifest).resolve().parent, accepted)
    accepted_jobs = {(j['source'], j['path']) for j in accepted['imports']}
    for e in release['entries']:
        if e['status'] == 'excluded':
            continue
        require((e['job']['source'], e['job']['path']) in accepted_jobs,
                'accepted manifest does not import the released job of ' + e['unit_id'])
        for i in e['inputs']:
            require(accepted['files'].get(i['path']) == i['sha256'],
                    'accepted release input differs from the accepted manifest: ' + i['path'])
    return release


def _archive_root(workspace, archive_root):
    if archive_root is not None:
        return Path(archive_root) if Path(archive_root).is_absolute() else workspace / archive_root
    # data/archive wins when both exist; workspace/archive is the fixture layout.
    for candidate in (workspace / 'data' / 'archive', workspace / 'archive'):
        if candidate.is_dir():
            return candidate
    return None


def check_candidate_bundle(unit, entry, manifest, workspace, archive_root, reviewable):
    """Tie a unit's candidate sha to the bytes its projection is built from.

    `candidate_sha256` hashes the archived candidate-bundle JSON (outcome, scope, zero proof,
    occurrence ids, record-set hashes) derived from the saved manifest by the campaign adapter. It
    is not one of `unit.files`, so it cannot be compared to a file hash. Instead: (1) the bundle
    receipt in the coverage manifest must be a bundle for this unit whose payload and candidate
    output name that same sha; (2) for reviewable units the archived bundle bytes must hash to it and
    equal what the adapter derives *now* from the very manifest `candidate_projection` imports
    (pinned hashes are verified first), so a bundle for other bytes cannot ride along.
    Failed/not-reached outcomes may carry no sha; nothing is projected for them.
    """
    from . import coverage
    from .campaign_sources import ADAPTERS
    name, sha, label = entry.get('bundle_receipt'), entry.get('candidate_sha256'), unit.unit_id
    if name is None:
        require(sha is None and not reviewable, 'reviewable unit has no bundle receipt: ' + label)
        return
    receipt = {r['id']: r for r in manifest['receipts']}.get(name)
    require(receipt is not None and receipt['kind'] == 'campaign-bundle'
            and digest(canonical(receipt['wrapper'])) == receipt['wrapper_sha256'], 'bundle receipt not selected: ' + label)
    payload = receipt['wrapper']['payload']
    require(isinstance(payload, dict) and payload.get('outcome') == entry.get('outcome'),
            'bundle receipt outcome differs from the unit outcome: ' + label)
    outputs = [o['sha256'] for o in payload.get('outputs', []) if o.get('role') == 'candidate-bundle']
    require(payload.get('unit_id') == unit.unit_id and payload.get('unit_revision') == unit.revision
            and payload.get('candidate_sha256') == sha and outputs == ([] if sha is None else [sha]),
            'bundle receipt does not name the recorded candidate sha: ' + label)
    if not reviewable:
        return
    require(isinstance(sha, str) and len(sha) == 64, 'reviewable unit has no candidate sha: ' + label)
    artifact = {a['sha256']: a for a in manifest['artifacts']}.get(sha)
    require(artifact is not None, 'candidate bundle is not a selected artifact: ' + label)
    root = _archive_root(workspace, archive_root)
    require(root is not None, 'candidate bundle needs the archive to cross-check: ' + label)
    try:
        stored = coverage._read_artifact(artifact, workspace, root)
    except coverage.CoverageError as exc:
        raise ReviewError('candidate bundle unreadable: %s (%s)' % (label, exc)) from None
    require(digest(stored) == sha, 'candidate bundle bytes do not hash to the recorded sha: ' + label)
    try:
        bundle = json.loads(stored)
    except ValueError:
        bundle = None
    require(isinstance(bundle, dict), 'candidate bundle is not a JSON object: ' + label)
    try:
        output = ADAPTERS[unit.source].records(workspace / unit.manifest, unit.manifest)
    except (ValueError, OSError, csv.Error) as exc:
        raise ReviewError('cannot re-derive candidate bundle for %s: %s' % (label, exc)) from None
    for occurrence in output['occurrences']:
        occurrence['occurrence_id'] = 'occ-' + digest(canonical([unit.revision, occurrence['logical_sha256'],
                                                                  occurrence['kind'], occurrence['selector']]))
    derived = dict(unit_id=unit.unit_id, unit_revision=unit.revision, outcome=entry['outcome'],
                   scope=output.get('scope', unit.scope), zero_proof=output.get('zero_proof'),
                   occurrence_ids=[o['occurrence_id'] for o in output['occurrences']],
                   occurrences_sha256=digest(canonical([o['logical_sha256'] for o in output['occurrences']])),
                   record_sets=output.get('record_sets', {}))
    require(output['outcome'] == entry['outcome'] and all(bundle.get(k) == v for k, v in derived.items()),
            'candidate projection input differs from the bundle the coverage manifest names: ' + label)


def build_queue(workspace, catalog, coverage_manifest, inventory, output, accepted_manifest, archive_root=None):
    """One immutable item per declared unit of the coverage manifest's run.

    Baseline job and inputs are what the accepted manifest's gate release recorded for the
    unit (the job may live at a different path than the candidate's); without a gate they
    are the accepted manifest's hashes for the unit's own job path. Candidate inputs are
    the campaign's pinned hashes. They differ exactly when the campaign observed changed bytes.
    """
    from . import coverage
    from .campaign import load_definition
    workspace = Path(workspace).absolute()
    manifest = coverage.load(coverage_manifest)
    require(manifest['manifest_id'] == coverage.identity(manifest), 'coverage manifest identity mismatch')
    try:  # structural, offline: receipt wrappers/chains, claims, counts (bytes are checked per unit below)
        coverage.validate(manifest)
    except coverage.CoverageError as exc:
        raise ReviewError('coverage manifest invalid: %s' % exc) from None
    definition = load_definition(workspace, inventory['definition'], inventory['inventory'])
    require(manifest['campaign_id'] == definition.campaign_id, 'coverage manifest belongs to another campaign')
    require(manifest['definition']['sha256'] == definition.sha256,
            'coverage manifest was frozen under a different definition revision')
    accepted = json.loads(Path(accepted_manifest).read_bytes())
    accepted_files, accepted_jobs = accepted['files'], {(j['source'], j['path']) for j in accepted['imports']}
    gated = accepted_release(accepted_manifest, accepted)
    released = None if gated is None else {e['unit_id']: e for e in gated['entries']}
    covered = {u['unit_id']: u for u in manifest['units']}
    require(set(covered) == {u.unit_id for u in definition.units}, 'coverage manifest does not cover every unit')
    projections, items = {}, []
    with closing(sqlite3.connect(Path(catalog).resolve().as_uri() + '?mode=ro', uri=True)) as live:
        for unit in definition.units:
            entry = covered[unit.unit_id]
            require(entry.get('unit_revision') == unit.revision, 'unit revision differs from coverage: ' + unit.unit_id)
            raw_table = RAW_TABLES[unit.source]
            require(all(p.startswith('data/') for p, _ in unit.files), 'unit inputs must live under data/')
            inputs = [dict(path=path[len('data/'):], sha256=sha) for path, sha in unit.files]
            job = dict(source=unit.source, path=unit.manifest[len('data/'):])
            baseline_inputs = baseline_job = None
            if released is not None and unit.unit_id in released:
                prior = released[unit.unit_id]
                require(prior['adapter'] == unit.source, 'accepted release names another adapter for ' + unit.unit_id)
                if prior['status'] != 'excluded':
                    baseline_job = dict(prior['job'])
                    baseline_inputs = [dict(path=i['path'], sha256=i['sha256']) for i in prior['inputs']]
            elif (job['source'], job['path']) in accepted_jobs:
                baseline_job = dict(job)
                require(all(i['path'] in accepted_files for i in inputs),
                        'accepted manifest does not hash every pinned input of ' + unit.unit_id)
                baseline_inputs = [dict(path=i['path'], sha256=accepted_files[i['path']]) for i in inputs]
            baseline = projection(live, unit.unit_id, raw_table)
            reviewable = entry['outcome'] in REVIEWABLE
            candidate = changes = None
            if not reviewable:
                check_candidate_bundle(unit, entry, manifest, workspace, archive_root, False)
            if reviewable:
                for path, sha in unit.files:
                    require(file_digest(workspace / path) == sha, 'pinned input changed: ' + path)
                check_candidate_bundle(unit, entry, manifest, workspace, archive_root, True)
                candidate = candidate_projection(unit.source, workspace / unit.manifest, unit.unit_id)
                for path, sha in unit.files:  # close the read window: inputs unchanged while projecting
                    require(file_digest(workspace / path) == sha, 'pinned input changed during projection: ' + path)
                changes = diff(baseline, candidate)
            projections[unit.unit_id] = candidate
            conflicts = []
            for cert, models in sorted(_certificates(candidate).items()):
                if len(models) > 1:
                    conflicts.append(dict(kind='repeated_certificate_distinct_devices', certificate=cert,
                                          device_ids=sorted(models), note='distinct hardware versions; preserve separately'))
            for record in (candidate or {}).get('records', []):
                for name, value in record['facts']:
                    if name == 'source_discrepancy':
                        conflicts.append(dict(kind='html_csv_discrepancy', source_key=record['source_key'], detail=value))
            conflicts += _duplicate_occurrence_conflicts(candidate)
            conflicts += _lineage_conflicts(_lineage_statements(live, unit.unit_id), candidate) if reviewable else []
            item = dict(schema=ITEM_SCHEMA, campaign_id=definition.campaign_id, definition_sha256=definition.sha256,
                        coverage_manifest_id=manifest['manifest_id'], unit_id=unit.unit_id, unit_revision=unit.revision,
                        adapter=unit.source, source_id=unit.unit_id, outcome=entry['outcome'],
                        bundle_receipt=entry.get('bundle_receipt'), candidate_bundle_sha256=entry.get('candidate_sha256'),
                        job=job, inputs=inputs,
                        reviewable=reviewable,
                        baseline=dict(job=baseline_job, inputs=baseline_inputs, projection_sha256=projection_sha256(baseline),
                                      records=len((baseline or {}).get('records', []))),
                        candidate=None if candidate is None else dict(projection_sha256=projection_sha256(candidate),
                                                                      records=len(candidate['records'])),
                        changes=changes, no_changes=None if changes is None else no_changes(changes),
                        conflicts=conflicts,
                        note='whole-projection review: accept or reject this unit entirely; '
                             'unreviewed or rejected units keep their baseline')
            items.append(item)
    # Same certificate across queries (records or raw rows): preserve, never independent corroboration.
    owners = {}
    for unit_id, value in projections.items():
        raw = {row.get(k) for row in (value or {}).get('raw_rows', []) for k in RAW_CERTIFICATE_FIELDS if row.get(k)}
        for cert in set(_certificates(value)) | raw:
            owners.setdefault(cert, set()).add(unit_id)
    published = []
    for item in items:
        for cert in sorted(c for c, units in owners.items() if item['unit_id'] in units and len(units) > 1):
            item['conflicts'].append(dict(kind='cross_query_certificate_overlap', certificate=cert,
                                          also_in=sorted(owners[cert] - {item['unit_id']}),
                                          independent_corroboration=False))
        item = _with_id(item, 'item_id', 'review-item-')
        _publish(item, Path(output) / 'items', item['item_id'] + '.json')
        published.append(item)
    return published


def check_item(item):
    require(item.get('schema') == ITEM_SCHEMA, 'not a review item')
    _check_id(item, 'item_id', 'review-item-')
    require(item['reviewable'] == (item['outcome'] in REVIEWABLE), 'reviewable flag contradicts outcome')
    require((item['candidate'] is None) == (not item['reviewable']), 'candidate presence contradicts outcome')


def decide(item_path, decision, reviewer, reason, output):
    item = load(item_path)
    check_item(item)
    require(decision in DECISIONS, 'decision must be accepted or rejected')
    require(isinstance(reviewer, str) and reviewer.strip() and isinstance(reason, str) and reason.strip(),
            'decision needs a named reviewer and a reason')
    require(decision != 'accepted' or item['reviewable'], 'failed or blocked units cannot be accepted')
    directory = Path(output) / 'decisions'
    if directory.is_dir():
        for path in sorted(directory.glob('review-decision-*.json')):
            prior = load(path)
            require(prior['item_id'] != item['item_id'], 'item already decided: ' + prior['decision_id'])
    record = _with_id(dict(schema=DECISION_SCHEMA, item_id=item['item_id'], unit_id=item['unit_id'],
                           decision=decision, reviewer=reviewer, reason=reason,
                           accepted_projection_sha256=item['candidate']['projection_sha256'] if decision == 'accepted' else None),
                      'decision_id', 'review-decision-')
    # Atomic per-item claim: the check above is advisory; exactly one decision per item can win this
    # exclusive (link-based) marker. The same decision may re-run to finish an interrupted publish.
    try:
        _publish(dict(item_id=item['item_id'], decision_id=record['decision_id']), directory / CLAIMS, item['item_id'] + '.json')
    except ReviewError as exc:
        raise ReviewError('item already decided: claimed by another decision (' + item['item_id'] + ')') from exc
    _publish(record, directory, record['decision_id'] + '.json')
    return record


# -- release -------------------------------------------------------------------------

DECISION_KEYS = {'schema', 'item_id', 'unit_id', 'decision', 'reviewer', 'reason', 'accepted_projection_sha256', 'decision_id'}


def _check_decision(record, item):
    require(isinstance(record, dict) and set(record) == DECISION_KEYS and record['schema'] == DECISION_SCHEMA,
            'not a review decision')
    _check_id(record, 'decision_id', 'review-decision-')
    require(record['decision'] in DECISIONS, 'unknown review decision: ' + repr(record['decision']))
    require(record['unit_id'] == item['unit_id'], 'decision names another unit')
    require(all(isinstance(record[k], str) and record[k].strip() for k in ('reviewer', 'reason')),
            'decision needs a named reviewer and a reason')
    if record['decision'] == 'accepted':
        require(item['reviewable'], 'failed or blocked units cannot be accepted')
        require(record['accepted_projection_sha256'] == item['candidate']['projection_sha256'],
                'decision does not bind this candidate')
    else:
        require(record['accepted_projection_sha256'] is None, 'rejection must not bind a projection')


def _check_claims(directory, by_id, decisions):
    """A claim marker without its decision is an interrupted decide: refuse, never guess.
    Decisions without a marker (published before markers existed) stay valid."""
    if not directory.is_dir():
        return
    for path in sorted(directory.iterdir()):
        if path.name.startswith('.review-'):
            continue  # leftover temp file of an interrupted publish; never a claim
        require(path.name.startswith('review-item-') and path.name.endswith('.json'), 'unexpected file in decision claims: ' + path.name)
        claim = load(path)
        require(isinstance(claim, dict) and set(claim) == {'item_id', 'decision_id'}
                and path.name == str(claim['item_id']) + '.json' and claim['item_id'] in by_id,
                'decision claim for an item outside this queue: ' + path.name)
        record = decisions.get(claim['item_id'])
        require(record is not None, 'interrupted decide: claim without a decision for ' + claim['item_id']
                + '; re-run the same decide command to complete it')
        require(record['decision_id'] == claim['decision_id'], 'decision claim names another decision: ' + path.name)


def build_release(queue, output, coverage_manifest):
    """Freeze every unit of the coverage manifest: accepted -> reviewed candidate;
    otherwise baseline, or excluded when the unit has no accepted baseline."""
    from . import coverage
    manifest = coverage.load(coverage_manifest)
    require(manifest['manifest_id'] == coverage.identity(manifest), 'coverage manifest identity mismatch')
    queue = Path(queue)
    items = [load(p) for p in sorted((queue / 'items').glob('review-item-*.json'))]
    for item in items:
        check_item(item)
        require(item['coverage_manifest_id'] == manifest['manifest_id'], 'queue item from another coverage manifest')
    require(len({i['unit_id'] for i in items}) == len(items), 'duplicate unit in queue')
    require({i['unit_id'] for i in items} == {u['unit_id'] for u in manifest['units']},
            'review queue must hold exactly one item per coverage unit')
    by_id = {i['item_id']: i for i in items}
    decisions = {}
    for path in sorted((queue / 'decisions').glob('review-decision-*.json')) if (queue / 'decisions').is_dir() else []:
        record = load(path)
        require(isinstance(record, dict) and record.get('item_id') in by_id, 'decision for an item outside this queue')
        _check_decision(record, by_id[record['item_id']])
        require(record['item_id'] not in decisions, 'two decisions for one item')
        decisions[record['item_id']] = record
    _check_claims(queue / 'decisions' / CLAIMS, by_id, decisions)
    first = items[0]
    require(all(i['definition_sha256'] == first['definition_sha256'] for i in items), 'queue mixes definitions')
    entries = []
    for item in sorted(items, key=lambda i: i['unit_id']):
        record = decisions.get(item['item_id'])
        accepted = record is not None and record['decision'] == 'accepted'
        reason = None if accepted else ('rejected' if record else
                                        'unreviewed' if item['reviewable'] else 'not reviewable: ' + item['outcome'])
        has_baseline = item['baseline']['projection_sha256'] is not None and item['baseline']['inputs'] is not None
        status = 'accepted' if accepted else 'baseline' if has_baseline else 'excluded'
        # The baseline keeps its accepted job (possibly at an older path); items built before
        # baseline.job existed carry none, and then the candidate job was the matched one.
        job = item['job'] if status != 'baseline' else (item['baseline'].get('job') or item['job'])
        entries.append(dict(unit_id=item['unit_id'], source_id=item['source_id'], adapter=item['adapter'],
                            status=status, reason=reason, job=job,
                            inputs=item['inputs'] if accepted else item['baseline']['inputs'],
                            projection_sha256=(item['candidate']['projection_sha256'] if accepted
                                               else None if status == 'excluded' else item['baseline']['projection_sha256']),
                            item_id=item['item_id'], decision_id=record['decision_id'] if record else None))
        if status == 'baseline' and job != item['job']:
            entries[-1]['candidate_job'] = item['job']  # optional: the unaccepted job the gate must not import
    release = _with_id(dict(schema=RELEASE_SCHEMA, campaign_id=first['campaign_id'],
                            definition_sha256=first['definition_sha256'],
                            coverage_manifest_id=manifest['manifest_id'], claims=RELEASE_CLAIMS,
                            entries=entries), 'release_id', 'campaign-release-')
    check_release(release)
    _publish(release, output, release['release_id'] + '.json')
    return release


def check_release(release):
    require(release.get('schema') == RELEASE_SCHEMA, 'not a campaign evidence release')
    _check_id(release, 'release_id', 'campaign-release-')
    require(release.get('claims') == RELEASE_CLAIMS, 'release claims were edited')
    entries = release.get('entries')
    require(isinstance(entries, list) and entries, 'release declares no entries')
    for key in ('unit_id', 'source_id'):
        require(len({e[key] for e in entries}) == len(entries), 'duplicate release ' + key)
    for e in entries:
        require(e['status'] in ('accepted', 'baseline', 'excluded') and e['adapter'] in RAW_TABLES
                and e['job']['source'] == e['adapter'], 'invalid release entry')
        require((e['status'] == 'accepted') == (e['reason'] is None), 'only accepted entries omit a reason')
        require(e['status'] != 'accepted' or e['decision_id'] is not None, 'accepted entries need a decision')
        if e['status'] == 'excluded':
            require(e['inputs'] is None and e['projection_sha256'] is None, 'excluded entries carry no inputs')
        else:
            require(isinstance(e['projection_sha256'], str) and len(e['projection_sha256']) == 64, 'entry projection missing')
            require(e['inputs'] and e['job']['path'] in {i['path'] for i in e['inputs']},
                    'entry job must be one of its hashed inputs')
        if 'candidate_job' in e:  # optional; absent in releases frozen before it existed
            c = e['candidate_job']
            require(e['status'] != 'accepted' and isinstance(c, dict) and set(c) == {'source', 'path'}
                    and c['source'] == e['adapter'] and isinstance(c['path'], str) and c != e['job'],
                    'invalid candidate_job on release entry')
    live = [(e['job']['source'], e['job']['path']) for e in entries if e['status'] != 'excluded']
    require(len(live) == len(set(live)), 'two release entries share one job')
    return release


# -- gate (called by rebuild) ----------------------------------------------------------

def gate_inputs(root, data):
    """Before any import: the release is hashed in the manifest and pins every gated job."""
    from . import coverage
    gate = data['campaign_gate']
    require(isinstance(gate, dict) and set(gate) == {'release', 'coverage_manifest'}
            and all(isinstance(gate[k], str) for k in gate), 'campaign_gate must name a release and its coverage manifest')
    require(gate['release'] in data['files'] and gate['coverage_manifest'] in data['files'],
            'campaign gate files must be hashed manifest inputs')
    release = check_release(load(Path(root) / gate['release']))
    manifest = coverage.load(Path(root) / gate['coverage_manifest'])
    require(manifest['manifest_id'] == coverage.identity(manifest) and manifest['manifest_id'] == release['coverage_manifest_id']
            and manifest['campaign_id'] == release['campaign_id'], 'campaign gate: release does not belong to its coverage manifest')
    require({e['unit_id'] for e in release['entries']} == {u['unit_id'] for u in manifest['units']},
            'campaign gate: release must cover every coverage unit')
    jobs = [(j['source'], j['path']) for j in data['imports']]
    for e in release['entries']:
        count = jobs.count((e['job']['source'], e['job']['path']))
        if e['status'] == 'excluded':
            require(count == 0, 'campaign gate: excluded unit must not be imported: ' + e['unit_id'])
            continue
        require(count == 1, 'campaign gate: manifest must import exactly the released job for ' + e['unit_id'])
        if e.get('candidate_job') is not None:
            require(jobs.count((e['candidate_job']['source'], e['candidate_job']['path'])) == 0,
                    'campaign gate: unaccepted candidate job must not be imported: ' + e['unit_id'])
        for item in e['inputs']:
            require(data['files'].get(item['path']) == item['sha256'],
                    'campaign gate: input differs from the reviewed release: ' + item['path'])
    return release


def gate_projections(db, release):
    """After all imports: each gated source equals its accepted or baseline projection."""
    for e in release['entries']:
        actual = projection_sha256(projection(db, e['source_id'], RAW_TABLES[e['adapter']]))
        require(actual == e['projection_sha256'],
                'campaign gate: projection of ' + e['source_id'] + ' differs from the reviewed release')


# -- CLI -------------------------------------------------------------------------------

def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workspace', type=Path, default=Path.cwd())
    commands = parser.add_subparsers(dest='command', required=True)
    queue = commands.add_parser('queue', help='build immutable items from a coverage manifest')
    queue.add_argument('coverage_manifest', type=Path)
    queue.add_argument('--output', type=Path, required=True)
    queue.add_argument('--catalog', type=Path, default=Path('data/devices.sqlite'))
    queue.add_argument('--definition', default=None)
    queue.add_argument('--inventory', default=None)
    queue.add_argument('--accepted-manifest', type=Path, default=Path('data/import-manifest.json'))
    queue.add_argument('--archive', type=Path, default=None, help='archive root (default data/archive)')
    show = commands.add_parser('show', help='print one item')
    show.add_argument('item', type=Path)
    decision = commands.add_parser('decide')
    decision.add_argument('item', type=Path)
    decision.add_argument('decision', choices=DECISIONS)
    decision.add_argument('--reviewer', required=True)
    decision.add_argument('--reason', required=True)
    decision.add_argument('--queue', type=Path, required=True)
    release = commands.add_parser('release')
    release.add_argument('queue', type=Path)
    release.add_argument('--coverage-manifest', type=Path, required=True)
    release.add_argument('--output', type=Path, default=Path('data/campaign-releases'))
    check = commands.add_parser('check-release')
    check.add_argument('release', type=Path)
    args = parser.parse_args(argv)
    if args.command == 'queue':
        from .campaign import DEFAULT_DEFINITION, DEFAULT_INVENTORY
        paths = dict(definition=args.definition or DEFAULT_DEFINITION, inventory=args.inventory or DEFAULT_INVENTORY)
        items = build_queue(args.workspace, args.catalog, args.coverage_manifest, paths, args.output, args.accepted_manifest,
                             archive_root=args.archive)
        result = [dict(item_id=i['item_id'], unit_id=i['unit_id'], outcome=i['outcome'], no_changes=i['no_changes'],
                       conflicts=len(i['conflicts'])) for i in items]
    elif args.command == 'show':
        result = load(args.item)
        check_item(result)
    elif args.command == 'decide':
        result = decide(args.item, args.decision, args.reviewer, args.reason, args.queue)
    elif args.command == 'release':
        result = build_release(args.queue, args.output, args.coverage_manifest)
    else:
        result = check_release(load(args.release))
    json.dump(result, sys.stdout, indent=2, sort_keys=True, ensure_ascii=False)
    sys.stdout.write('\n')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
