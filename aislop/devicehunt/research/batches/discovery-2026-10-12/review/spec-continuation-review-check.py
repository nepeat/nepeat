"""Independent saved-byte checks for the finite SPEC continuation; never fetch."""
import datetime as dt
import hashlib
from html.parser import HTMLParser
import importlib.util
import json
from pathlib import Path
import re
from urllib import robotparser
from urllib.parse import urlsplit

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
BATCH = HERE.parent / 'spec-continuation'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class Rows(HTMLParser):
    def __init__(self, body):
        super().__init__(convert_charrefs=True)
        self.open_rows, self.rows = [], []
        self.title, self.in_title = '', False
        self.feed(body)

    def handle_starttag(self, tag, attrs):
        if tag == 'tr':
            self.open_rows.append([])
        elif tag in ('td', 'th') and self.open_rows:
            self.open_rows[-1].append('')
        elif tag == 'br' and self.open_rows and self.open_rows[-1]:
            self.open_rows[-1][-1] += ' '
        elif tag == 'title':
            self.in_title = True

    def handle_endtag(self, tag):
        if tag == 'tr' and self.open_rows:
            self.rows.append([' '.join(c.split()) for c in self.open_rows.pop()])
        elif tag == 'title':
            self.in_title = False

    def handle_data(self, data):
        if self.in_title:
            self.title += data
        if self.open_rows and self.open_rows[-1]:
            self.open_rows[-1][-1] += data


def main():
    plan = json.loads((BATCH / 'plan.json').read_text())
    inputs = json.loads((BATCH / 'inputs.json').read_text())
    checks = json.loads((BATCH / 'checks.json').read_text())
    observations = json.loads((BATCH / 'observations.json').read_text())
    requests = [json.loads(line) for line in (BATCH / 'requests.jsonl').read_text().splitlines()]
    assert (BATCH / 'plan.json').read_bytes() == (HERE.parent / 'spec-next-plan/plan.json').read_bytes()
    assert sha(BATCH / 'plan.json') == inputs['plan_sha256']
    pins = {**inputs['dependencies'], **inputs['prior_ledgers_and_plan']}
    for name, expected in pins.items():
        assert sha(ROOT / name) == expected, name
    prior = [json.loads(line) for name in inputs['prior_ledgers_and_plan'] if name.endswith('.jsonl')
             for line in (ROOT / name).read_text().splitlines() if line.strip()]
    assert len(prior) == inputs['prior_requests'] == 604
    assert [r for r in prior if r.get('status') != 200] == inputs['prior_failures']
    assert len(inputs['prior_failures']) == 3 and not any(r.get('status') == 429 for r in prior)
    selected = [s['url'] for s in plan['sources']]
    assert len(set(selected)) == 18 and not set(selected).intersection(r['url'] for r in prior)
    assert len(requests) == 19 and [r['sequence'] for r in requests] == list(range(1, 20))
    assert [r['url'] for r in requests] == ['https://www.spec.org/robots.txt'] + selected
    robots = robotparser.RobotFileParser()
    robots.parse((BATCH / 'robots.txt').read_text().splitlines())
    assert sha(BATCH / 'robots.txt') == checks['fresh_robots_sha256'] == inputs['dependencies']['data/raw/discovery-2026-10-11-spec/wave10/robots/robots.txt']
    utc_gaps = []
    for n, request in enumerate(requests):
        assert request['status'] == 200 and request['curl_returncode'] == 0 and request['curl_stderr'] == ''
        assert not request['automatic_redirects'] and request['retries'] == 0
        assert not request['login_marker_detected'] and request['location_not_followed'] is None
        argv = request['argv']
        assert '-L' not in argv and '--location' not in argv and argv[-1] == request['url']
        assert argv[argv.index('--retry') + 1] == '0'
        assert argv[argv.index('--user-agent') + 1] == request['user_agent']
        body = (BATCH / request['body_path']).read_bytes()
        assert len(body) == request['body_bytes'] and sha(BATCH / request['body_path']) == request['body_sha256']
        assert sha(BATCH / request['header_path']) == request['header_sha256']
        header = (BATCH / request['header_path']).read_text()
        assert re.findall(r'^HTTP/\S+ (\d+)', header, re.M) == ['200']
        assert not re.search(r'^Location:', header, re.M | re.I)
        assert not re.search(rb'<input[^>]+type=["\']?password|<title>[^<]*(?:sign in|log in|login)', body, re.I)
        if n:
            assert request['seconds_after_prior_completion'] >= 10
            gap = (dt.datetime.fromisoformat(request['started_at_utc']) - dt.datetime.fromisoformat(requests[n-1]['finished_at_utc'])).total_seconds()
            utc_gaps.append(gap)
            assert gap >= 10
            assert request['policy']['sha256'] == sha(BATCH / 'robots.txt')
            assert robots.can_fetch(request['user_agent'], request['url']) and robots.crawl_delay(request['user_agent']) == 10
            assert '/cgi-bin/' not in urlsplit(request['url']).path and not urlsplit(request['url']).query
    manifest_entries = []
    for line in (BATCH / 'SHA256SUMS').read_text().splitlines():
        expected, name = line.split('  ', 1)
        assert sha(BATCH / name) == expected, name
        manifest_entries.append(name)
    for key, name in [('requests_sha256','requests.jsonl'),('observations_sha256','observations.json'),
                      ('capture_script_sha256','capture.py'),('observation_script_sha256','observe.py')]:
        assert sha(BATCH / name) == checks[key]
    assert sha(ROOT / observations['jbb_parser_path']) == observations['jbb_parser_sha256']
    spec = importlib.util.spec_from_file_location('review_saved_spec_index', ROOT / 'research/batches/discovery-2026-10-11/spec/scripts/parse_idx.py')
    index_parser = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(index_parser)
    indexes = {name:index_parser.rows(str(ROOT / 'data/raw/discovery-2026-10-11-spec/index' / (name + '_results_' + name + '_.html')))
               for name in ('jbb2015','cpu2017')}
    for source in plan['sources']:
        assert source['saved_index_row'] in indexes[source['benchmark']]
        links = source['saved_index_row']['_links']
        assert source['url'] == 'https://www.spec.org' + links['HTML' if source['benchmark'] == 'jbb2015' else 'Text']
    raw_root = ROOT / 'data/raw/discovery-2026-10-11-spec'
    saved_cpu_files = {p.name for name in ('cpu2017','wave8/cpu2017','wave9/cpu2017') for p in (raw_root/name).glob('*.txt')}
    matched = [r for r in indexes['cpu2017'] if any(v in (r.get('test_sponsor','')+' '+r.get('hw_model','')).casefold() for v in ('inspur','ieit'))]
    fetched_names = {(r['test_sponsor'],r['hw_model']) for r in matched if Path(r['_links']['Text']).name in saved_cpu_files}
    remaining = {}
    for row in sorted(matched,key=lambda r:r['_links']['Text'],reverse=True):
        key = (row['test_sponsor'],row['hw_model'])
        if key not in fetched_names:
            remaining.setdefault(key,row)
    assert len(remaining) == 244
    newest = sorted(remaining.values(),key=lambda r:r['_links']['Text'],reverse=True)[:12]
    assert newest == [s['saved_index_row'] for s in plan['sources'] if s['benchmark'] == 'cpu2017']
    pages = observations['observed_pages']
    assert [p['url'] for p in pages] == selected
    locators = 0
    extra_jbb_header_rows = 0
    summary = []
    for page, source, request in zip(pages, plan['sources'], requests[1:]):
        assert page['saved_index_row'] == source['saved_index_row']
        assert page['capture_sha256'] == request['body_sha256'] and page['captured_at_utc'] == request['finished_at_utc']
        path = BATCH / page['body_path']
        if page['benchmark'] == 'jbb2015':
            parsed = Rows(path.read_text())
            assert parsed.title == page['document_title']
            found = [r[0] for r in parsed.rows if len(r) == 1 and re.fullmatch(r'Hardware hw_\d+', r[0])]
            assert found == ['Hardware hw_1'] == [b['section_as_printed'] for b in page['hardware_blocks']]
            for header in page['header_rows']:
                assert parsed.rows[header['row_index_zero_based']] == header['raw_cells_whitespace_normalized']
                extra_jbb_header_rows += 1
            fields = page['hardware_blocks'][0]['fields']
            for field in fields:
                actual = parsed.rows[field['locator']['table_row_index_zero_based']]
                assert actual == field['raw_cell_row'] == [field['label_as_printed'],field['value_as_printed_cell_text']]
                assert field['literal_None_means_source_omission'] == (actual[1] == 'None')
                assert field['locator']['section'] == 'Hardware hw_1'
                locators += 1
            values = {f['label_as_printed']:f['value_as_printed_cell_text'] for f in fields}
            assert values['Vendor'] == 'Inspur Corporation'
            assert values['Name'] == source['saved_index_row']['product.SUT.hw.system.hw_1.name']
            summary.append(dict(path=page['body_path'], vendor=values['Vendor'], name=values['Name'],
                                form_factor=values.get('Form Factor'), power_quantity_rating=values.get('Power Supply Quantity and Rating (W)'),
                                power_details=values.get('Power Supply Details'), hardware_blocks=found))
        else:
            lines = path.read_text().splitlines()
            assert 'POWER-RELEVANT HARDWARE' not in {line.strip() for line in lines} and not page['power_section_present']
            assert [s['section_as_printed'] for s in page['hardware_sections']] == ['HARDWARE']
            assert lines[1].strip() == source['saved_index_row']['test_sponsor'] + ' ' + source['saved_index_row']['hw_model']
            for header in page['header_lines']:
                assert lines[header['line_1based']-1] == header['raw_line']
                locators += 1
            for section in page['hardware_sections']:
                start = section['start_line_1based']-1
                raw = section['raw_section_lines']
                assert lines[start:start+len(raw)] == raw
                for field in section['fields']:
                    assert [lines[n-1] for n in field['lines_1based']] == field['raw_lines']
                    first = re.match(r'^\s*([^:]+):\s?(.*)$',field['raw_lines'][0])
                    assert first.group(1).strip() == field['label_as_printed']
                    assert [first.group(2)] + field['raw_lines'][1:] == field['value_lines_as_printed']
                    locators += 1
            summary.append(dict(path=page['body_path'],full_system_header=lines[1],hardware_fields=len(page['hardware_sections'][0]['fields']),power_section=False))
    assert locators == checks['evidence_rows_or_fields_verified'] == 354
    result = dict(verdict='ACCEPT discovery only',checked_utc=dt.datetime.now(dt.timezone.utc).isoformat(),
                  hashes={n:sha(BATCH/n) for n in ('handoff.md','checks.json','inputs.json','plan.json','observations.json','requests.jsonl','SHA256SUMS')},
                  HTTP=19,result_bodies=18,statuses={'200':19},minimum_UTC_gap=min(utc_gaps),
                  minimum_recorded_monotonic_gap=min(r['seconds_after_prior_completion'] for r in requests[1:]),
                  historical_requests=len(prior),historical_failures=len(inputs['prior_failures']),
                  dependency_hashes_verified=len(pins),local_manifest_hashes_verified=len(manifest_entries),
                  raw_field_or_CPU_header_locators=locators,additional_jbb_header_rows=extra_jbb_header_rows,
                  total_verified_field_or_header_records=locators+extra_jbb_header_rows,
                  remaining_distinct_CPU_names_before_batch=len(remaining),remaining_after_batch=len(remaining)-12,pages=summary,
                  scope='Saved-byte discovery review only; no candidate/identity/role/routing acceptance, requests, imports or production changes')
    (HERE / 'spec-continuation-review-checks.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items() if k not in ('pages','hashes')},indent=2))


if __name__ == '__main__':
    main()
