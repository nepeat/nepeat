"""Finite SPEC continuation: saved eighteen links, fresh robots, no redirects/retries."""
import datetime
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time
from urllib import robotparser
from urllib.parse import urlsplit

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
PLAN = HERE.parent / 'spec-next-plan/plan.json'
RAW = ROOT / 'data/raw/discovery-2026-10-11-spec'
UA = 'devicehunt-research/1.0 (non-commercial hardware identification research; low-rate)'
CAP = 20
DELAY = 10
LOG = HERE / 'requests.jsonl'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def create_json(path, data):
    with path.open('x') as handle:
        json.dump(data, handle, ensure_ascii=False, indent=2)
        handle.write('\n')


def historical():
    ledgers = sorted(RAW.rglob('*requests*.jsonl'))
    rows = [json.loads(line) for p in ledgers for line in p.read_text().splitlines() if line.strip()]
    return ledgers, rows


def main():
    assert not LOG.exists(), 'new run only; no resume/retry or overwrite'
    plan = json.loads(PLAN.read_text())
    assert len(plan['sources']) == 18 and len({s['url'] for s in plan['sources']}) == 18
    for name, expected in plan['dependencies'].items():
        assert sha(ROOT / name) == expected
    ledgers, previous = historical()
    assert not any(r.get('status') == 429 for r in previous), 'historical throttle needs explicit resolution'
    selected = {s['url'] for s in plan['sources']}
    assert not any(r.get('url') in selected for r in previous), 'only previously unrequested exact links authorized'
    pins = {str(p.relative_to(ROOT)): sha(p) for p in ledgers + [PLAN, PLAN.with_suffix('.py')]}
    create_json(HERE / 'inputs.json', {'observed_utc': utc(), 'plan_sha256': sha(PLAN),
                'dependencies': plan['dependencies'], 'prior_ledgers_and_plan': pins,
                'prior_requests': len(previous), 'prior_failures': [r for r in previous if r.get('status') != 200],
                'selected_prior_exact_url_overlap': [], 'selected_sources': plan['sources'],
                'limits': {'actual_HTTP_including_robots_redirects': CAP, 'result_pages': 18,
                           'minimum_seconds_after_prior_completion': DELAY, 'searches': 0, 'screenshots': 0}})
    with (HERE / 'plan.json').open('xb') as handle:
        handle.write(PLAN.read_bytes())
    last_finished = None
    actual = 0
    stopped = False
    outcomes = []
    robots = None
    policy_path = HERE / 'robots.txt'

    def fetch(url, name, purpose, policy=None):
        nonlocal last_finished, actual, stopped
        assert actual < CAP
        assert urlsplit(url).scheme == 'https' and urlsplit(url).netloc == 'www.spec.org'
        assert '/cgi-bin/' not in urlsplit(url).path and not urlsplit(url).query
        assert not any(r.get('url') == url and r.get('status') in (402, 403, 404, 429, 0) for r in historical()[1])
        wait = max(0, DELAY - (time.monotonic() - last_finished)) if last_finished is not None else 0
        if wait:
            time.sleep(wait)
        path = HERE / name
        assert not path.exists()
        path.parent.mkdir(exist_ok=True)
        headers = HERE / (name + '.headers')
        assert not headers.exists()
        started = utc()
        gap = time.monotonic() - last_finished if last_finished is not None else None
        actual += 1
        argv = ['curl', '--silent', '--show-error', '--max-time', '45', '--retry', '0',
                '--user-agent', UA, '--dump-header', str(headers), '--output', str(path),
                '--write-out', '%{http_code}', url]
        result = subprocess.run(argv, capture_output=True, text=True)
        last_finished = time.monotonic()
        finished = utc()
        status = int(result.stdout[-3:]) if re.fullmatch(r'\d{3}', result.stdout) else 0
        body = path.read_bytes() if path.exists() else b''
        login = bool(re.search(rb'<input[^>]+type=["\']?password|<title>[^<]*(?:sign in|log in|login)', body, re.I))
        location = next((line.split(':', 1)[1].strip() for line in headers.read_text(errors='replace').splitlines()
                         if line.lower().startswith('location:')), None) if headers.exists() else None
        entry = {'sequence': actual, 'url': url, 'method': 'GET', 'purpose': purpose,
                 'started_at_utc': started, 'finished_at_utc': finished,
                 'seconds_after_prior_completion': gap, 'minimum_delay_seconds': DELAY,
                 'automatic_redirects': False, 'retries': 0, 'user_agent': UA,
                 'argv': argv, 'status': status, 'curl_returncode': result.returncode,
                 'curl_stderr': result.stderr, 'location_not_followed': location,
                 'body_path': name if path.exists() else None, 'body_sha256': sha(path) if path.exists() else None,
                 'body_bytes': len(body), 'header_path': str(headers.relative_to(HERE)) if headers.exists() else None,
                 'header_sha256': sha(headers) if headers.exists() else None,
                 'login_marker_detected': login, 'prior_exact_url_failure_check': 'clear',
                 'policy': policy}
        with LOG.open('a') as handle:
            handle.write(json.dumps(entry, ensure_ascii=False) + '\n')
        stopped = status == 429
        print(json.dumps({'sequence': actual, 'status': status, 'url': url, 'bytes': len(body)}), flush=True)
        return entry

    entry = fetch('https://www.spec.org/robots.txt', 'robots.txt', 'fresh policy')
    if entry['status'] == 200 and entry['curl_returncode'] == 0 and not entry['login_marker_detected']:
        robots = robotparser.RobotFileParser()
        robots.parse(policy_path.read_text(errors='replace').splitlines())
        DELAY_ACTUAL = robots.crawl_delay(UA) or robots.crawl_delay('*') or 0
        assert DELAY_ACTUAL <= DELAY, 'fresh delay exceeds authorized conservative delay; checkpoint instead'
    else:
        stopped = True
    for selected_source in plan['sources']:
        url = selected_source['url']
        if stopped:
            outcomes.append({'url': url, 'outcome': 'not requested; run stopped after policy failure or429'})
            continue
        allowed = robots.can_fetch(UA, url)
        policy = {'path': 'robots.txt', 'sha256': sha(policy_path), 'exact_path_allowed': allowed,
                  'crawl_delay_seconds': robots.crawl_delay(UA) or robots.crawl_delay('*'), 'user_agent': UA}
        if not allowed:
            outcomes.append({'url': url, 'outcome': 'not requested; fresh robots denial', 'policy': policy})
            continue
        kind = selected_source['benchmark']
        filename = Path(urlsplit(url).path).name
        e = fetch(url, kind + '/' + filename, 'planned published full-disclosure result', policy)
        outcome = 'captured' if e['status'] == 200 and e['curl_returncode'] == 0 and not e['login_marker_detected'] else 'stopped source; no retry or alternate route'
        outcomes.append({'url': url, 'benchmark': kind, 'outcome': outcome,
                         'request_sequence': e['sequence'], 'status': e['status'], 'body_path': e['body_path']})
    assert all(sha(ROOT / name) == expected for name, expected in pins.items())
    create_json(HERE / 'capture-outcomes.json', {'finished_utc': utc(), 'actual_HTTP': actual, 'cap': CAP,
                'result_sources_planned': 18, 'outcomes': outcomes, 'prior_inputs_unchanged': True,
                'redirects_followed': 0, 'searches': 0, 'screenshots': 0,
                'whole_run_stopped_on429': any(o.get('status') == 429 for o in outcomes)})


if __name__ == '__main__':
    main()
