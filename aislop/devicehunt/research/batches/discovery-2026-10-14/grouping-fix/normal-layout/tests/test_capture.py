import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from devicehunt.artifacts import Archive
from devicehunt.campaign_state import CampaignState, StateError
from devicehunt.capture import Collector, Policy, Response, LeaseBusy, StopCollection, fingerprint


class CaptureTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.archive = Archive(Path(self.tmp.name) / 'archive', create=True)
        self.url = 'https://example.test/a'
        self.calls = []
        self.state = CampaignState.initialize(self.archive.root)
        self.state.allocate_account('test',definition_hash='test',policy_hash='capture-policy/1',request_limit=100,byte_limit=2000000000)
        self.state.synchronize_archive(self.archive,decision='capture fixture safety binding',policy_hash='capture-policy/1')
        for entry in ('socket.socket.connect','socket.create_connection','urllib.request.OpenerDirector.open','subprocess.run','subprocess.Popen'):
            guard = patch(entry,side_effect=AssertionError('forbidden network/external entrypoint'))
            guard.start()
            self.addCleanup(guard.stop)

    def tearDown(self):
        self.archive.db.close()
        self.tmp.cleanup()

    def collector(self, response=None, **kw):
        def transport(*args, observe_response):
            self.calls.append(args)
            result = response or Response(200, self.url, {'ETag': 'v1'}, b'original')
            observe_response(result.status,result.final_url,result.headers)
            return result
        return Collector(self.archive, Policy(frozenset([self.url]), budget=4, **kw), transport,
                         state=self.state,account_id='test',mock_transport=True)

    def test_exact_identity(self):
        self.assertNotEqual(fingerprint('GET', self.url), fingerprint('GET', self.url + '?x=1'))
        self.assertNotEqual(fingerprint('GET', self.url), fingerprint('GET', self.url, headers={'Accept': 'x'}))
        self.assertNotEqual(fingerprint('POST', self.url, b'a'), fingerprint('POST', self.url, b'b'))

    def test_reuse_and_refresh_304(self):
        first = self.collector().capture(self.url)
        reused = self.collector().capture(self.url)
        self.assertEqual(reused['prior_capture_id'], first['id'])
        self.assertNotIn('started_at', reused)
        c = self.collector(Response(304, self.url, {}, b''))
        refreshed = c.capture(self.url, refresh=True)
        self.assertEqual(refreshed['artifact_sha256'], first['artifact_sha256'])
        self.assertEqual(self.calls[-1][3]['If-None-Match'], 'v1')
        attempt = self.archive.get_receipt(refreshed['attempt_id'])['payload']
        self.assertEqual(attempt['headers']['If-None-Match'],'v1')
        self.assertEqual(attempt['conditional_prior_capture_id'],first['id'])
        self.assertEqual(attempt['request_id'],first['request_id'])
        self.assertNotEqual(attempt['wire_request_id'],attempt['request_id'])
        self.assertEqual(self.collector().capture(self.url)['prior_capture_id'],refreshed['id'])
        self.assertEqual(len(self.calls), 2)
        self.assertEqual(len(self.archive.receipts('capture_attempt')), 2)

    def test_304_without_prior_invalid(self):
        result = self.collector(Response(304, self.url, {}, b'')).capture(self.url)
        self.assertEqual(result['outcome'], 'invalid')

    def test_saved_denial_and_new_denial_body(self):
        c = self.collector(prior_failures=({'url': self.url, 'error': 'HTTP Error 403: Forbidden'},))
        self.assertEqual(c.capture(self.url)['outcome'], 'blocked')
        self.assertEqual(self.calls, [])
        self.state.synchronize_archive(self.archive,decision='review prior source denial',policy_hash='capture-policy/1')
        self.url += '/new'
        c = self.collector(Response(403, self.url, {}, b'denied'))
        result = c.capture(self.url)
        self.assertEqual(self.archive.resolve(result['response_sha256']).read_bytes(), b'denied')
        self.assertEqual(c.capture(self.url, refresh=True)['outcome'], 'blocked')
        self.assertEqual(len(self.calls), 1)

    def test_invalid_pdf_and_empty_unavailable_distinct(self):
        for status, body, expected in [(200, b'<html>login</html>', 'invalid'),
                                       (200, b'', 'empty'), (404, b'missing', 'unavailable')]:
            result = self.collector(Response(status, self.url, {}, body)).capture(self.url, expected='pdf', refresh=True)
            self.assertEqual(result['outcome'], expected)
            self.assertNotIn('artifact_sha256', result)

    def test_throttle_checkpoint(self):
        c = self.collector(Response(429, self.url, {}, b'slow'))
        self.assertEqual(c.capture(self.url)['outcome'], 'throttled')
        with self.assertRaises(StopCollection): c.capture(self.url)

    def test_lease_contention_expiry_and_interrupt(self):
        c = self.collector()
        c.clock = lambda: 10
        keys = ['host:example.test']
        c._lease(keys)
        other = self.collector()
        other.clock = lambda: 11
        with self.assertRaises(LeaseBusy): other._lease(keys)
        other.clock = lambda: 200
        with self.assertRaises(LeaseBusy): other._lease(keys)
        c._release(keys)
        other._lease(keys)
        c._release(keys)  # stale owner cannot clear a replacement lease
        with self.assertRaises(LeaseBusy): c._lease(keys)
        other._release(keys)
        def interrupt(*args, **kwargs): raise KeyboardInterrupt()
        c.transport = interrupt
        with self.assertRaises(KeyboardInterrupt): c.capture(self.url)
        self.assertEqual(len(self.archive.receipts('capture_attempt')), 1)
        self.assertEqual(len(self.archive.receipts('capture_result')), 0)
        c._lease(keys)
        c._release(keys)

    def test_corrupt_cache_not_reused(self):
        result = self.collector().capture(self.url)
        path = self.archive.resolve(result['artifact_sha256'])
        path.chmod(0o644)
        path.write_bytes(b'corrupt')
        with self.assertRaises((ValueError, OSError)): self.collector().capture(self.url)
        self.assertEqual(len(self.calls), 1)

    def test_timeout_and_size_bound(self):
        c = self.collector()
        def timeout(*args, **kwargs): raise TimeoutError('expired')
        c.transport = timeout
        self.assertEqual(c.capture(self.url)['outcome'], 'timeout')
        # Uncertain timeouts block even newly allocated accounts. Use a separate
        # isolated authority to test the independent oversize result.
        self.archive.close()
        self.archive = Archive(Path(self.tmp.name)/'size-archive',create=True)
        self.state = CampaignState.initialize(self.archive.root)
        self.state.allocate_account('test',definition_hash='test',policy_hash='capture-policy/1',request_limit=10,byte_limit=1000)
        self.state.synchronize_archive(self.archive,decision='size fixture binding',policy_hash='capture-policy/1')
        c = self.collector(Response(200, self.url, {}, b'long'), max_bytes=3)
        result = c.capture(self.url, refresh=True)
        self.assertEqual(result['outcome'], 'invalid')
        self.assertTrue(result['body_truncated'])
        self.assertEqual(self.archive.resolve(result['response_sha256']).read_bytes(), b'lon')

    def test_historical_capture_and_persistent_failure(self):
        import hashlib
        saved = Path(self.tmp.name) / 'saved.xml'
        saved.write_bytes(b'<urlset/>')
        row = {'url': self.url, 'status': 200, 'sha256': hashlib.sha256(saved.read_bytes()).hexdigest(),
               'checked_at': 'historical-time'}
        result = self.collector().register_saved(row, saved)
        self.assertEqual(result['historical'], row)
        self.assertNotIn('started_at', result)
        offline = Collector(self.archive,Policy(frozenset([self.url])))
        self.assertEqual(offline.capture(self.url)['outcome'], 'reused')
        self.assertEqual(self.calls, [])
        self.url += '/failed'
        self.collector(prior_failures=({'url': self.url, 'error': 'historical timeout'},))
        self.assertEqual(self.collector().capture(self.url)['outcome'], 'saved_unavailable')
        self.assertEqual(self.calls, [])

    def test_changed_version_and_deduplicated_bytes(self):
        first = self.collector().capture(self.url)
        latest = self.collector(Response(200, self.url, {}, b'new')).capture(self.url, refresh=True)
        self.assertNotEqual(first['artifact_sha256'], latest['artifact_sha256'])
        self.assertEqual(self.collector().capture(self.url)['artifact_sha256'], latest['artifact_sha256'])
        self.url += '/other'
        other = self.collector(Response(200, self.url, {}, b'new')).capture(self.url)
        self.assertEqual(other['artifact_sha256'], latest['artifact_sha256'])
        self.assertNotEqual(other['id'], latest['id'])

    def test_error_reuse_and_cache_validation(self):
        first = self.collector(Response(404, self.url, {}, b'error')).capture(self.url)
        self.assertEqual(self.collector().capture(self.url)['prior_capture_id'], first['id'])
        self.assertEqual(len(self.calls), 1)
        self.url += '/html'
        self.collector(Response(200, self.url, {}, b'<html/>')).capture(self.url)
        with self.assertRaises(ValueError): self.collector().capture(self.url, expected='pdf')
        self.assertEqual(len(self.calls), 2)

    def test_saved_receipt_idempotence(self):
        import hashlib
        path = Path(self.tmp.name) / 'saved'
        path.write_bytes(b'saved')
        row = {'url': self.url, 'status': 200, 'sha256': hashlib.sha256(b'saved').hexdigest()}
        c = self.collector()
        a = c.register_saved(row, path, locator={'report': 'r', 'ordinal': 1})
        b = c.register_saved(row, path, locator={'report': 'r', 'ordinal': 1})
        distinct = c.register_saved(row, path, locator={'report': 'r', 'ordinal': 2})
        self.assertEqual(a['id'], b['id'])
        self.assertNotEqual(a['id'], distinct['id'])
        self.assertEqual(len(self.archive.receipts('capture_result')), 2)

    def test_saved_in_place_registration_keeps_saved_id_without_copy(self):
        import hashlib
        workspace = Path(self.tmp.name)
        path = workspace / 'saved-in-place'
        path.write_bytes(b'in place')
        row = {'url': self.url, 'status': 200, 'sha256': hashlib.sha256(b'in place').hexdigest()}
        # The default workspace guess (root.parent) never authorizes in-place locations.
        with self.assertRaisesRegex(ValueError, 'explicit workspace'):
            self.collector().register_saved(row, path, copy=False)
        explicit = Archive(self.archive.root, workspace=workspace)
        try:
            in_place = Collector(explicit, Policy(frozenset([self.url]))).register_saved(row, path, copy=False)
            self.assertFalse(explicit.object_path(row['sha256']).exists())
            self.assertEqual(explicit.resolve(row['sha256']), path)
            again = Collector(explicit, Policy(frozenset([self.url]))).register_saved(row, path, copy=False)
            copied = self.collector().register_saved(row, path)
            self.assertEqual(in_place['id'], again['id'])
            self.assertEqual(in_place['id'], copied['id'])
            with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
                Collector(explicit, Policy(frozenset([self.url]))).register_saved(dict(row, sha256='0' * 64), path, copy=False)
        finally:
            explicit.close()

    def test_saved_slots_stay_distinct_and_offline_lookup_honours_scope(self):
        import hashlib
        workspace = Path(self.tmp.name)
        explicit = Archive(self.archive.root, workspace=workspace)
        try:
            rows = {}
            for slot, payload in (('before', b'csv-before'), ('after', b'csv-after')):
                path = workspace / slot
                path.write_bytes(payload)
                row = {'url': self.url, 'status': 200, 'sha256': hashlib.sha256(payload).hexdigest()}
                rows[slot] = Collector(explicit, Policy(frozenset([self.url]))).register_saved(
                    row, path, copy=False, method='POST', body=b'maker=x', source='rra',
                    observation_scope='unit/1:' + slot, slot=slot, unit_revision='unit/1')
            self.assertNotEqual(rows['before']['request_id'], rows['after']['request_id'])
            self.assertEqual(rows['after']['slot'], 'after')
            for slot in ('before', 'after'):
                policy = Policy(frozenset([self.url]), source='rra', observation_scope='unit/1:' + slot)
                # Offline replay needs the same request identity the slot was saved under.
                found = Collector(explicit, policy)._cached(rows[slot]['request_id'])
                self.assertEqual([r['id'] for r in found], [rows[slot]['id']])
            # A saved row declared with the live request headers has the live request identity.
            headers = {'Content-Type': 'application/x-www-form-urlencoded'}
            saved = Collector(explicit, Policy(frozenset([self.url]))).register_saved(
                {'url': self.url, 'status': 200, 'sha256': hashlib.sha256(b'csv-after').hexdigest()},
                workspace / 'after', copy=False, method='POST', body=b'maker=x', headers=headers,
                source='rra', observation_scope='unit/1:headers', slot='after')
            policy = Policy(frozenset([self.url]), source='rra', observation_scope='unit/1:headers')
            replay = Collector(explicit, policy).capture(self.url, method='POST', body=b'maker=x', headers=headers)
            self.assertEqual((replay['outcome'], replay['prior_capture_id']), ('reused', saved['id']))
            self.assertNotIn('Content-Type', json.dumps(explicit.get_receipt(saved['id'])['payload']))
            with self.assertRaisesRegex(ValueError, 'observation scope'):
                Collector(explicit, Policy(frozenset([self.url]))).register_saved(
                    {'url': self.url, 'status': 403}, slot='orphan')
            # Legacy undeclared registration keeps its historical request identity.
            legacy = Collector(explicit, Policy(frozenset([self.url]))).register_saved(
                {'url': self.url, 'status': 403, 'error': None})
            self.assertEqual(legacy['request_id'], fingerprint('GET', self.url))
            self.assertNotIn('slot', legacy)
        finally:
            explicit.close()

    def test_saved_throttle_never_stops_offline_replay_of_another_url(self):
        throttled = self.url + '/busy'
        self.collector().register_saved({'url': throttled, 'status': 503})
        other = self.url + '/other'
        offline = Collector(self.archive, Policy(frozenset([other])), state=self.state)
        self.assertEqual(offline.capture(other)['outcome'], 'offline_unavailable')
        with self.assertRaises(StopCollection):  # live dispatch still checkpoints
            Collector(self.archive, Policy(frozenset([other]), budget=4), lambda *a, **k: None,
                      state=self.state, account_id='test', mock_transport=True).capture(other)
        self.assertEqual(self.calls, [])

    def test_body_protocol_error_after_denial_publishes_reconcilable_result(self):
        import http.client
        def transport(*args, observe_response):
            self.calls.append(args)
            observe_response(403, self.url, {})
            raise http.client.IncompleteRead(b'partial')
        collector = Collector(self.archive, Policy(frozenset([self.url]), budget=4), transport,
                              state=self.state, account_id='test', mock_transport=True)
        result = collector.capture(self.url)
        self.assertEqual((result['outcome'], result['status'], result['error']), ('blocked', 403, 'IncompleteRead'))
        reservation = self.state.reservation(result['reservation_id'])
        self.assertEqual(reservation['state'], 'uncertain')
        done = self.state.reconcile_result(reservation['id'], archive=self.archive, result_id=result['id'],
                                           decision='fixture: denial result published despite truncated body')
        self.assertEqual(done['state'], 'complete')
        self.assertTrue(self.state.restrictions(self.url))

    def test_saved_status_policy_and_postlease_race(self):
        c = self.collector()
        denied = c.register_saved({'url': self.url, 'status': 403, 'error': None})
        self.assertEqual(denied['outcome'], 'blocked')
        self.url += '/throttled'
        c = self.collector()
        self.assertEqual(c.register_saved({'url': self.url, 'status': 429})['outcome'], 'throttled')
        with self.assertRaises(StopCollection): self.collector().capture(self.url)
        self.url = 'https://another.test/race'
        c = self.collector()
        real_lease = c._lease
        def race(keys):
            c._receipt('capture_result', {'url': self.url, 'request_id': fingerprint('GET', self.url),
                                        'status': 404, 'outcome': 'unavailable','request_context_known': True,'source':None,'session_scope':None,'observation_scope':None})
            real_lease(keys)
        c._lease = race
        self.assertEqual(c.capture(self.url)['outcome'], 'saved_unavailable')
        self.assertEqual(self.calls, [])

    def test_304_redirect_and_saved_symlink_rejected(self):
        import hashlib
        self.collector().capture(self.url)
        result = self.collector(Response(304, self.url + '/redirect', {}, b'')).capture(self.url, refresh=True)
        self.assertEqual(result['outcome'], 'invalid')
        path = Path(self.tmp.name) / 'original'
        path.write_bytes(b'saved')
        link = Path(self.tmp.name) / 'linked'
        link.symlink_to(path)
        row = {'url': self.url, 'status': 200, 'sha256': hashlib.sha256(b'saved').hexdigest()}
        with self.assertRaises(ValueError): self.collector().register_saved(row, link)

    def test_adapter_rejects_invalid_saved_xml_before_writes(self):
        import hashlib
        import importlib.util
        import json
        spec = importlib.util.spec_from_file_location('fcc_adapter', Path(__file__).resolve().parents[1] / 'research/fetch_fcc_sitemaps.py')
        adapter = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(adapter)
        saved = Path(self.tmp.name) / 'saved'
        saved.mkdir()
        url = 'https://fccid.co/sitemaps/main.xml'
        index = ('<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><sitemap><loc>' + url + '</loc></sitemap></sitemapindex>').encode()
        (saved / 'index.xml').write_bytes(index)
        (saved / 'main.xml').write_bytes(b'<html>error</html>')
        initial = [{'url': 'https://fccid.co/sitemap.xml', 'status': 200, 'path': 'index.xml', 'sha256': hashlib.sha256(index).hexdigest()}]
        child = {'url': url, 'status': 200, 'path': 'main.xml', 'sha256': hashlib.sha256(b'<html>error</html>').hexdigest()}
        (saved / 'fetch-report.json').write_text(json.dumps(initial))
        (saved / 'collection-report.json').write_text(json.dumps({'entries': [child]}))
        destination = Path(self.tmp.name) / 'new-archive'
        with self.assertRaises(ValueError):
            adapter.main(['--saved-root', str(saved), '--archive', str(destination), '--report-dir', str(saved / 'reports')])
        self.assertFalse(destination.exists())

    def test_saved_adapter_repeated_modern_open_and_damaged_existing_refusal(self):
        import hashlib,importlib.util,json
        module_path = Path(__file__).resolve().parents[1]/'research/fetch_fcc_sitemaps.py'
        spec = importlib.util.spec_from_file_location('fcc_adapter_compat',module_path)
        adapter = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(adapter)
        # The adapter's workspace identity is a real synthetic workspace in /tmp.
        adapter.__file__ = str(Path(self.tmp.name)/'research/fetch_fcc_sitemaps.py')
        saved = Path(self.tmp.name)/'saved-adapter'
        saved.mkdir()
        url = 'https://fccid.co/sitemaps/main.xml'
        index = ('<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><sitemap><loc>'+url+'</loc></sitemap></sitemapindex>').encode()
        child = b'<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"/>'
        (saved/'index.xml').write_bytes(index)
        (saved/'main.xml').write_bytes(child)
        initial = [{'url':'https://fccid.co/sitemap.xml','status':200,'path':'index.xml','sha256':hashlib.sha256(index).hexdigest()}]
        previous = [{'url':url,'status':200,'path':'main.xml','sha256':hashlib.sha256(child).hexdigest()}]
        (saved/'fetch-report.json').write_text(json.dumps(initial))
        (saved/'collection-report.json').write_text(json.dumps({'entries':previous}))
        root = Path(self.tmp.name)/'adapter-archive'
        reports = Path(self.tmp.name)/'adapter-reports'
        args = ['--saved-root',str(saved),'--archive',str(root),'--report-dir',str(reports)]
        self.assertEqual(adapter.main(args),0)
        archive = Archive(root)
        frozen = {p.name:p.read_bytes() for p in (root/'receipts').glob('*.json')}
        sequence = archive.require_ready()['sequence']
        archive.close()
        self.assertEqual(adapter.main(args),0)
        reopened = Archive(root)
        self.assertEqual(reopened.require_ready()['sequence'],sequence)
        self.assertEqual(frozen,{p.name:p.read_bytes() for p in (root/'receipts').glob('*.json')})
        reopened.close()
        # Existing root without index is damage, never a new-init opportunity.
        (root/'index.sqlite').unlink()
        with self.assertRaises(ValueError):
            adapter.main(args)
        self.assertFalse((root/'index.sqlite').exists())
        (root/'index.sqlite').write_bytes(b'corrupt isolated index')
        with self.assertRaises(ValueError):
            adapter.main(args)
        self.assertEqual((root/'index.sqlite').read_bytes(),b'corrupt isolated index')

    def test_redirect_never_accepted(self):
        result = self.collector(Response(302, 'https://denied.test', {'Location': 'https://denied.test'}, b'moved')).capture(self.url)
        self.assertEqual(result['outcome'], 'invalid')
        self.assertEqual(len(self.calls), 1)


if __name__ == '__main__':
    unittest.main()
