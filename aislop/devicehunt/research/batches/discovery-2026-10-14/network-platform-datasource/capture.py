"""Bounded HTTP acquisition only; deterministic parsing lives separately."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import time
import urllib.error
import urllib.parse
import urllib.request
import urllib.robotparser

OWN = Path(__file__).resolve().parent
AGENT = 'devicehunt-public-research/1.0'
HOSTS = {'www.nvidia.com', 'www.bisdn.de', 'platforms.cumulusnetworks.com', 'support.cumulusnetworks.com'}
CAP = 10

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None

def acquire(url, name, policy=None):
    parts = urllib.parse.urlsplit(url)
    if parts.scheme != 'https' or parts.netloc not in HOSTS or parts.username or parts.fragment:
        raise ValueError('URL is outside the fixed source host allowlist')
    if not name.replace('-', '').replace('_', '').isalnum():
        raise ValueError('invalid capture name')
    ledger = OWN / 'requests.jsonl'
    prior = [json.loads(line) for line in ledger.read_text().splitlines()] if ledger.exists() else []
    if sum(row.get('status') is not None for row in prior) >= CAP:
        raise ValueError('ten-HTTP-exchange acquisition budget exhausted')
    if any(row.get('status') in (401, 402, 403, 429) and urllib.parse.urlsplit(row['url']).netloc == parts.netloc for row in prior):
        raise ValueError('prior access restriction: no retry or alternate route')
    policy_context = None
    if parts.path != '/robots.txt':
        if policy is None:
            raise ValueError('a captured robots policy is required')
        receipt = json.loads((OWN / policy).read_text())
        if receipt['url'] != 'https://' + parts.netloc + '/robots.txt' or receipt['status'] not in (200, 404):
            raise ValueError('policy URL/status does not authorize this host')
        raw = (OWN / receipt['body_path']).read_bytes()
        if hashlib.sha256(raw).hexdigest() != receipt['body_sha256']:
            raise ValueError('robots policy hash mismatch')
        body = raw.decode('utf-8', errors='replace')
        if receipt['status'] == 200 and 'user-agent:' not in body.lower():
            raise ValueError('unrecognized robots response; do not infer permission')
        robot = urllib.robotparser.RobotFileParser(receipt['url'])
        robot.parse(body.splitlines() if receipt['status'] == 200 else [])
        if not robot.can_fetch(AGENT, url):
            raise ValueError('robots policy denies planned URL')
        delay = robot.crawl_delay(AGENT) or 0
        if delay > 60:
            raise ValueError('policy delay exceeds this bounded collector')
        if delay and prior:
            elapsed = datetime.datetime.now(datetime.timezone.utc).timestamp() - datetime.datetime.fromisoformat(prior[-1]['finished_utc']).timestamp()
            if elapsed < delay:
                time.sleep(delay - elapsed)
        policy_context = {'receipt': policy, 'sha256': receipt['body_sha256'], 'allowed': True, 'crawl_delay_seconds': delay}
    prefix = OWN / 'raw' / ('%02d-' % (len(prior) + 1) + name)
    if any(prefix.with_suffix(suffix).exists() for suffix in ('.body', '.headers', '.json')):
        raise ValueError('capture prefix already exists; preserve original captures')
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    row = {'sequence': len(prior) + 1, 'url': url, 'method': 'GET', 'user_agent': AGENT,
           'started_utc': started, 'automatic_redirects': False, 'retries': 0, 'policy': policy_context}
    try:
        request = urllib.request.Request(url, headers={'User-Agent': AGENT})
        opener = urllib.request.build_opener(NoRedirect)
        try:
            response = opener.open(request, timeout=45)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            # Count an actual exchange even if its body is too large or fails to read.
            row.update(status=response.status, location=response.headers.get('Location'),
                       content_type=response.headers.get('Content-Type'))
            data = response.read(8_000_001)
            if len(data) > 8_000_000:
                raise ValueError('response exceeds8MB capture cap')
            headers = str(response.headers).encode('utf-8')
            row.update(status=response.status, location=response.headers.get('Location'),
                       content_type=response.headers.get('Content-Type'),
                       body_path=str(prefix.with_suffix('.body').relative_to(OWN)),
                       body_bytes=len(data), body_sha256=hashlib.sha256(data).hexdigest(),
                       headers_path=str(prefix.with_suffix('.headers').relative_to(OWN)),
                       headers_sha256=hashlib.sha256(headers).hexdigest())
            prefix.with_suffix('.body').write_bytes(data)
            prefix.with_suffix('.headers').write_bytes(headers)
    except Exception as error:
        row.setdefault('status', None)
        row.update(error_type=type(error).__name__, error=str(error))
    row['finished_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    receipt_path = prefix.with_suffix('.json')
    receipt_path.write_text(json.dumps(row, indent=2) + '\n')
    with ledger.open('a') as stream:
        stream.write(json.dumps(row) + '\n')
    print(json.dumps(row, indent=2))
    if row.get('status') is None or row.get('error_type'):
        raise SystemExit(2)
    if row['status'] in (401, 402, 403, 429):
        raise SystemExit(3)

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('url')
    parser.add_argument('--name', required=True)
    parser.add_argument('--policy', help='owned relative path to captured policy receipt')
    args = parser.parse_args()
    acquire(args.url, args.name, args.policy)
