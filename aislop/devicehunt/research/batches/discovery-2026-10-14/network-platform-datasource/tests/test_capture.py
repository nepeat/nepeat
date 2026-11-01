from contextlib import redirect_stdout
from email.message import Message
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('bounded_capture', ROOT / 'capture.py')
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)


class Response:
    def __init__(self, status=200, body=b'User-agent: *\nDisallow:\n', location=None):
        self.status, self.body = status, body
        self.headers = Message()
        self.headers['Content-Type'] = 'text/plain'
        if location:
            self.headers['Location'] = location

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False

    def read(self, count):
        return self.body[:count]


class CaptureTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / 'raw').mkdir()
        self.owner = patch.object(capture, 'OWN', self.root)
        self.owner.start()
        self.addCleanup(self.owner.stop)
        self.opener = patch.object(capture.urllib.request, 'build_opener')
        self.mock = self.opener.start()
        self.addCleanup(self.opener.stop)

    def acquire(self, *args, **kwargs):
        with redirect_stdout(io.StringIO()):
            return capture.acquire(*args, **kwargs)

    def ledger(self):
        return [json.loads(line) for line in (self.root / 'requests.jsonl').read_text().splitlines()]

    def test_allowlist_and_missing_policy_prevent_calls(self):
        for url in ('http://www.bisdn.de/robots.txt', 'https://example.org/robots.txt',
                    'https://www.bisdn.de/en/linux/compatibility/'):
            with self.assertRaises(ValueError):
                self.acquire(url, 'denied')
        self.mock.assert_not_called()

    def test_dns_failures_do_not_consume_http_budget(self):
        rows = [{'status': None, 'url': 'https://www.bisdn.de/robots.txt'}] * 10
        (self.root / 'requests.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in rows))
        self.mock.return_value.open.return_value = Response()
        self.acquire('https://www.bisdn.de/robots.txt', 'policy')
        self.assertEqual(self.ledger()[-1]['sequence'], 11)
        self.assertEqual(self.ledger()[-1]['status'], 200)

    def test_actual_http_budget_is_hard_stop(self):
        rows = [{'status': 200, 'url': 'https://www.bisdn.de/robots.txt'}] * 10
        (self.root / 'requests.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in rows))
        with self.assertRaisesRegex(ValueError, 'budget'):
            self.acquire('https://www.bisdn.de/robots.txt', 'policy')
        self.mock.assert_not_called()

    def test_orphaned_capture_file_is_never_overwritten(self):
        original = self.root / 'raw/01-policy.body'
        original.write_bytes(b'original interrupted capture')
        with self.assertRaisesRegex(ValueError, 'already exists'):
            self.acquire('https://www.bisdn.de/robots.txt', 'policy')
        self.assertEqual(original.read_bytes(), b'original interrupted capture')
        self.mock.assert_not_called()

    def test_robots_hash_and_denial_gate_content(self):
        self.mock.return_value.open.return_value = Response(body=b'User-agent: *\nDisallow: /en/\n')
        self.acquire('https://www.bisdn.de/robots.txt', 'policy')
        self.mock.reset_mock()
        with self.assertRaisesRegex(ValueError, 'denies'):
            self.acquire('https://www.bisdn.de/en/linux/compatibility/', 'matrix', 'raw/01-policy.json')
        self.mock.assert_not_called()
        (self.root / 'raw/01-policy.body').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'hash mismatch'):
            self.acquire('https://www.bisdn.de/en/linux/compatibility/', 'matrix', 'raw/01-policy.json')

    def test_denied_response_stops_host_and_redirect_is_not_followed(self):
        self.mock.return_value.open.return_value = Response(status=403)
        with self.assertRaises(SystemExit):
            self.acquire('https://www.bisdn.de/robots.txt', 'policy')
        self.mock.reset_mock()
        with self.assertRaisesRegex(ValueError, 'prior access restriction'):
            self.acquire('https://www.bisdn.de/robots.txt', 'policy2')
        self.mock.assert_not_called()
        handler = capture.NoRedirect()
        self.assertIsNone(handler.redirect_request(None, None, 302, 'redirect', {}, 'https://example.org/'))

    def test_oversized_actual_response_remains_counted_and_fails(self):
        self.mock.return_value.open.return_value = Response(body=b'x'*8_000_001)
        with self.assertRaises(SystemExit):
            self.acquire('https://www.bisdn.de/robots.txt', 'policy')
        self.assertEqual(self.ledger()[0]['status'], 200)
        self.assertEqual(self.ledger()[0]['error_type'], 'ValueError')

    def test_dns_failure_is_unavailable_not_empty(self):
        self.mock.return_value.open.side_effect = urllib.error.URLError('DNS failure')
        with self.assertRaises(SystemExit):
            self.acquire('https://www.bisdn.de/robots.txt', 'policy')
        row = self.ledger()[0]
        self.assertIsNone(row['status'])
        self.assertNotIn('body_path', row)


if __name__ == '__main__':
    unittest.main()
