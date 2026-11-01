"""Bounded vendor-index capture and offline link discovery; never imports devices.

Capture is one explicit URL at a time, no redirects/retries/recursive crawling.
Every attempted exchange consumes the 12-request cap. Parse uses saved bytes only.
"""
from pathlib import Path
from html.parser import HTMLParser
from urllib.parse import urljoin, urlsplit
from urllib.request import Request, build_opener, HTTPRedirectHandler
from urllib.error import HTTPError
from urllib.robotparser import RobotFileParser
import argparse
import datetime
import hashlib
import json
import re
import sqlite3
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
UA = 'devicehunt-public-research/1.0'
MAX_REQUESTS = 12
ALLOWED_HOSTS = {'www.celestica.com', 'www.edge-core.com', 'docs.oracle.com', 'www.accton.com'}

def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for b in iter(lambda: f.read(1 << 20), b''):
            h.update(b)
    return h.hexdigest()

def dump_new(path, value):
    with path.open('x') as f:
        json.dump(value, f, indent=2, ensure_ascii=False)
        f.write('\n')

class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None

def capture(args):
    parsed = urlsplit(args.url)
    assert parsed.scheme == 'https' and parsed.netloc in ALLOWED_HOSTS
    assert not parsed.username and not parsed.password and not parsed.fragment
    ledger = HERE / 'requests.jsonl'
    previous = [json.loads(line) for line in ledger.read_text().splitlines()] if ledger.exists() else []
    for n, reserved in enumerate(previous):
        receipt = HERE / (reserved['path'] + '.receipt.json')
        assert receipt.exists(), 'Earlier request interrupted; inspect checkpoint before any more transport'
        previous[n] = json.loads(receipt.read_text())
    assert len(previous) < MAX_REQUESTS
    assert not any(r.get('status') == 429 for r in previous), 'Batch stopped on throttle'
    assert not any(urlsplit(r['url']).netloc == parsed.netloc and r.get('status') in (402, 403)
                   for r in previous), 'Source stopped on denial'
    recovery = None
    recovery_errors = set()
    if args.environment_recovery_plan:
        plan_path = Path(args.environment_recovery_plan).resolve()
        assert plan_path.parent == HERE
        recovery = json.loads(plan_path.read_text())
        failed = recovery['failed_receipt']
        failed_path = ROOT / failed['path']
        assert sha(failed_path) == failed['sha256']
        failed_entry = json.loads(failed_path.read_text())
        assert failed_entry['state'] == 'error' and 'status' not in failed_entry
        assert 'Temporary failure in name resolution' in failed_entry['error']
        assert args.url in recovery['allowed_urls']
        assert sum(bool(r.get('environment_recovery_plan')) for r in previous) < recovery['maximum_additional_attempts']
        assert not any(r.get('environment_recovery_plan') and (r.get('status') != 200 or r.get('state') != 'completed') for r in previous)
        assert sha(ROOT / recovery['source_capture']['path']) == recovery['source_capture']['sha256']
        assert args.url in (ROOT / recovery['source_capture']['path']).read_text()
        recovery_errors.add(failed_entry['sequence'])
        recovery = {'path': str(plan_path.relative_to(ROOT)), 'sha256': sha(plan_path), 'prior_dns_failure_sequence': failed_entry['sequence']}
    assert not any(urlsplit(r['url']).netloc == parsed.netloc and r.get('state') == 'error' and r['sequence'] not in recovery_errors
                   for r in previous), 'Source stopped on failed exchange'
    assert not any(r['url'] == args.url and r['sequence'] not in recovery_errors for r in previous), 'No repeat requests'
    dest = HERE / args.name
    assert dest.parent == HERE and not dest.exists()
    policy = None
    if parsed.path != '/robots.txt':
        assert args.robots and args.robots_url
        p = Path(args.robots).resolve()
        assert p.is_relative_to(ROOT) and p.is_file()
        assert urlsplit(args.robots_url).netloc == parsed.netloc
        robot = RobotFileParser(args.robots_url)
        robot.parse(p.read_text().splitlines())
        assert robot.can_fetch(UA, args.url), 'Robots forbids exact URL'
        delay = robot.crawl_delay(UA)
        assert delay in (None, 0), 'Delay requires explicit paced capture plan'
        policy = {'path': str(p.relative_to(ROOT)), 'sha256': sha(p),
                  'url': args.robots_url, 'agent': UA, 'allowed': True, 'crawl_delay': delay}
    entry = {'sequence': len(previous) + 1, 'url': args.url, 'method': 'GET', 'user_agent': UA,
             'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'policy': policy,
             'path': args.name, 'redirect_followed': False}
    if recovery:
        entry['environment_recovery_plan'] = recovery
    # Reserve the attempt before transport, so interruption cannot reset budget.
    with ledger.open('a') as f:
        f.write(json.dumps(dict(entry, state='reserved')) + '\n')
    try:
        opener = build_opener(NoRedirect)
        try:
            response = opener.open(Request(args.url, headers={'User-Agent': UA}), timeout=40)
        except HTTPError as exc:
            response = exc
        with response:
            entry['status'] = response.code
            entry['headers'] = {k: v for k, v in response.headers.items() if k.lower() not in ('set-cookie', 'cookie')}
            body = response.read(8_000_001)
            assert len(body) <= 8_000_000, 'Response exceeds bound'
        with dest.open('xb') as f:
            f.write(body)
        entry.update(bytes=len(body), sha256=sha(dest), state='completed')
    except Exception as exc:
        entry.update(state='error', error=type(exc).__name__ + ': ' + str(exc))
        raise
    finally:
        entry['finished_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        # One unique final receipt beside immutable body; reserved ledger stays immutable.
        dump_new(HERE / (args.name + '.receipt.json'), entry)
    print(json.dumps(entry))

def parse(args):
    # One implementation for all offline parsing; the earlier prototype is retired.
    from offline_parser import build
    config = json.loads((HERE / args.config).read_text())
    result = build(config)
    dump_new(HERE / args.output, result)
    print(json.dumps(result['summary'], indent=2))

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    cap = sub.add_parser('capture')
    cap.add_argument('--url', required=True)
    cap.add_argument('--name', required=True)
    cap.add_argument('--robots')
    cap.add_argument('--robots-url')
    cap.add_argument('--environment-recovery-plan', help='Explicit hash-pinned plan for a prior environment-only DNS failure')
    cap.set_defaults(run=capture)
    off = sub.add_parser('parse')
    off.add_argument('--config', default='inputs-ready.json')
    off.add_argument('--output', default='discovery.json')
    off.set_defaults(run=parse)
    args = parser.parse_args()
    args.run(args)
