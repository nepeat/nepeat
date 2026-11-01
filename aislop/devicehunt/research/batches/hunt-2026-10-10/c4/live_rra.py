"""c4: supervised, finite live RRA refresh (2026-10-10). Coordinator-run only.

Phases (each allocates and activates its own immutable finite account; nothing retries):
  smoke                      one form GET through production transport
  refresh UNIT_ID --cycle C  one unit's full step set, bundled into a NEW data/raw dir and
                             compared with the pinned c0 expectations
  rebundle UNIT_ID --cycle C re-bundle a live capture carrying reviewed company evidence (no HTTP)
  settle RES --result R      record the decision for one status-less timed-out attempt (no HTTP)
  reconcile ACCOUNT --label L
                             record the decision re-baselining a timed account after a
                             reboot/clock anomaly; remaining time only shrinks (no HTTP)

Writes archive receipts/objects and operational account rows only; never the catalog,
manifest or exports. Reports measured requests, bytes and wall time.
"""
import argparse
import json
import sqlite3
import sys
import time
from contextlib import closing
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT))

from devicehunt import rra  # noqa: E402
from devicehunt.artifacts import Archive  # noqa: E402
from devicehunt.campaign import DEFAULT_DEFINITION, DEFAULT_INVENTORY, load_definition  # noqa: E402
from devicehunt.campaign_rra_live import RraRefresh, RraScope, _form, _strict  # noqa: E402
from devicehunt.campaign_sources import ADAPTERS  # noqa: E402
from devicehunt.campaign_state import CampaignState  # noqa: E402
from devicehunt.capture import Collector, Policy  # noqa: E402

OUT = Path(__file__).resolve().parent
HOST = 'www.rra.go.kr'
POLICY_HASH = 'rra-refresh/c4-2026-10-10'
MAX_BYTES = 1_000_000          # per response; reservations charge this conservatively
TIMEOUT = 120.0                # RRA's CSV export exceeded the 30 s default on the first attempt
LEASE = 300.0
WINDOW = 1800.0                # elapsed seconds per account (`deadline` would be an absolute wall time)


def archive_totals(archive):
    with closing(sqlite3.connect(archive.database.as_uri() + '?mode=ro', uri=True)) as db:
        receipts, head = db.execute('SELECT COUNT(*), MAX(sequence) FROM receipts').fetchone()
    objects = sum(p.stat().st_size for p in (archive.root / 'objects').rglob('*') if p.is_file())
    receipt_bytes = sum(p.stat().st_size for p in (archive.root / 'receipts').glob('*.json'))
    return dict(receipts=receipts, head_sequence=head, object_bytes=objects, receipt_bytes=receipt_bytes,
                index_bytes=archive.database.stat().st_size)


def open_all():
    archive = Archive(ROOT / 'data/archive', ROOT)
    state = CampaignState(archive.root)
    return archive, state


def account(state, archive, name, requests, decision):
    definition = load_definition(ROOT, DEFAULT_DEFINITION, DEFAULT_INVENTORY)
    state.allocate_account(name, definition_hash=definition.sha256, policy_hash=POLICY_HASH,
                           request_limit=requests, byte_limit=requests * (MAX_BYTES + 1), window=WINDOW,
                           host_limits={HOST: (requests, requests * (MAX_BYTES + 1))})  # reserve() charges max_bytes + 1
    state.activate(archive, name, policy_hash=POLICY_HASH, decision=decision)
    return definition


def smoke(args):
    archive, state = open_all()
    try:
        before, start = archive_totals(archive), time.time()
        name = 'c4-smoke-' + args.cycle
        account(state, archive, name, 1, args.authorization + ': '
                'one form GET smoke before any RRA refresh (c2b precondition 2)')
        policy = Policy(frozenset([rra.FORM]), budget=1, max_bytes=MAX_BYTES, timeout=TIMEOUT, lease_seconds=LEASE,
                        policy_hash=POLICY_HASH, source='rra',
                        observation_scope=json.dumps(['c4-smoke', args.cycle]), reuse_policy='never')
        collector = Collector(archive, policy, state=state, account_id=name, run_id=name, unit_revision='c4-smoke')
        result = collector.capture(rra.FORM, method='GET', validator=_strict(_form), validator_id='rra-form/1',
                                   resource='rra-search-form')
        report = dict(phase='smoke', account=name, outcome=result['outcome'], status=result.get('status'),
                      error=result.get('error'), artifact_sha256=result.get('artifact_sha256'),
                      seconds=round(time.time() - start, 3), archive_before=before, archive_after=archive_totals(archive),
                      account_state=state.account(name))
        return report
    finally:
        archive.close()


def refresh(args):
    definition = load_definition(ROOT, DEFAULT_DEFINITION, DEFAULT_INVENTORY)
    unit = next(u for u in definition.units if u.unit_id == args.unit)
    saved = json.loads((ROOT / unit.manifest).read_text())
    rows = unit.expectations['csv_rows_before']
    max_details = args.max_details if args.max_details is not None else rows
    requests = 4 + max_details if max_details else 4   # form, csv-before, details|empty-result, csv-after
    archive, state = open_all()
    try:
        before, start = archive_totals(archive), time.time()
        name = 'c4-' + args.unit + '-' + args.cycle
        account(state, archive, name, requests,
                args.authorization + ': finite live refresh of ' + args.unit + ' (pinned ' + str(rows) +
                ' rows), detail cap ' + str(max_details) + ', ' + str(requests) + ' requests max')
        scope = RraScope.build(saved['query'], max_details, saved.get('csv_comparison', 'byte_identical'))
        base = Policy(frozenset(), budget=requests, max_bytes=MAX_BYTES, timeout=TIMEOUT, lease_seconds=LEASE,
                      policy_hash=POLICY_HASH)

        def collector_for(policy):
            return Collector(archive, policy, state=state, account_id=name, run_id=name, unit_revision=unit.revision)
        live = RraRefresh(archive, base, collector_for, scope, unit_revision=unit.revision, cycle_id=args.cycle,
                          min_interval=args.interval)
        captured = live.capture()
        report = dict(phase='refresh', unit_id=args.unit, account=name, cycle=args.cycle, request_cap=requests,
                      interval_seconds=args.interval,
                      detail_cap=max_details, pinned_rows=rows, outcome=captured['outcome'], reason=captured.get('reason'),
                      new_observations=captured['new_observations'],
                      steps=[{k: s.get(k) for k in ('slot', 'outcome', 'status', 'error', 'artifact_sha256')}
                             for s in captured['steps']])
        if captured['outcome'] == 'captured':
            target = ROOT / 'data/raw' / (Path(unit.manifest).parent.name + '-live-' + args.cycle)
            bundled = live.bundle(captured, target)
            report['bundle'] = bundled
            if bundled['outcome'] in ('validated_nonempty', 'explicit_zero'):
                relative = Path(bundled['bundle']).relative_to(ROOT).as_posix()
                adapter = ADAPTERS['rra']
                output = adapter.records(ROOT / relative, relative)
                report['comparison_with_pinned'] = adapter.compare(output, unit.expectations)
                report['live_query_records'] = output.get('query_records')
        report.update(seconds=round(time.time() - start, 3), archive_before=before, archive_after=archive_totals(archive),
                      account_state=state.account(name))
        return report
    finally:
        archive.close()


def rebundle(args):
    """No HTTP: rebuild a live cycle's step set from its archived capture receipts and bundle it
    again into a NEW directory, carrying the saved unit's reviewed company evidence."""
    from devicehunt.campaign_rra_live import compare_live
    definition = load_definition(ROOT, DEFAULT_DEFINITION, DEFAULT_INVENTORY)
    unit = next(u for u in definition.units if u.unit_id == args.unit)
    saved_path = ROOT / unit.manifest
    saved = json.loads(saved_path.read_text())
    live_dir = ROOT / 'data/raw' / (Path(unit.manifest).parent.name + '-live-' + args.cycle)
    live = json.loads((live_dir / 'bundle.json').read_text())
    acquisition = live['acquisition']
    if acquisition['unit_revision'] != unit.revision or acquisition['cycle_id'] != args.cycle:
        raise SystemExit('live bundle belongs to another unit revision or cycle')
    archive, state = open_all()
    try:
        observed, steps = {}, []
        for slot, receipt_id in acquisition['capture_receipts'].items():
            payload = archive.verified_receipt(receipt_id)['wrapper']['payload']
            if payload.get('outcome') != 'success' or not payload.get('artifact_sha256'):
                raise SystemExit('slot ' + slot + ' receipt is not a successful capture')
            observed[slot] = archive.resolve(payload['artifact_sha256']).read_bytes()
            steps.append(dict(slot=slot, observation_id=receipt_id))
        # Every rebuilt slot must equal the bytes the live bundle already holds.
        files = {'form': live['form'], 'csv-before': live['csv_before'], 'csv-after': live['csv_after']}
        files.update({'detail-%03d' % n: d for n, d in enumerate(live['details'], 1)})
        if 'empty_result' in live:
            files['empty-result'] = live['empty_result']
        if set(files) != set(observed) or any((live_dir / f['path']).read_bytes() != observed[s] for s, f in files.items()):
            raise SystemExit('archived observations do not equal the live bundle files')
        scope = RraScope.build(saved['query'], len(live['details']), saved.get('csv_comparison', 'byte_identical'))
        refresh = RraRefresh(archive, Policy(frozenset()), None, scope, unit_revision=unit.revision, cycle_id=args.cycle)
        target = ROOT / 'data/raw' / (Path(unit.manifest).parent.name + '-live-' + args.cycle + '-carried')
        result = refresh.bundle(dict(outcome='captured', observed=observed, steps=steps), target, carry_from=saved_path)
        report = dict(phase='rebundle', unit_id=args.unit, cycle=args.cycle, http_requests=0, source_live_bundle=str(live_dir.relative_to(ROOT)),
                      bundle=result)
        if result['outcome'] in ('validated_nonempty', 'explicit_zero'):
            relative = Path(result['bundle']).relative_to(ROOT).as_posix()
            output = ADAPTERS['rra'].records(ROOT / relative, relative)
            report['compare_live'] = compare_live(output, unit.expectations)
        return report
    finally:
        archive.close()


def settle(args):
    """Record the explicit decision for one uncertain, status-less attempt (no HTTP)."""
    archive, state = open_all()
    try:
        row = state.settle_unobserved(args.reservation, archive=archive, result_id=args.result, decision=args.decision)
        return dict(phase='settle', reservation=args.reservation, account=row['account_id'], url=row['url'],
                    state=state.reservation(args.reservation)['state'], decision=args.decision,
                    account_state=state.account(row['account_id']))
    finally:
        archive.close()


def reconcile(args):
    """Record the explicit decision re-baselining a timed account after a reboot/clock anomaly (no HTTP)."""
    archive, state = open_all()
    try:
        before = state.account(args.account)
        state.reconcile_clock(args.account, archive=archive, decision=args.decision)
        return dict(phase='reconcile', account=args.account, decision=args.decision,
                    account_before=before, account_after=state.account(args.account))
    finally:
        archive.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    s = commands.add_parser('smoke')
    s.add_argument('--cycle', required=True)
    r = commands.add_parser('refresh')
    r.add_argument('unit')
    r.add_argument('--cycle', required=True)
    r.add_argument('--max-details', type=int)
    r.add_argument('--interval', type=float, default=0.15, help='seconds between dispatched requests')
    b = commands.add_parser('rebundle')
    b.add_argument('unit')
    b.add_argument('--cycle', required=True)
    t = commands.add_parser('settle')
    t.add_argument('reservation')
    t.add_argument('--result', required=True, help='published capture_result receipt id proving no status')
    t.add_argument('--decision', required=True)
    c = commands.add_parser('reconcile')
    c.add_argument('account')
    c.add_argument('--label', required=True, help='fresh report label (one per reconciliation)')
    c.add_argument('--decision', required=True)
    for sub in (s, r):
        sub.add_argument('--authorization', default='user 2026-10-10 "give it a shot, see how expensive it is"',
                         help='recorded user authorization for this live scope (goes into the account decision)')
    args = parser.parse_args()
    name = (args.command + '-' + args.reservation if args.command == 'settle' else
            args.command + '-' + args.account + '-' + args.label if args.command == 'reconcile' else
            args.command + ('-' + args.unit if args.command in ('refresh', 'rebundle') else '') + '-' + args.cycle) + '.json'
    path = OUT / name
    if path.exists():
        raise SystemExit('report exists; use a fresh cycle id')
    try:
        report = dict(smoke=smoke, refresh=refresh, settle=settle, rebundle=rebundle,
                      reconcile=reconcile)[args.command](args)
    except BaseException as exc:
        # Keep a cost record even when a phase fails after charging (spent_bytes is the
        # worst-case reservation charge, not measured bytes).
        report = dict(phase=args.command, failed=type(exc).__name__, message=str(exc)[:500])
        try:
            archive, state = open_all()
            try:
                report['archive_after'] = archive_totals(archive)
            finally:
                archive.close()
        except Exception:
            pass
        raise
    finally:
        with path.open('x') as stream:
            json.dump(locals().get('report', {}), stream, indent=2, sort_keys=True, default=str)
            stream.write('\n')
    print(json.dumps(report, indent=2, sort_keys=True, default=str)[:4000])


if __name__ == '__main__':
    main()
