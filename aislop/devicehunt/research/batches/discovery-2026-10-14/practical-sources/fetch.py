"""One bounded HTTP exchange; no automatic redirects, retries, crawl or imports."""
import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time
from urllib import robotparser
from urllib.parse import urlsplit, urljoin

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
UA = 'devicehunt-public-research/1.0'
POLICIES = {'sd4100': 'https://www.celestica.com/robots.txt',
            'zs3-2': 'https://docs.oracle.com/robots.txt',
            'zs3-4': 'https://docs.oracle.com/robots.txt'}
GROUPS = {'sd4100': 1, 'zs3-2': 2, 'zs3-4': 3}
def sha(b): return hashlib.sha256(b).hexdigest()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('group', choices=GROUPS)
    parser.add_argument('url')
    parser.add_argument('output')
    parser.add_argument('--policy', action='store_true')
    parser.add_argument('--parent', type=int)
    parser.add_argument('--sandbox-dns-escalation', action='store_true')
    args = parser.parse_args()
    pins = json.loads((HERE/'input-pins.json').read_text())
    plan_bytes = (ROOT/pins['plan']).read_bytes()
    assert sha(plan_bytes) == pins['plan_sha256']
    plan = json.loads(plan_bytes)
    lead = next(x for x in plan['leads'] if x['priority'] == GROUPS[args.group])
    assert sha((ROOT/lead['basis']).read_bytes()) == lead['parent_sha256']
    ledger = HERE/'requests.jsonl'
    records = [json.loads(x) for x in ledger.read_text().splitlines()] if ledger.exists() else []
    reached = [r for r in records if r['status'] != 0]
    assert len(reached) < 8 and not any(r['status'] == 429 for r in records)
    repeats = [r for r in records if r['url'] == args.url]
    if repeats:
        assert args.sandbox_dns_escalation and len(repeats) == 1 and repeats[0]['status'] == 0 and repeats[0]['curl_returncode'] == 6, 'only tool-required escalation after sandbox DNS failure; no remote retry'
    else:
        assert not args.sandbox_dns_escalation
    assert urlsplit(args.url).scheme == 'https'
    policy_url = POLICIES[args.group]
    host = urlsplit(policy_url).netloc
    assert urlsplit(args.url).netloc == host, 'unexpected origin'
    prior_source = [r for r in records if r['group'] == args.group and not r['is_policy']]
    policies = [r for r in reached if r['url'] == policy_url]
    assert len(prior_source) + len(policies) < 3
    assert not any(r['status'] in (0, 402, 403, 404, 429) or r['gate_marker'] or r['curl_returncode'] for r in prior_source)
    policy = None
    delay = 0
    authorization = {'plan_sha256': sha(plan_bytes)}
    if args.policy:
        assert args.url == policy_url and not policies and args.parent is None
        authorization['kind'] = 'fresh policy'
    else:
        assert len(policies) == 1 and policies[0]['status'] == 200 and not policies[0]['curl_returncode'] and not policies[0]['gate_marker'], 'policy unavailable'
        policy_body = (HERE/policies[0]['body_path']).read_bytes()
        assert sha(policy_body) == policies[0]['body_sha256']
        robots = robotparser.RobotFileParser()
        robots.parse(policy_body.decode('utf-8').splitlines())
        assert robots.can_fetch(UA, args.url), 'fresh policy denies exact path'
        delay = robots.crawl_delay(UA) or robots.crawl_delay('*') or 0
        assert delay <= 60, 'checkpoint longer policy delay'
        policy = {'body_path': policies[0]['body_path'], 'sha256': sha(policy_body), 'user_agent': UA, 'exact_URL': args.url, 'allowed': True, 'crawl_delay_seconds': delay}
        if args.url == lead['URL']:
            assert not prior_source and args.parent is None
            authorization.update(kind='exact planned route', basis=lead['basis'], basis_sha256=lead['parent_sha256'], literal_href=lead['raw_href'])
        else:
            assert len(prior_source) == 1 and args.parent == prior_source[0]['sequence']
            parent = prior_source[0]
            assert parent['status'] in (301, 302, 303, 307, 308)
            assert args.url == urljoin(parent['url'], parent['location_not_followed'])
            assert 'known_captured_target_checked' in args.output, 'manual offline novelty/scope review required before redirect'
            authorization.update(kind='literal same-origin redirect', parent_sequence=args.parent, parent_header_sha256=parent['header_sha256'], literal_Location=parent['location_not_followed'])
    previous = [r for r in records if urlsplit(r['url']).netloc == host]
    wait = 0
    if previous:
        elapsed = (dt.datetime.now(dt.timezone.utc)-dt.datetime.fromisoformat(previous[-1]['finished_at_utc'])).total_seconds()
        wait = max(0, delay-elapsed)
    if wait: time.sleep(wait)
    output = HERE/args.output
    headers = HERE/(args.output+'.headers')
    assert output.parent == HERE and not output.exists() and not headers.exists(), 'immutable new output only'
    argv = ['curl', '--silent', '--show-error', '--max-time', '45', '--connect-timeout', '15', '--retry', '0', '--max-redirs', '0', '--user-agent', UA, '--dump-header', str(headers), '--output', str(output), '--write-out', '%{http_code}', args.url]
    started = dt.datetime.now(dt.timezone.utc).isoformat()
    result = subprocess.run(argv, capture_output=True, text=True)
    finished = dt.datetime.now(dt.timezone.utc).isoformat()
    status = int(result.stdout) if re.fullmatch(r'\d{3}', result.stdout) else 0
    body = output.read_bytes() if output.exists() else b''
    header = headers.read_bytes() if headers.exists() else b''
    location = next((s.split(':', 1)[1].strip() for s in header.decode('utf-8', errors='replace').splitlines() if s.lower().startswith('location:')), None)
    gate = bool(re.search(rb'<input[^>]+type=["\x27]?password|<title>[^<]*(?:sign in|log in|login|register|registration)', body, re.I))
    record = dict(sequence=len(records)+1, group=args.group, is_policy=args.policy, method='GET', url=args.url, started_at_utc=started, finished_at_utc=finished, status=status, user_agent=UA, argv=argv, curl_returncode=result.returncode, curl_stderr=result.stderr, automatic_redirects=False, retries=0, location_not_followed=location, gate_marker=gate, body_path=output.name if output.exists() else None, body_bytes=len(body), body_sha256=sha(body) if output.exists() else None, header_path=headers.name if headers.exists() else None, header_sha256=sha(header) if headers.exists() else None, policy=policy, authorization=authorization, seconds_waited_for_policy=wait)
    with ledger.open('a') as f: f.write(json.dumps(record)+'\n')
    print(json.dumps(record, indent=2))

if __name__ == '__main__': main()
