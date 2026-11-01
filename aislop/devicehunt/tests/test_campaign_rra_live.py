"""C2b gates: the RRA refresh path through shared capture, with mocked transport only.

Real saved RRA bytes are read and served by a fake transport into isolated archives.
Network and external-process entrypoints are forbidden; no live account exists.
"""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from devicehunt import rra
from devicehunt.artifacts import Archive
from devicehunt.campaign_rra_live import DetailCapExceeded, RraRefresh, RraScope, compare_live, next_work
from devicehunt.campaign_sources import RraSavedAdapter
from devicehunt.campaign_state import CampaignState
from devicehunt.capture import Collector, Policy, Response

ROOT = Path(__file__).resolve().parents[1]
INVENTORY = ROOT / 'research/batches/campaign-pilot-2026-10-04/inventory/inventory.json'
NONEMPTY = ('rra-official-query-5827f2fe7092', 'data/raw/rra-edge-core-hyphen-official')
ZERO = ('rra-official-query-78fc6b2e95f6', 'data/raw/rra-edge-core-space-official')
SECRET = 'JSESSIONID=never-persist-this'


def pinned(unit_id):
    return next(u for u in json.loads(INVENTORY.read_text())['units'] if u['unit_id'] == unit_id)


class SavedRra:
    """Serves one saved bundle's exact bytes; any other request is a test failure."""
    def __init__(self, directory, after=None, statuses=None, replace=None):
        self.dir = ROOT / directory
        self.bundle = json.loads((self.dir / 'bundle.json').read_text())
        self.after, self.statuses, self.replace = after, statuses or {}, replace or {}
        self.calls = []

    def read(self, entry):
        return (self.dir / entry['path']).read_bytes()

    def route(self, method, url, body):
        b = self.bundle
        if (method, url) == ('GET', rra.FORM):
            return 'form', self.read(b['form'])
        if (method, url) == ('POST', rra.CSV_URL):
            first = sum(1 for m, u, _ in self.calls if (m, u) == ('POST', rra.CSV_URL)) == 1
            return ('csv-before', self.read(b['csv_before'])) if first else ('csv-after', self.after or self.read(b['csv_after']))
        if (method, url) == ('POST', rra.RESULT_URL):
            return 'empty-result', self.read(b['empty_result'])
        for n, entry in enumerate(b['details'], 1):
            if (method, url) == ('GET', entry['url']):
                return 'detail-%03d' % n, self.read(entry)
        raise AssertionError('request outside the saved allowlist: %s %s' % (method, url))

    def __call__(self, method, url, body, headers, timeout, max_bytes, *, observe_response):
        self.calls.append((method, url, body))
        slot, payload = self.route(method, url, body)
        payload = self.replace.get(slot, payload)
        status = self.statuses.get(slot, 200)
        observe_response(status, url, {'Set-Cookie': SECRET, 'Content-Type': 'text/html'})
        return Response(status, url, {'Set-Cookie': SECRET}, payload)


class RraLiveTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        for entry in ('socket.socket.connect', 'socket.create_connection', 'socket.getaddrinfo',
                      'http.client.HTTPConnection.connect', 'urllib.request.OpenerDirector.open',
                      'subprocess.run', 'subprocess.Popen'):
            guard = patch(entry, side_effect=AssertionError('forbidden network/external entrypoint'))
            guard.start()
            self.addCleanup(guard.stop)
        self.archive = Archive(self.root / 'archive', workspace=self.root, create=True)
        self.addCleanup(self.archive.close)
        self.state = CampaignState.initialize(self.archive.root)
        self.state.synchronize_archive(self.archive, decision='fixture binding')

    def tearDown(self):
        self.tmp.cleanup()

    def refresh(self, transport, directory, *, requests=50, cycle='cycle-1', max_details=200,
                comparison='byte_identical', fault=None, account=None, unit_revision='unit/fixture'):
        account = account or 'account-' + cycle
        self.state.allocate_account(account, definition_hash='fixture', policy_hash='rra-refresh/1',
                                    request_limit=requests, byte_limit=10 ** 9)
        bundle = json.loads((ROOT / directory / 'bundle.json').read_text())
        scope = RraScope.build(bundle['query'], max_details, comparison)
        base = Policy(frozenset(), budget=requests, policy_hash='rra-refresh/1', max_bytes=4_000_000)

        def collector_for(policy):
            return Collector(self.archive, policy, transport, state=self.state, account_id=account,
                             mock_transport=True, fault=fault, unit_revision=unit_revision, run_id=cycle)
        self.pauses = []
        return RraRefresh(self.archive, base, collector_for, scope, unit_revision='unit/fixture', cycle_id=cycle,
                          pause=self.pauses.append)

    def assert_no_secret(self):
        for path in (self.archive.root / 'receipts').glob('*.json'):
            self.assertNotIn('never-persist-this', path.read_text(), path.name)

    def test_refresh_reproduces_the_pinned_saved_records(self):
        transport = SavedRra(NONEMPTY[1])
        refresh = self.refresh(transport, NONEMPTY[1])
        captured = refresh.capture()
        self.assertEqual(captured['outcome'], 'captured')
        result = refresh.bundle(captured, self.root / 'out')
        self.assertEqual(result['outcome'], 'validated_nonempty')
        records = RraSavedAdapter().records(Path(result['bundle']), 'out/bundle.json')
        expected = pinned(NONEMPTY[0])['expectations']
        self.assertEqual(records['record_sets']['validated_original_records_sha256'],
                         expected['validated_original_records_sha256'])
        # form + before + 7 details + after, each its own durable reservation.
        self.assertEqual(len(transport.calls), 10)
        with self.state.db() as db:
            states = [r[0] for r in db.execute('SELECT state FROM reservations')]
        self.assertEqual(states, ['complete'] * 10)
        self.assert_no_secret()

    def test_explicit_zero_requests_the_saved_no_results_page(self):
        transport = SavedRra(ZERO[1])
        refresh = self.refresh(transport, ZERO[1])
        result = refresh.bundle(refresh.capture(), self.root / 'out')
        self.assertEqual(result['outcome'], 'explicit_zero')
        self.assertEqual([c[1] for c in transport.calls], [rra.FORM, rra.CSV_URL, rra.RESULT_URL, rra.CSV_URL])
        records = RraSavedAdapter().records(Path(result['bundle']), 'out/bundle.json')
        self.assertEqual(records['record_sets']['validated_original_records_sha256'],
                         pinned(ZERO[0])['expectations']['validated_original_records_sha256'])

    def test_after_export_is_a_separate_observation_and_cycles_do_not_share_slots(self):
        transport = SavedRra(ZERO[1])
        self.refresh(transport, ZERO[1]).capture()
        csv_calls = [c for c in transport.calls if c[1] == rra.CSV_URL]
        self.assertEqual(len(csv_calls), 2)
        self.assertEqual(csv_calls[0], csv_calls[1])  # identical POST, still two requests
        # Resuming the same cycle reuses its own published slots: no new request.
        again = self.refresh(transport, ZERO[1], account='account-resume').capture()
        self.assertEqual(again['outcome'], 'captured')
        self.assertEqual({s['outcome'] for s in again['steps']}, {'reused'})
        self.assertEqual(len(transport.calls), 4)
        # A new cycle never reuses an older cycle's observation.
        self.refresh(transport, ZERO[1], cycle='cycle-2').capture()
        self.assertEqual(len(transport.calls), 8)

    def test_changed_after_export_fails_and_declared_reorder_passes(self):
        before = (ROOT / NONEMPTY[1] / 'before.csv').read_bytes()
        changed = before.replace(b'Edge-Core', b'Edge-Corf', 1)
        self.assertNotEqual(changed, before)
        refresh = self.refresh(SavedRra(NONEMPTY[1], after=changed), NONEMPTY[1])
        result = refresh.bundle(refresh.capture(), self.root / 'changed')
        self.assertEqual(result['outcome'], 'failed')
        self.assertIn('changed during capture', result['reason'])
        self.assertFalse((self.root / 'changed' / 'bundle.json').exists())
        lines = before.decode('cp949').splitlines(keepends=True)
        start = next(n for n, line in enumerate(lines) if line.startswith('번호')) + 1
        rows = [line for line in lines[start:] if line.strip()][::-1]
        # Only generated row numbers and order change; every certificate's cells stay exact.
        reordered = ''.join(lines[:start]) + ''.join(str(n) + row[row.index(','):] for n, row in enumerate(rows, 1))
        refresh = self.refresh(SavedRra(NONEMPTY[1], after=reordered.encode('cp949')), NONEMPTY[1],
                               cycle='cycle-reorder', comparison='exact_rows_ignore_order')
        self.assertEqual(refresh.bundle(refresh.capture(), self.root / 'reordered')['outcome'], 'validated_nonempty')

    def test_detail_cap_stops_before_any_detail_request(self):
        transport = SavedRra(NONEMPTY[1])
        captured = self.refresh(transport, NONEMPTY[1], max_details=3).capture()
        self.assertEqual(captured['outcome'], 'budget_exhausted')
        self.assertEqual([c[1] for c in transport.calls], [rra.FORM, rra.CSV_URL])
        with self.assertRaises(ValueError):
            self.refresh(transport, NONEMPTY[1], cycle='cycle-x').bundle(captured, self.root / 'x')

    def test_planner_derives_details_only_from_the_validated_before_csv(self):
        scope = RraScope.build(json.loads((ROOT / NONEMPTY[1] / 'bundle.json').read_text())['query'], 7)
        before = (ROOT / NONEMPTY[1] / 'before.csv').read_bytes()
        observed, urls = {'form': b'', 'csv-before': before}, []
        while (work := next_work(scope, observed)) and work['slot'] != 'csv-after':
            urls.append(work['url'])
            observed[work['slot']] = b''
        self.assertEqual(urls, [rra.DETAIL + row[-1] for row in rra.csv_rows(before)])
        with self.assertRaises(DetailCapExceeded):
            next_work(RraScope.build(dict(scope.query), 6), {'form': b'', 'csv-before': before})
        with self.assertRaises(ValueError):
            RraScope.build(dict(scope.query, firm='x'), 7)

    def test_denied_detail_blocks_durably_and_is_never_retried(self):
        transport = SavedRra(NONEMPTY[1], statuses={'detail-003': 403})
        captured = self.refresh(transport, NONEMPTY[1]).capture()
        self.assertEqual(captured['outcome'], 'blocked')
        calls = len(transport.calls)
        self.assertEqual(calls, 5)  # form, before, detail 1-3
        self.assertTrue(self.state.restrictions(transport.bundle['details'][2]['url']))
        again = self.refresh(transport, NONEMPTY[1], account='account-again').capture()
        self.assertEqual(again['outcome'], 'blocked')
        self.assertEqual(len(transport.calls), calls)
        self.assert_no_secret()

    def test_throttled_export_checkpoints_the_host(self):
        transport = SavedRra(NONEMPTY[1], statuses={'csv-before': 429})
        self.assertEqual(self.refresh(transport, NONEMPTY[1]).capture()['outcome'], 'throttled')
        again = self.refresh(transport, NONEMPTY[1], cycle='cycle-2').capture()
        self.assertEqual(again['outcome'], 'throttled')
        self.assertEqual(len(transport.calls), 2)

    def test_quota_exhaustion_never_publishes_a_partial_bundle(self):
        transport = SavedRra(NONEMPTY[1])
        refresh = self.refresh(transport, NONEMPTY[1], requests=5)
        captured = refresh.capture()
        self.assertEqual(captured['outcome'], 'budget_exhausted')
        self.assertEqual(len(transport.calls), 5)
        with self.assertRaises(ValueError):
            refresh.bundle(captured, self.root / 'partial')
        self.assertFalse((self.root / 'partial').exists())

    def test_invalid_detail_page_fails_the_unit(self):
        transport = SavedRra(NONEMPTY[1], replace={'detail-002': b'<html><body>error</body></html>'})
        captured = self.refresh(transport, NONEMPTY[1]).capture()
        self.assertEqual(captured['outcome'], 'failed')
        self.assertEqual(captured['steps'][-1]['slot'], 'detail-002')

    def test_requests_are_paced_and_bound_to_the_unit_revision(self):
        transport = SavedRra(NONEMPTY[1])
        refresh = self.refresh(transport, NONEMPTY[1])
        captured = refresh.capture()
        self.assertEqual(self.pauses, [0.15] * 9)  # between ten dispatches
        self.assertEqual(captured['new_observations'], 10)
        with self.state.db() as db:
            self.assertEqual({r[0] for r in db.execute('SELECT unit_revision FROM reservations')}, {'unit/fixture'})
        again = self.refresh(transport, NONEMPTY[1], account='account-again')
        reused = again.capture()
        self.assertEqual((self.pauses, reused['new_observations']), ([], 0))  # reuse never waits
        with self.assertRaisesRegex(ValueError, 'unit revision'):
            self.refresh(transport, NONEMPTY[1], cycle='cycle-2', unit_revision='capture/1').capture()

    def test_unparseable_responses_are_invalid_results_not_stranded_reservations(self):
        transport = SavedRra(NONEMPTY[1], replace={'csv-before': b'\xb9\xf8\xc8\xa3\n"abc'})
        captured = self.refresh(transport, NONEMPTY[1]).capture()
        self.assertEqual(captured['outcome'], 'failed')
        self.assertEqual(captured['steps'][-1]['error'], 'ValueError')
        with self.state.db() as db:
            self.assertEqual({r[0] for r in db.execute('SELECT state FROM reservations')}, {'complete'})
            self.assertIsNone(db.execute("SELECT stopped FROM accounts WHERE id='account-cycle-1'").fetchone()[0])
        # Resuming the cycle keeps the original failure name; nothing is re-requested.
        again = self.refresh(transport, NONEMPTY[1], account='account-again').capture()
        self.assertEqual((again['outcome'], again['steps'][-1]['outcome']), ('failed', 'invalid'))
        self.assertEqual(len(transport.calls), 2)

    def test_transport_failure_is_reported_as_interrupted(self):
        class Broken(SavedRra):
            def __call__(self, method, url, body, headers, timeout, max_bytes, *, observe_response):
                if url == rra.CSV_URL:
                    self.calls.append((method, url, body))
                    raise OSError('connection reset')
                return super().__call__(method, url, body, headers, timeout, max_bytes, observe_response=observe_response)
        captured = self.refresh(Broken(NONEMPTY[1]), NONEMPTY[1]).capture()
        self.assertEqual(captured['outcome'], 'interrupted')
        self.assertIsNone(captured['steps'][-1]['status'])

    def test_slot_scopes_are_unambiguous_for_colon_identities(self):
        transport = SavedRra(ZERO[1])
        a = self.refresh(transport, ZERO[1], cycle='b:c', unit_revision='unit/fixture')
        a.unit_revision = 'a'
        b = self.refresh(transport, ZERO[1], cycle='c', unit_revision='unit/fixture')
        b.unit_revision = 'a:b'
        self.assertNotEqual(a.slot_scope('form'), b.slot_scope('form'))

    def test_crash_after_published_result_is_reconciled_without_redispatch(self):
        transport = SavedRra(NONEMPTY[1])
        crashes = []

        def fault(point):
            if point == 'after_result_publication' and len(transport.calls) == 4 and not crashes:
                crashes.append(point)
                raise KeyboardInterrupt()
        with self.assertRaises(KeyboardInterrupt):
            self.refresh(transport, NONEMPTY[1], fault=fault).capture()
        # The interrupted reservation stops the account until explicit reconciliation.
        stopped = self.refresh(transport, NONEMPTY[1], account='account-cycle-1').capture()
        self.assertEqual(stopped['outcome'], 'stopped')
        self.assertEqual(len(transport.calls), 4)
        with self.state.db() as db:
            uncertain = dict(db.execute("SELECT * FROM reservations WHERE state='uncertain'").fetchone())
        result = next(r for r in self.archive.receipts('capture_result')
                      if r['payload'].get('reservation_id') == uncertain['id'])
        self.state.reconcile_result(uncertain['id'], archive=self.archive, result_id=result['id'],
                                    decision='fixture: result published before crash')
        self.state.resume_account(self.archive, 'account-cycle-1', decision='fixture: reconciled interruption')
        refresh = self.refresh(transport, NONEMPTY[1], account='account-cycle-1')
        captured = refresh.capture()
        self.assertEqual(captured['outcome'], 'captured')
        self.assertEqual(len(transport.calls), 10)  # the crashed slot was reused, not re-requested
        self.assertEqual(refresh.bundle(captured, self.root / 'out')['outcome'], 'validated_nonempty')


if __name__ == '__main__':
    unittest.main()


CELESTICA = ('rra-official-query-41e4ca912b33', 'data/raw/rra-celestica-official')


class RraLiveBundleCarryTests(unittest.TestCase):
    """Reviewed company evidence carry and location-insensitive pinned comparison."""
    # Reuse the isolated-archive fixture without inheriting (and re-running) its tests.
    setUp, tearDown, refresh = RraLiveTests.setUp, RraLiveTests.tearDown, RraLiveTests.refresh

    def saved_copy(self, directory, name='saved'):
        """A throwaway copy of a saved bundle's declaration + evidence files only."""
        import shutil
        saved = ROOT / directory
        dest = self.root / name
        dest.mkdir()
        shutil.copy(saved / 'bundle.json', dest / 'bundle.json')
        declared = json.loads((saved / 'bundle.json').read_text())['company_evidence']
        pending, seen = [declared], set()
        while pending:
            entry = pending.pop()
            if entry['path'] in seen:
                continue
            seen.add(entry['path'])
            shutil.copy(saved / entry['path'], dest / entry['path'])
            data = json.loads((saved / entry['path']).read_text())
            pending += ([data['base']] if 'base' in data else []) + list(data.get('exhibits', {}).values())
        return dest / 'bundle.json'

    def test_celestica_carried_evidence_restores_the_pinned_eligible_rows(self):
        refresh = self.refresh(SavedRra(CELESTICA[1]), CELESTICA[1], requests=200)
        captured = refresh.capture()
        self.assertEqual(captured['outcome'], 'captured')
        plain = refresh.bundle(captured, self.root / 'plain')
        self.assertNotIn('company_evidence', json.loads(Path(plain['bundle']).read_text()))
        self.assertEqual(RraSavedAdapter().records(Path(plain['bundle']), 'plain/bundle.json')
                         ['stats']['eligible_manufacturer_rows'], 48)
        saved = ROOT / CELESTICA[1] / 'bundle.json'
        result = refresh.bundle(captured, self.root / 'carried', carry_from=saved)
        self.assertEqual(result['outcome'], 'validated_nonempty')
        live = json.loads(Path(result['bundle']).read_text())
        old = json.loads(saved.read_text())
        self.assertEqual(live['company_evidence'], old['company_evidence'])
        # first_result_page is an old-session observation: never carried.
        self.assertIn('first_result_page', old)
        self.assertNotIn('first_result_page', live)
        self.assertFalse((self.root / 'carried' / 'result-first.html').exists())
        for name in ('celestica-historical-companies.json', 'celestica-subsidiaries-2025.json',
                     'celestica-sec-8.1-reviewed.json', 'celestica-archived-21.1-reviewed.json'):
            self.assertEqual((self.root / 'carried' / name).read_bytes(), (ROOT / CELESTICA[1] / name).read_bytes())
        records = RraSavedAdapter().records(Path(result['bundle']), 'carried/bundle.json')
        expected = pinned(CELESTICA[0])['expectations']
        self.assertEqual(records['stats']['eligible_manufacturer_rows'], expected['eligible_manufacturer_rows'])
        self.assertEqual(records['stats']['model_assertions'], expected['model_assertions'])
        self.assertEqual(compare_live(records, expected),
                         dict(status='matches_pinned', mismatches=[], location_only=True))

    def test_composite_cell_wrapper_is_carried_with_its_base_and_exhibits(self):
        refresh = self.refresh(SavedRra(CELESTICA[1]), CELESTICA[1], requests=200)
        captured = refresh.capture()
        saved = ROOT / CELESTICA[1] / 'bundle-composite-cells.json'
        result = refresh.bundle(captured, self.root / 'composite', carry_from=saved)
        self.assertEqual(result['outcome'], 'validated_nonempty')
        for name in ('rra-composite-manufacturer-cells-2026-10-11.json', 'celestica-historical-companies.json',
                     'celestica-subsidiaries-2025.json', 'celestica-sec-8.1-reviewed.json',
                     'celestica-archived-21.1-reviewed.json'):
            self.assertEqual((self.root / 'composite' / name).read_bytes(), (ROOT / CELESTICA[1] / name).read_bytes())
        records = RraSavedAdapter().records(Path(result['bundle']), 'composite/bundle.json')
        self.assertEqual((records['stats']['eligible_manufacturer_rows'], records['stats']['model_assertions']), (106, 154))

    def test_tampered_evidence_refuses_the_bundle(self):
        saved = self.saved_copy(CELESTICA[1])
        victim = saved.parent / 'celestica-subsidiaries-2025.json'
        victim.write_bytes(victim.read_bytes() + b' ')
        refresh = self.refresh(SavedRra(CELESTICA[1]), CELESTICA[1], requests=200)
        captured = refresh.capture()
        with self.assertRaises(ValueError):
            refresh.bundle(captured, self.root / 'refused', carry_from=saved)
        self.assertFalse((self.root / 'refused').exists())
        # Top-level declaration mismatch refuses too.
        saved = self.saved_copy(CELESTICA[1], 'saved2')
        top = saved.parent / 'celestica-historical-companies.json'
        top.write_bytes(top.read_bytes() + b'\n')
        with self.assertRaises(ValueError):
            refresh.bundle(captured, self.root / 'refused2', carry_from=saved)

    def test_compare_live_ignores_location_but_not_content(self):
        expected = pinned(NONEMPTY[0])['expectations']
        refresh = self.refresh(SavedRra(NONEMPTY[1]), NONEMPTY[1])
        result = refresh.bundle(refresh.capture(), self.root / 'elsewhere' / 'unit')
        records = RraSavedAdapter().records(Path(result['bundle']), 'elsewhere/unit/bundle.json')
        self.assertEqual(RraSavedAdapter().compare(records, expected)['status'], 'differs_from_pinned')
        self.assertEqual(compare_live(records, expected),
                         dict(status='matches_pinned', mismatches=[], location_only=True))
        self.assertEqual(compare_live(records, None)['status'], 'not_pinned')
        detail = (ROOT / NONEMPTY[1] / 'detail-001.html').read_bytes()
        changed = detail.replace(b'<td>AS7712-32X</td>', b'<td>AS7712-32Y</td>', 1)
        self.assertNotEqual(changed, detail)
        refresh = self.refresh(SavedRra(NONEMPTY[1], replace={'detail-001': changed}), NONEMPTY[1], cycle='cycle-2')
        result = refresh.bundle(refresh.capture(), self.root / 'elsewhere' / 'mutated')
        self.assertEqual(result['outcome'], 'validated_nonempty')
        records = RraSavedAdapter().records(Path(result['bundle']), 'elsewhere/mutated/bundle.json')
        verdict = compare_live(records, expected)
        self.assertEqual(verdict['status'], 'differs_from_pinned')
        self.assertFalse(verdict['location_only'])
        self.assertIn('validated_original_records_sha256', verdict['mismatches'])

    def test_collisions_wrong_scope_and_midwrite_failure_leave_nothing(self):
        refresh = self.refresh(SavedRra(NONEMPTY[1]), NONEMPTY[1])
        captured = refresh.capture()
        # Evidence named like a generated file is refused before the directory exists.
        saved = self.saved_copy(CELESTICA[1], 'coll')
        top = saved.parent / 'celestica-historical-companies.json'
        data = json.loads(top.read_text())
        (saved.parent / 'before.csv').write_bytes((saved.parent / 'celestica-subsidiaries-2025.json').read_bytes())
        data['base']['path'] = 'before.csv'
        top.write_text(json.dumps(data))
        meta = json.loads(saved.read_text())
        meta['company_evidence']['sha256'] = rra.digest(top.read_bytes())
        saved.write_text(json.dumps(meta))
        with self.assertRaises(ValueError) as ctx:
            refresh.bundle(captured, self.root / 'c1', carry_from=saved)
        self.assertIn('collides', str(ctx.exception))
        self.assertFalse((self.root / 'c1').exists())
        # Evidence from another query scope is refused.
        other = self.saved_copy(CELESTICA[1], 'other')
        with self.assertRaises(ValueError) as ctx:
            refresh.bundle(captured, self.root / 'c2', carry_from=other)
        self.assertIn('query scope', str(ctx.exception))
        self.assertFalse((self.root / 'c2').exists())
        # A failure after mkdir removes the half-written directory.
        with patch('devicehunt.campaign_rra_live.rra.validate', side_effect=RuntimeError('boom')):
            with self.assertRaises(RuntimeError):
                refresh.bundle(captured, self.root / 'c3')
        self.assertFalse((self.root / 'c3').exists())
