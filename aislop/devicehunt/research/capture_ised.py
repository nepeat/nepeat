"""Capture public ISED REL company searches and their displayed CSV exports.

Run from the project root. Cookies live only in memory. This research collector
does not import evidence or claim complete coverage of the REL database.
"""
import argparse
import hashlib
import http.cookiejar
import json
import re
import time
import urllib.parse
import urllib.request
from html.parser import HTMLParser
from pathlib import Path

BASE = 'https://sms-sgs.ic.gc.ca'
ACTION = '/equipmentSearch/searchRadioEquipments'


class Form(HTMLParser):
    def __init__(self, html):
        super().__init__()
        self.active = False
        self.values, self.buttons = {}, {}
        self.select = None
        self.download = None
        self.download_csv = None
        self.next_page = None
        self.feed(html)

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if tag == 'form':
            self.active = a.get('action') == ACTION
        if a.get('id') == 'downloadButton' and tag == 'a':
            self.download = a.get('href')
        if a.get('id') == 'downloadCSVButton' and tag == 'a':
            self.download_csv = a.get('href')
        if tag == 'a' and a.get('rel') == 'next':
            self.next_page = a.get('href')
        if not self.active:
            return
        if tag == 'input' and a.get('name'):
            kind = a.get('type', 'text')
            if kind in ('checkbox', 'radio') and 'checked' not in a:
                return
            destination = self.buttons if kind == 'submit' else self.values
            destination[a['name']] = a.get('value', 'on' if kind == 'checkbox' else '')
        if tag == 'select':
            self.select = a.get('name')
        if tag == 'option' and self.select:
            if self.select not in self.values or 'selected' in a:
                self.values[self.select] = a.get('value', '')

    def handle_endtag(self, tag):
        if tag == 'form':
            self.active = False
        if tag == 'select':
            self.select = None


def capture(company, output):
    opener = urllib.request.build_opener(
        urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
    responses = []
    result_pages = []

    def fetch(url, name, values=None):
        if urllib.parse.urlsplit(url).netloc != urllib.parse.urlsplit(BASE).netloc:
            raise ValueError('Unexpected export host')
        data = None if values is None else urllib.parse.urlencode(values).encode()
        with opener.open(urllib.request.Request(url, data=data), timeout=45) as r:
            body = r.read(50_000_001)
            if len(body) > 50_000_000:
                raise ValueError('Response exceeds capture limit')
            entry = dict(url=url, final_url=r.geturl(), method='GET' if data is None else 'POST',
                         content_type=r.headers.get('Content-Type'), path=name,
                         sha256=hashlib.sha256(body).hexdigest())
        (output / name).write_bytes(body)
        responses.append(entry)
        (output / 'capture.json').write_text(json.dumps(dict(
            schema_version=2, company_query=company, complete=False,
            responses=responses, result_pages=result_pages), indent=2) + '\n')
        print('%s: %s (%d bytes)' % (company, name, len(body)), flush=True)
        return body

    output.mkdir(parents=True, exist_ok=False)
    body = fetch(BASE + ACTION + '?lang=en_CA', 'form.html')
    form = Form(body.decode())
    if '_eventId_search' not in form.buttons or 'companyName' not in form.values:
        raise ValueError('Public search form changed')
    values = dict(form.values, companyName=company)
    values['_eventId_search'] = form.buttons['_eventId_search']
    for attempt in range(12):
        body = fetch(BASE + ACTION, 'response-%02d.html' % attempt, values)
        form = Form(body.decode())
        if '_eventId_refresh' not in form.buttons:
            break
        time.sleep(5)
        values = dict(form.values, _eventId_refresh=form.buttons['_eventId_refresh'])
    else:
        raise TimeoutError('ISED did not finish within 12 refreshes')
    # Follow links actually displayed by the result, preserving every page.
    # The importer checks contiguous ranges, table counts and stable totals.
    total_match = re.search(r'id="foundItems"[^>]*>\s*(\d+)\s*</span>', body.decode())
    if total_match:
        total = int(total_match[1])
        result_pages.append(responses[-1]['path'])
        for page in range(1, 501):
            bounds = re.search(r'Displayed Items:\s*from\s+(\d+)\s+to\s+(\d+)', body.decode())
            if total == 0 or (bounds and int(bounds[2]) == total):
                break
            if not bounds or not form.next_page:
                raise ValueError('Missing ISED page range or next-page link')
            body = fetch(urllib.parse.urljoin(BASE, form.next_page), 'page-%03d.html' % page)
            result_pages.append(responses[-1]['path'])
            form = Form(body.decode())
        else:
            raise ValueError('ISED pagination exceeded 500 pages')
    has_export = bool(form.download)
    if has_export:
        body = fetch(urllib.parse.urljoin(BASE, form.download), 'export-00.response')
        for attempt in range(1, 13):
            if b'<html' not in body[:1000].lower():
                break
            form = Form(body.decode())
            if form.download_csv:
                body = fetch(urllib.parse.urljoin(BASE, form.download_csv), 'results.csv')
                if b'<html' in body[:1000].lower():
                    raise ValueError('Prepared CSV download returned HTML')
                break
            if '_eventId_refresh' not in form.buttons:
                raise ValueError('CSV export returned an unexpected HTML page')
            time.sleep(5)
            values = dict(form.values, _eventId_refresh=form.buttons['_eventId_refresh'])
            body = fetch(BASE + ACTION, 'export-%02d.response' % attempt, values)
        else:
            raise TimeoutError('ISED export did not finish within 12 refreshes')
    # No CSV is not an empty result: the importer must examine the last response.
    manifest = dict(schema_version=2, company_query=company, complete=True,
                    responses=responses, result_pages=result_pages)
    (output / 'capture.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps(dict(query=company, export_captured=has_export, output=str(output))), flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('company')
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    capture(args.company, args.output)
