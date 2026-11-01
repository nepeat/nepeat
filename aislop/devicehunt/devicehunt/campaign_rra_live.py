"""Bounded RRA manufacturer-query refresh through shared capture (c2b).

Same exact request sequence as research/capture_rra_query.py, but every request is
admitted, budgeted, restricted and recorded by `capture.Collector`. No curl, no
redirects and no retries. Each step has its own observation slot scoped to the unit
revision and refresh cycle. The after-export is therefore always a second, separately
reserved HTTP observation; it can never reuse the before-export. A resumed cycle
reuses its own published slots without a new request.

Detail requests come only from the validated before-CSV, under a predeclared cap; a
larger result set stops before any detail request. A partial or failed step set
never becomes a bundle. Nothing here imports into the catalog or activates accounts.
"""
from __future__ import annotations
import dataclasses
import json
import os
import re
import shutil
from pathlib import Path
import time
from urllib.parse import quote

from . import rra
from .artifacts import digest
from .campaign_state import StopCollection

QUERY_FIELDS = ('category', 'fromdate', 'todate', 'firm', 'equip', 'model_no', 'app_no', 'maker', 'nation')
COMPARISONS = ('byte_identical', 'exact_rows_ignore_order')
FORM_HEADERS = {'Content-Type': 'application/x-www-form-urlencoded'}


class DetailCapExceeded(ValueError):
    pass


@dataclasses.dataclass(frozen=True)
class RraScope:
    """Exact pinned query scope plus the predeclared detail-request cap."""
    query: tuple
    max_details: int
    comparison: str = 'byte_identical'

    @classmethod
    def build(cls, query, max_details, comparison='byte_identical'):
        if (not isinstance(query, dict) or tuple(query) != QUERY_FIELDS
                or any(not isinstance(v, str) for v in query.values()) or not query['maker']
                or any(query[k] for k in ('category', 'firm', 'equip', 'model_no', 'app_no', 'nation'))):
            raise ValueError('only exact manufacturer RRA query scopes are supported')
        if type(max_details) is not int or max_details < 0 or comparison not in COMPARISONS:
            raise ValueError('nonnegative integer detail cap and declared comparison mode required')
        # Reuse the validator's date rules rather than re-deriving them here.
        probe = dict(schema_version=1, form_url=rra.FORM, csv_url=rra.CSV_URL, complete=True, query=dict(query))
        _check_query(probe)
        return cls(tuple(query.items()), max_details, comparison)

    @property
    def body(self):
        # Same field order and percent-encoding as the curl --data-urlencode helper.
        return '&'.join(k + '=' + quote(v, safe='') for k, v in self.query).encode()


def _check_query(bundle):
    import datetime as dt
    import re
    query = bundle['query']
    for name in ('fromdate', 'todate'):
        if not re.fullmatch(r'\d{8}', query[name]):
            raise ValueError('Invalid RRA query dates')
        dt.datetime.strptime(query[name], '%Y%m%d')
    if query['fromdate'] > query['todate']:
        raise ValueError('Reversed RRA query dates')


def _strict(validator):
    """Any validator failure (csv.Error, RecursionError, ...) is an invalid response,
    so Collector publishes the bytes and a result instead of stranding the reservation."""
    def checked(payload):
        try:
            return validator(payload)
        except ValueError:
            raise
        except Exception as exc:
            raise ValueError(type(exc).__name__) from None
    return checked


def _detail(certificate, maker):
    def check(payload):
        parsed = rra.parse_detail(payload, certificate)
        # Same manufacturer rule as rra.validate, applied before more requests are spent.
        if maker.casefold() not in parsed['manufacturer'].casefold():
            raise ValueError('RRA detail manufacturer does not match the captured query')
    return check


def _form(payload):
    text = payload.decode('cp949')
    if '/ko/license/csv_down.do' not in text or 'name="maker"' not in text:
        raise ValueError('RRA form/export association changed')


def _csv(payload):
    rra.csv_rows(payload, allow_empty=True)


def next_work(scope, observed):
    """Pure planner. `observed` maps completed slots to their validated bytes.

    Returns the next request descriptor, or None when the step set is complete.
    """
    if 'form' not in observed:
        return dict(slot='form', method='GET', url=rra.FORM, body=b'', headers={},
                    resource='rra-search-form', validator=_strict(_form), validator_id='rra-form/1')
    if 'csv-before' not in observed:
        return dict(slot='csv-before', method='POST', url=rra.CSV_URL, body=scope.body, headers=dict(FORM_HEADERS),
                    resource='rra-csv-export', validator=_strict(_csv), validator_id='rra-csv/1')
    rows = rra.csv_rows(observed['csv-before'], allow_empty=True)
    if len(rows) > scope.max_details:
        raise DetailCapExceeded('before-CSV lists %d certificates; predeclared cap is %d' % (len(rows), scope.max_details))
    if rows:
        for n, row in enumerate(rows, 1):
            slot = 'detail-%03d' % n
            if slot not in observed:
                certificate = row[-1]
                return dict(slot=slot, method='GET', url=rra.DETAIL + quote(certificate, safe=''), body=b'', headers={},
                            resource='rra-certificate:' + certificate, certificate=certificate,
                            validator=_strict(_detail(certificate, dict(scope.query)['maker'])),
                            validator_id='rra-detail/2')
    elif 'empty-result' not in observed:
        return dict(slot='empty-result', method='POST', url=rra.RESULT_URL, body=scope.body, headers=dict(FORM_HEADERS),
                    resource='rra-result-page', validator=_strict(rra.validate_empty_result),
                    validator_id='rra-empty-result/1')
    if 'csv-after' not in observed:
        return dict(slot='csv-after', method='POST', url=rra.CSV_URL, body=scope.body, headers=dict(FORM_HEADERS),
                    resource='rra-csv-export', validator=_strict(_csv), validator_id='rra-csv/1')
    return None


BUDGET_STOPS = {'durable quota exhausted', 'durable host quota exhausted', 'account deadline exhausted',
                'account elapsed window exhausted'}
RESTRICTION_STOPS = {'durable source restriction', 'restriction arrived before dispatch',
                     'collection checkpointed after throttling'}


def _stop_outcome(message, restrictions):
    """Exact known stops only; restriction stops are classified from durable state."""
    if message in BUDGET_STOPS:
        return 'budget_exhausted'
    if message in RESTRICTION_STOPS:
        kinds = {r['kind'] for r in restrictions}
        # A denial is the stronger, non-resumable state when both are present.
        return 'blocked' if 'deny' in kinds else 'throttled' if 'throttle' in kinds else 'stopped'
    return 'stopped'


OUTCOMES = {'blocked': 'blocked', 'throttled': 'throttled', 'saved_unavailable': 'unavailable',
            'offline_unavailable': 'unavailable', 'unavailable': 'unavailable', 'timeout': 'unavailable',
            'invalid': 'failed', 'empty': 'failed'}


def _read_evidence(directory, entry):
    from .documents import dependency
    return dependency(directory, entry)  # sibling name only; raises on checksum mismatch


def _load_carried_evidence(saved_bundle):
    """Return (declaration, {name: bytes}) for the saved bundle's company evidence, or None.

    Evidence may reference sibling files (`base`, `exhibits`); those are carried too, each
    verified against the hash its parent declares. Any mismatch refuses the whole bundle.
    """
    saved = Path(saved_bundle).resolve()
    saved_data = json.loads(saved.read_text(encoding='utf-8'))
    declared, saved_query = saved_data.get('company_evidence'), saved_data.get('query')
    if declared is None:
        return None
    if (not isinstance(declared, dict) or set(declared) != {'path', 'sha256'}
            or any(not isinstance(v, str) for v in declared.values())):
        raise ValueError('malformed saved company_evidence declaration')
    files, pending = {}, [declared]
    while pending:
        entry = pending.pop()
        if entry['path'] in files:
            if digest(files[entry['path']]) != entry['sha256']:
                raise ValueError('carried company evidence name collides with a different file: ' + entry['path'])
            continue
        data = _read_evidence(saved.parent, entry)
        files[entry['path']] = data
        try:
            parsed = json.loads(data)
        except ValueError:
            continue
        if isinstance(parsed, dict):
            refs = ([parsed['base']] if isinstance(parsed.get('base'), dict) else []) + \
                [v for v in (parsed.get('exhibits') or {}).values() if isinstance(v, dict)]
            for ref in refs:
                if set(ref) != {'path', 'sha256'}:
                    raise ValueError('malformed company evidence reference')
                pending.append(ref)
    return dict(declared), files, saved_query


GENERATED = re.compile(r'(search-form\.html|before\.csv|after\.csv|detail-\d{3,}\.html|empty-result\.html'
                       r'|candidate\.json|bundle\.json)')


def _check_collisions(files):
    for name in files:
        if GENERATED.fullmatch(name):
            raise ValueError('carried company evidence collides with a generated bundle file: ' + name)


def compare_live(output, expectations):
    """Pinned comparison for a live bundle: identical to the adapter comparison except that
    `detail_path` is compared by file name, since a live bundle sits in a different directory."""
    from pathlib import PurePosixPath
    from .campaign_sources import RraSavedAdapter

    def base(values):
        values = dict(values)
        if isinstance(values.get('occurrences'), list):
            values['occurrences'] = [dict(o, detail_path=PurePosixPath(o['detail_path']).name)
                                     if isinstance(o, dict) and 'detail_path' in o else o
                                     for o in values['occurrences']]
        return values
    if not expectations:
        return dict(status='not_pinned', mismatches=[], location_only=False)
    actual = base(RraSavedAdapter().pinned_values(output))
    wanted = base(expectations)
    mismatches = sorted(k for k, v in actual.items() if k in wanted and wanted[k] != v)
    status = 'matches_pinned' if not mismatches else 'differs_from_pinned'
    raw = RraSavedAdapter().compare(output, expectations)
    return dict(status=status, mismatches=mismatches,
                location_only=raw['status'] == 'differs_from_pinned' and status == 'matches_pinned')


class RraRefresh:
    """One bounded refresh cycle of one unit.

    `collector_for(policy)` builds a Collector bound to the caller's explicitly
    allocated account. Each step narrows the base policy to that exact URL and slot.
    """
    def __init__(self, archive, base_policy, collector_for, scope, *, unit_revision, cycle_id,
                 min_interval=0.15, pause=time.sleep):
        if not unit_revision or not cycle_id:
            raise ValueError('unit revision and refresh cycle identity required')
        if min_interval < 0:
            raise ValueError('nonnegative request interval required')
        self.archive, self.base_policy, self.collector_for = archive, base_policy, collector_for
        self.scope, self.unit_revision, self.cycle_id = scope, unit_revision, cycle_id
        # Pacing between dispatched requests, as the curl helper slept between details.
        self.min_interval, self.pause = min_interval, pause

    def slot_scope(self, slot):
        # Unambiguous even when identities contain ':' (e.g. ISO timestamps).
        return json.dumps([self.unit_revision, self.cycle_id, slot], separators=(',', ':'))

    def _prior_outcome(self, result):
        """A resumed non-success slot keeps its original outcome name."""
        prior = result.get('prior_capture_id')
        if result['outcome'] == 'saved_unavailable' and prior:
            try:
                return self.archive.get_receipt(prior)['payload'].get('outcome') or result['outcome']
            except (KeyError, ValueError):
                pass
        return result['outcome']

    def capture(self):
        """Run or resume the step set. Returns the outcome plus per-slot receipts."""
        observed, steps, dispatched = {}, [], False

        def finish(outcome, **extra):
            new = sum(1 for s in steps if s.get('new_observation'))
            return dict(outcome=outcome, steps=steps, observed=observed, new_observations=new, **extra)
        while True:
            try:
                work = next_work(self.scope, observed)
            except DetailCapExceeded as exc:
                return finish('budget_exhausted', reason=str(exc))
            if work is None:
                return finish('captured')
            policy = dataclasses.replace(self.base_policy, allowed_urls=frozenset([work['url']]), source='rra',
                                         observation_scope=self.slot_scope(work['slot']), reuse_policy='compatible')
            collector = self.collector_for(policy)
            if getattr(collector, 'unit_revision', None) != self.unit_revision:
                raise ValueError('collector reservations must bind this refresh unit revision')
            if dispatched and self.min_interval:
                self.pause(self.min_interval)
            try:
                result = collector.capture(work['url'], method=work['method'], body=work['body'], headers=work['headers'],
                                           validator=work['validator'], validator_id=work['validator_id'],
                                           resource=work['resource'])
            except StopCollection as exc:
                restrictions = collector.state.restrictions(work['url'], source='rra', resource=work['resource']) \
                    if collector.state is not None else []
                steps.append(dict(slot=work['slot'], url=work['url'], outcome='stopped', reason=str(exc)))
                return finish(_stop_outcome(str(exc), restrictions), reason=str(exc))
            # Sticky once anything dispatched: a later dispatch after a reused slot still waits.
            dispatched = dispatched or bool(result.get('attempt_id'))
            outcome = self._prior_outcome(result)
            step = dict(slot=work['slot'], url=work['url'], outcome=outcome, receipt_id=result['id'],
                        observation_id=result.get('prior_capture_id') or result['id'],
                        new_observation=bool(result.get('attempt_id')), status=result.get('status'), error=result.get('error'),
                        reservation_id=result.get('reservation_id'), artifact_sha256=result.get('artifact_sha256'))
            steps.append(step)
            if result.get('attempt_id') and collector.state.reservation(result['reservation_id'])['state'] == 'uncertain':
                # Transport/body failure: charged, unreconciled; the account is stopped.
                return finish('interrupted', reason=outcome)
            if outcome not in ('success', 'not_modified', 'reused'):
                return finish(OUTCOMES.get(outcome, 'failed'), reason=outcome)
            observed[work['slot']] = self.archive.resolve(result['artifact_sha256']).read_bytes()

    def bundle(self, captured, output, *, carry_from=None):
        """Materialize a complete captured step set as a validated bundle in a NEW directory.

        `carry_from` is the unit's saved/pinned bundle.json. Its reviewed `company_evidence`
        (and the sibling evidence files that declaration references) is verified against its
        declared hashes, copied byte-identically and declared identically. Evidence is never
        invented. `first_result_page` is deliberately NOT carried: it is an observation of the
        old session (rra.validate treats it as optional), so copying it would pass it off as live.
        """
        if captured['outcome'] != 'captured':
            raise ValueError('only a complete captured step set becomes a bundle')
        carried = _load_carried_evidence(carry_from) if carry_from is not None else None
        if carried:
            _check_collisions(carried[1])
            if carried[2] != dict(self.scope.query):
                raise ValueError('carried company evidence belongs to a different query scope')
        observed = captured['observed']
        receipts = {s['slot']: s['observation_id'] for s in captured['steps']}
        root = Path(output)
        root.mkdir(parents=True, exist_ok=False)
        try:
            return self._write_bundle(captured, root, observed, receipts, carried)
        except BaseException:
            # This call created `root` exclusively, so everything in it is ours.
            shutil.rmtree(root, ignore_errors=True)
            raise

    def _write_bundle(self, captured, root, observed, receipts, carried):
        def write(name, slot):
            data = observed[slot]
            fd = os.open(root / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
            with os.fdopen(fd, 'wb') as stream:
                stream.write(data)
            return dict(path=name, sha256=digest(data))
        query = dict(self.scope.query)
        rows = rra.csv_rows(observed['csv-before'], allow_empty=True)
        details = []
        for n, row in enumerate(rows, 1):
            entry = write('detail-%03d.html' % n, 'detail-%03d' % n)
            details.append(dict(certificate=row[-1], url=rra.DETAIL + quote(row[-1], safe=''), **entry))
        bundle = dict(schema_version=1, complete=True, form_url=rra.FORM, csv_url=rra.CSV_URL, query=query,
                      form=write('search-form.html', 'form'), csv_before=write('before.csv', 'csv-before'),
                      csv_after=write('after.csv', 'csv-after'), expected_rows=len(rows), details=details,
                      acquisition=dict(method='devicehunt.campaign_rra_live', unit_revision=self.unit_revision,
                                       cycle_id=self.cycle_id, capture_receipts=receipts))
        if not rows:
            bundle['empty_result'] = dict(url=rra.RESULT_URL, **write('empty-result.html', 'empty-result'))
        if self.scope.comparison != 'byte_identical':
            bundle['csv_comparison'] = self.scope.comparison
        if carried:
            declaration, files, _ = carried
            for name, data in files.items():
                fd = os.open(root / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
                with os.fdopen(fd, 'wb') as stream:
                    stream.write(data)
                if digest((root / name).read_bytes()) != digest(data):
                    raise ValueError('carried company evidence changed while copying: ' + name)
            bundle['company_evidence'] = declaration
        candidate = root / 'candidate.json'
        candidate.write_text(json.dumps(bundle, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
        try:
            rra.validate(candidate)
        except ValueError as exc:
            # Retain the failed candidate as evidence of the attempt; never as a bundle.
            return dict(outcome='failed', reason=str(exc), candidate=str(candidate))
        final = root / 'bundle.json'
        os.rename(candidate, final)
        return dict(outcome='explicit_zero' if not rows else 'validated_nonempty', bundle=str(final))
