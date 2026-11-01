"""Offline independent wave5 provenance and locator checks; no imports into databases."""
import collections
from contextlib import ExitStack
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import sys
from unittest.mock import patch
from urllib import robotparser
from urllib.parse import urljoin, urlsplit

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[4]
HERE = Path(__file__).resolve().parent
SOURCE = HERE.parent / 'original-vendor-wave5'
sys.path.insert(0, str(ROOT))
from devicehunt.documents import dependency, read_capture, read_pages


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def clean(value):
    return re.sub(r'\s+', ' ', value).strip()


class ArticleBlocks(HTMLParser):
    def __init__(self, text):
        super().__init__(convert_charrefs=True)
        self.inside = False
        self.stack = []
        self.blocks = []
        self.article_text = []
        self.links = []
        self.feed(text)

    def handle_starttag(self, tag, attrs):
        if tag == 'a' and dict(attrs).get('href'):
            self.links.append(dict(attrs)['href'])
        if tag == 'article':
            self.inside = True
        if self.inside:
            self.stack.append([tag, []])

    def handle_data(self, text):
        if self.inside:
            self.article_text.append(text)
            for item in self.stack:
                item[1].append(text)

    def handle_endtag(self, tag):
        if self.inside:
            indices = [i for i, item in enumerate(self.stack) if item[0] == tag]
            if indices:
                index = indices[-1]
                item = self.stack[index]
                if tag in ('h2', 'p', 'tr', 'li'):
                    self.blocks.append((tag, clean(''.join(item[1]))))
                del self.stack[index:]
        if tag == 'article':
            self.inside = False


def main():
    assert (SOURCE / 'handoff.md').exists() and (SOURCE / 'artifact-manifest.json').exists(), 'wait for author freeze'
    before = {p.name: sha(p) for p in SOURCE.iterdir() if p.is_file()}
    report = {}
    with ExitStack() as guards:
        for name in ('socket.socket.connect', 'socket.create_connection', 'subprocess.run', 'subprocess.Popen'):
            guards.enter_context(patch(name, side_effect=AssertionError('offline reviewer')))
        plan_path = SOURCE.parent / 'next-leads-r2/plan.json'
        plan = json.loads(plan_path.read_text())
        ledger = [json.loads(line) for line in (SOURCE / 'requests.jsonl').read_text().splitlines()]
        assert len(ledger) == 9 <= plan['budget']['HTTP_all_sources_total_cap'] == 12
        assert [r['sequence'] for r in ledger] == list(range(1, 10))
        assert len({r['url'] for r in ledger}) == 9
        by_sequence = {r['sequence']: r for r in ledger}
        for request in ledger:
            assert request['status'] in (200, 302) and request['curl_returncode'] == 0
            assert not request['automatic_redirects'] and request['retries'] == 0 and not request['login_marker']
            assert '--retry' in request['argv'] and request['argv'][request['argv'].index('--retry') + 1] == '0'
            assert '-L' not in request['argv'] and '--location' not in request['argv']
            assert request['argv'][-1] == request['url']
            body = SOURCE / request['body_path']
            headers = SOURCE / request['header_path']
            assert sha(body) == request['body_sha256'] and body.stat().st_size == request['body_bytes']
            assert sha(headers) == request['header_sha256']
            statuses = re.findall(r'^HTTP/\S+\s+(\d+)', headers.read_text(), re.M)
            assert statuses == [str(request['status'])]
            locations = re.findall(r'^location:\s*(.+)$', headers.read_text(), re.I | re.M)
            assert (locations[0].strip() if locations else None) == request['location_not_followed']
            if request['is_policy']:
                assert request['authorization']['plan_sha256'] == sha(plan_path)
                continue
            policy = request['policy']
            assert policy['sha256'] == sha(SOURCE / policy['path']) and policy['exact_URL'] == request['url']
            robots = robotparser.RobotFileParser()
            robots.parse((SOURCE / policy['path']).read_text().splitlines())
            assert robots.can_fetch(request['user_agent'], request['url']) and policy['exact_path_allowed']
            assert (robots.crawl_delay(request['user_agent']) or robots.crawl_delay('*') or 0) == policy['crawl_delay_seconds'] == 0
            auth = request['authorization']
            if auth['kind'] == 'planned canonical landing/page':
                assert request['url'] in {lead['first_fetch'] for lead in plan['leads']}
                assert sha(ROOT / auth['basis']) == auth['basis_sha256']
            else:
                parent = by_sequence[auth['parent_sequence']]
                assert parent['sequence'] < request['sequence'] and parent['group'] == request['group']
                assert urlsplit(parent['url']).netloc == urlsplit(request['url']).netloc
                if auth['kind'] == 'fresh published download':
                    links = ArticleBlocks((SOURCE / parent['body_path']).read_text()).links
                    assert auth['exact_href'] in links and urljoin(parent['url'], auth['exact_href']) == request['url']
                    assert sha(SOURCE / parent['body_path']) == auth['parent_body_sha256']
                else:
                    assert auth['kind'] == 'manual saved redirect' and parent['status'] == 302
                    assert urljoin(parent['url'], parent['location_not_followed']) == request['url']
                    assert sha(SOURCE / parent['header_path']) == auth['parent_header_sha256']
        findings = json.loads((SOURCE / 'findings.json').read_text())
        pages_by_pdf = {}
        metadata = {}
        pdfinfo = json.loads((HERE / 'original-vendor-wave5-pdfinfo-review.json').read_text())
        for pdf, sidecar, count in (('as6700-quick-start.pdf', 'as6700-pages.json', 2), ('as5710-as5712-ds-r09.pdf', 'as5712-pages.json', 4)):
            expected = findings['sources'][pdf]['sha256']
            pdfbytes = dependency(SOURCE, {'path': pdf, 'sha256': expected})
            sidecarbytes = dependency(SOURCE, {'path': sidecar, 'sha256': sha(SOURCE / sidecar)})
            pages, meta = read_capture(sidecarbytes, expected)
            assert read_pages(sidecarbytes, expected) == pages
            independent = next(item for item in pdfinfo['outputs'] if item['pdf'] == pdf)
            assert independent['pdf_sha256'] == expected and independent['page_count'] == count
            assert len(pages) == count
            assert meta['extraction_method'] == 'Poppler pdftotext -layout embedded text; no OCR'
            parsed = json.loads(sidecarbytes)
            page_commands = [c for c in parsed['provenance']['commands'] if '-f' in c['argv']]
            assert len(page_commands) == count
            for page, command in zip(pages, page_commands, strict=True):
                argv = command['argv']
                assert argv[argv.index('-f') + 1] == argv[argv.index('-l') + 1] == str(page['page'])
                assert argv[argv.index('-remove-hyphens') + 1] == 'none' and '-layout' in argv
                assert hashlib.sha256(page['text'].encode()).hexdigest() == command['stdout_sha256']
            pages_by_pdf[pdf] = {p['page']: p['text'].splitlines() for p in pages}
            metadata[pdf] = meta
        article = ArticleBlocks((SOURCE / 'oracle7420-specifications.html').read_text())
        article_text = clean(' '.join(article.article_text))
        pdf_obs = html_obs = 0
        for item in findings['observations']:
            if item['source'].endswith('.pdf'):
                actual = pages_by_pdf[item['source']][item['pdf_page']]
                assert [actual[n - 1] for n in item['lines_1based']] == item['raw_lines']
                assert item['visually_reviewed'] and (SOURCE / item['visual_page']).exists()
                pdf_obs += 1
            elif 'raw_block' in item:
                tag, text = article.blocks[item['locator']['article_block_ordinal_zero_based']]
                assert tag == item['locator']['tag'] and text == item['raw_block']
                html_obs += 1
            else:
                assert item['raw_text'] in article_text
                html_obs += 1
        assert pdf_obs == 26 and html_obs == 10
        for row in findings['ordering_rows']:
            assert pages_by_pdf[row['source']][row['pdf_page']][row['line_1based'] - 1] == row['raw_line']
            assert re.split(r'\s{2,}', row['raw_line'].strip()) == row['raw_cells_from_embedded_text']
            assert row['visually_reviewed'] and 'no aliases' in row['scope']
        assert len(findings['ordering_rows']) == 38
        disputed = [r for r in findings['ordering_rows'] if r.get('model_text_render_conflict')]
        assert len(disputed) == 2
        for row in disputed:
            assert row['model_as_embedded_text'] == 'F' + row['model_visible_in_render']
            assert 'unresolved' in row['conflict_note']
        render = json.loads((SOURCE / 'render-provenance.json').read_text())
        distinct = set()
        count = 0
        for run in render['renders']:
            assert sha(SOURCE / run['source']) == run['pdf_sha256'] and run['returncode'] == 0
            for page, image in zip(run['pages'], run['images'], strict=True):
                assert sha(SOURCE / image['path']) == image['sha256']
                distinct.add((run['source'], page))
                count += 1
        assert len(distinct) == 6 and count == render['local_rendered_pages'] == 7 <= render['local_render_cap'] == 8
        report.update(actual_HTTP=9, statuses=dict(collections.Counter(str(r['status']) for r in ledger)),
                      total_HTTP_cap=12, policies=2, manually_counted_redirects=2, planned_sources=3,
                      complete_PDF_pages=6, distinct_visual_pages=6, render_instances=7,
                      PDF_observations_checked=pdf_obs, HTML_observations_checked=html_obs, ordering_rows_checked=38,
                      unresolved_fan_text_render_rows=2, honest_extraction_metadata=metadata)
        for name, expected in plan['prior_input_hashes'].items():
            assert sha(ROOT / name) == expected
        assert before == {p.name: sha(p) for p in SOURCE.iterdir() if p.is_file()}
        report.update(PASS=True, source_artifact_hashes=before, plan_sha256=sha(plan_path),
                      source_files_unchanged=True, network_calls=0, imports=0, database_opens=0,
                      scope='Read-only discovery review. Source content and all seven render images manually inspected; no extraction/re-render/partial collection/import.')
    with (HERE / 'original-vendor-wave5-review-checks.json').open('x') as handle:
        json.dump(report, handle, indent=2)
        handle.write('\n')
    print('PASS:9HTTP/6PDFpages/7renderinstances/36observations/38literal orderingrows; source files unchanged')


if __name__ == '__main__':
    main()
