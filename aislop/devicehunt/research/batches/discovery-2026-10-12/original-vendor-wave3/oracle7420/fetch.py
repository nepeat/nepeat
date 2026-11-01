"""Exactly one robots request and two declared pages, never follow redirects."""
import datetime
import hashlib
import json
from pathlib import Path
import urllib.error
import urllib.request
import urllib.robotparser

HERE = Path(__file__).resolve().parent
PREFIX = 'https://docs.oracle.com/en/storage/zfs-storage/zfs-appliance/os8-8-x/service-manual/'
PAGES = [('internal-components.html', PREFIX + 'sun-zfs-storage-7420-internal-components.html'),
         ('attached-storage.html', PREFIX + 'sun-zfs-storage-7420-attached-storage.html')]
AGENT = 'devicehunt/1.0 (bounded public documentation research)'


class NoRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def main():
    if (HERE / 'request-ledger.json').exists():
        raise RuntimeError('This finite capture has already started; do not repeat requests')
    ledger = []
    opener = urllib.request.build_opener(NoRedirects())

    def fetch(filename, url):
        assert len(ledger) < 3
        entry = {'request_number': len(ledger) + 1, 'method': 'GET', 'url': url,
                 'headers': {'User-Agent': AGENT}, 'timeout_seconds': 30,
                 'redirect_policy': 'disabled; no automatic additional HTTP requests',
                 'requested_at': datetime.datetime.now(datetime.timezone.utc).isoformat()}
        ledger.append(entry)
        request = urllib.request.Request(url, headers=entry['headers'])
        try:
            try:
                response = opener.open(request, timeout=30)
            except urllib.error.HTTPError as exc:
                response = exc
            with response:
                body = response.read()
                entry.update(status=response.code, final_url=response.geturl(),
                             content_type=response.headers.get('Content-Type'),
                             location=response.headers.get('Location'),
                             retry_after=response.headers.get('Retry-After'),
                             bytes=len(body), sha256=hashlib.sha256(body).hexdigest(), file=filename)
            path = HERE / filename
            with path.open('xb') as output:
                output.write(body)
            entry['outcome'] = 'captured' if entry['status'] == 200 else 'HTTP outcome; source stopped'
        except (urllib.error.URLError, TimeoutError, OSError) as exc:
            entry.update(status=None, outcome='unavailable; no retry', error=type(exc).__name__ + ': ' + str(exc))
            body = None
        entry['completed_at'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        (HERE / 'request-ledger.json').write_text(json.dumps(ledger, indent=2) + '\n')
        return entry, body

    robot, body = fetch('robots.txt', 'https://docs.oracle.com/robots.txt')
    stop = None
    if robot['status'] not in (200, 404):
        stop = 'robots unavailable/denied/redirected; no page requests'
    elif robot['status'] == 200:
        if body is None or b'<html' in body[:1000].lower():
            stop = 'robots response is not policy text; no page requests'
        else:
            policy = urllib.robotparser.RobotFileParser()
            policy.parse(body.decode('utf-8-sig').splitlines())
            robot['page_policy'] = {url: policy.can_fetch('devicehunt', url) for _, url in PAGES}
            robot['crawl_delay_seconds'] = policy.crawl_delay('devicehunt')
            if not all(robot['page_policy'].values()):
                stop = 'robots prohibits named source; no page requests'
            if robot['crawl_delay_seconds']:
                stop = 'nonzero robots crawl delay; checkpoint before page requests'
    if not stop:
        for filename, url in PAGES:
            entry, body = fetch(filename, url)
            if entry['status'] != 200:
                stop = 'named page non-success; no retry or additional Oracle requests'
                break
            text = body.decode('utf-8-sig', errors='replace').lower()
            if ('<title' in text and 'sign in' in text.split('</title>', 1)[0]) or 'login required' in text:
                entry['outcome'] = 'login-required page; source stopped'
                stop = 'login required; no additional Oracle requests'
                break
    (HERE / 'request-ledger.json').write_text(json.dumps(ledger, indent=2) + '\n')
    report = {'scope': 'robots plus two fixed Oracle7420 pages only; no redirects/retries/searches/screenshots',
              'actual_http_requests': len(ledger), 'cap': 3,
              'throttled': any(e['status'] == 429 for e in ledger),
              'stop_reason': stop or 'two named pages completed; stop condition met',
              'outcomes': [{'url': e['url'], 'status': e['status'], 'outcome': e['outcome']} for e in ledger]}
    (HERE / 'fetch-report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
