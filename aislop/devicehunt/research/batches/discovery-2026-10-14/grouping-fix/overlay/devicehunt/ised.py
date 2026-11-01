"""Import original and paginated ISED REL company searches with CSV evidence.

The public CSV has no header. Only independently labelled columns are
interpreted. Paginated captures preserve HTML identities when CSV differs.
"""
import csv
import io
import json
import re
from collections import Counter
from datetime import datetime
from pathlib import Path
from urllib.parse import urlsplit

from .catalog import digest, normalize, target_for
from .importers import Page
from .oracle_evidence import plain

URL = 'https://sms-sgs.ic.gc.ca/equipmentSearch/searchRadioEquipments?lang=en_CA'
HEADERS = ['Hardware Version Identification Number (HVIN)', 'Product Marketing Name (PMN)',
           'Host Marketing Name (HMN)', 'Firmware Version Identification Number (FVIN)',
           'Certification Number', 'Approval Date']


def store_export(c, sid, rows, matched_numbers):
    """Keep every checked CSV row, including unmatched and non-target companies."""
    c.db.execute('DELETE FROM ised_equipment WHERE source_id=?', (sid,))
    for n, row in enumerate(rows, 1):
        c.db.execute('INSERT INTO ised_equipment VALUES (' + ','.join('?' for _ in range(13)) + ')',
                     (sid, n, *row[:6], row[7], row[8], int(n in matched_numbers),
                      json.dumps(row, ensure_ascii=False), URL))


def html_rows(body):
    table = re.search(r'<table\b[^>]*\bid="equipmentSearchResultTable"[^>]*>(.*?)</table>', body, re.S)
    count = re.search(r'<span\b[^>]*\bid="foundItems"[^>]*>\s*(\d+)\s*</span>', body)
    if not table or not count:
        raise ValueError('Missing ISED result table or total; processing/error pages are not empty results')
    companies = [plain(s) for s in re.findall(r'<th\b[^>]*\bcolspan\s*=\s*["\']?7["\']?[^>]*>(.*?)</th>', table[1], re.S)]
    if len(companies) != 1 or not re.fullmatch(r'\d+ - .+', companies[0]):
        raise ValueError('Importer requires one explicit company group; multi-company queries need a separate parser')
    number, company = companies[0].split(' - ', 1)
    rows = Page(table[0]).rows
    if not rows or rows.pop(0) != HEADERS or any(len(r) != 6 for r in rows):
        raise ValueError('ISED labelled result columns changed')
    if not rows or len(rows) != int(count[1]):
        raise ValueError('Incomplete ISED HTML results: pagination must be captured before importing')
    return number, company, rows


def validate_capture(path):
    path = Path(path)
    data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('complete') is not True
            or not isinstance(data.get('company_query'), str)
            or not target_for(data['company_query']) or not isinstance(data.get('responses'), list)):
        raise ValueError('Invalid or unfinished ISED capture')
    pages, exports, names = [], [], set()
    for entry in data['responses']:
        name = entry.get('path')
        if not isinstance(name, str) or Path(name).name != name or name in names:
            raise ValueError('Invalid ISED capture dependency')
        names.add(name)
        file = (path.parent / name).resolve()
        if file.parent != path.parent.resolve():
            raise ValueError('ISED dependency escapes capture directory')
        for key in ('url', 'final_url'):
            u = urlsplit(entry.get(key, ''))
            if u.scheme != 'https' or u.netloc != 'sms-sgs.ic.gc.ca' or u.path != urlsplit(URL).path:
                raise ValueError('Unexpected ISED source endpoint')
        if entry.get('method') not in ('GET', 'POST'):
            raise ValueError('Unexpected ISED request method')
        body = file.read_bytes()
        if digest(body) != entry.get('sha256'):
            raise ValueError('ISED capture checksum mismatch')
        text = body.decode('utf-8-sig')
        if '<html' in text[:1000].lower():
            if 'id="equipmentSearchResultTable"' in text:
                pages.append(html_rows(text))
        else:
            rows = list(csv.reader(io.StringIO(text), strict=True))
            if not rows or any(len(r) != 44 for r in rows):
                raise ValueError('ISED headerless CSV layout changed')
            exports.append(rows)
    if not pages or len(exports) != 1 or any(p != pages[0] for p in pages):
        raise ValueError('Missing or inconsistent ISED search/export evidence')
    number, company, labelled = pages[0]
    if target_for(company) != target_for(data['company_query']):
        raise ValueError('ISED result company does not match the target query')
    rows, matched = exports[0], []
    for r in rows:
        if r[0] != number or r[1] != company or not r[2] or not r[7].startswith(number + '-'):
            raise ValueError('ISED CSV company or certification identity mismatch')
        try:
            date = datetime.fromisoformat(r[8]).date().isoformat()
        except ValueError as exc:
            raise ValueError('Invalid ISED source approval timestamp') from exc
        matched.append((r[2], r[3], r[4], r[5], r[7], date))
    if Counter(matched) != Counter(map(tuple, labelled)):
        raise ValueError('ISED CSV and labelled HTML identities differ')
    return number, company, rows


def import_capture(c, path):
    if json.loads(Path(path).read_text()).get('schema_version') == 2:
        return import_paged_capture(c, path)
    number, company, rows = validate_capture(path)
    sid = 'ised-rel-' + number
    c.source(sid, URL, 'regulatory', 'Public factual REL fields; ISED source rights retained',
             'One complete single-company public REL query, with HTML total and labelled fields checked against CSV. '
             'Not all Canadian equipment or the full REL database. Company is a regulatory listing association, '
             'not proof of factory, OEM or hyperscaler deployment. Headerless CSV fields outside the labelled '
             'identity columns remain uninterpreted. All 44-column CSV rows also remain in ised_equipment. '
             'Approval dates are source assertions; 1994-01-01 can be a historical placeholder.', path)
    store_export(c, sid, rows, set(range(1, len(rows) + 1)))
    for n, row in enumerate(rows, 1):
        ids = [('ic_id', row[7])]
        # PMN and host names can describe families or different equipment: facts,
        # not aliases that would satisfy exact-model inventory validation.
        facts = [('ised_company_number', number), ('ised_hvin', row[2]),
                 ('ised_approval_timestamp_raw', row[8])]
        facts.extend((key, row[i]) for i, key in [(3, 'ised_product_marketing_name'),
                     (4, 'ised_host_marketing_name'), (5, 'ised_firmware_version')] if row[i])
        c.add(sid, str(n), URL, target_for(company), row[2], row[2], 'radio equipment',
              {'csv_row_number': n, 'csv_cells': row, 'company_number': number,
               'company_name': company, 'html_column_order': HEADERS},
              [(company, 'regulatory_company')], ids, facts)
    c.finish(sid, len(rows))
    return {'query_records': len(rows), 'imported': len(rows)}


def labelled_page(body):
    """Return a page's global range and company-scoped, labelled identity rows.

    ISED groups by company NAME. The number in a group heading need not be the
    company number for every row (Oracle has two numbers under one heading).
    """
    count = re.search(r'<span\b[^>]*\bid="foundItems"[^>]*>\s*(\d+)\s*</span>', body)
    if not count or 'Radio Equipment List (REL) - Search Results' not in plain(body):
        raise ValueError('Not a completed ISED result page')
    total = int(count[1])
    table = re.search(r'<table\b[^>]*\bid="equipmentSearchResultTable"[^>]*>(.*?)</table>', body, re.S)
    if total == 0:
        if table and Page(table[0]).rows:
            raise ValueError('ISED zero total conflicts with table rows')
        return {'total': 0, 'start': 0, 'end': 0, 'rows': []}
    bounds = re.search(r'Displayed Items:\s*from\s+(\d+)\s+to\s+(\d+)', body)
    if not table or not bounds or not 1 <= int(bounds[1]) <= int(bounds[2]) <= total:
        raise ValueError('Missing or invalid ISED displayed range')
    chunks = re.split(r'<th\b[^>]*\bcolspan\s*=\s*["\']?7["\']?[^>]*>(.*?)</th>', table[1], flags=re.S)
    rows, header_seen = [], False
    if len(chunks) < 3 or Page(chunks[0]).rows:
        raise ValueError('ISED rows precede a company group')
    for i in range(1, len(chunks), 2):
        label = plain(chunks[i])
        if not re.fullmatch(r'[A-Za-z0-9]+ - .+', label):
            raise ValueError('ISED company heading changed')
        number, company = label.split(' - ', 1)
        for row in Page(chunks[i + 1]).rows:
            if row == HEADERS:
                header_seen = True
                continue
            if not header_seen or len(row) != 6 or not row[0]:
                raise ValueError('ISED labelled identity fields changed')
            rows.append({'group_number': number, 'company': company, 'cells': row})
    start, end = map(int, bounds.groups())
    if len(rows) != end - start + 1:
        raise ValueError('ISED displayed range does not match its row count')
    return {'total': total, 'start': start, 'end': end, 'rows': rows}


def validate_paged_capture(path):
    path = Path(path)
    data = json.loads(path.read_text())
    if (data.get('schema_version') != 2 or data.get('complete') is not True
            or not isinstance(data.get('company_query'), str) or not target_for(data['company_query'])
            or not isinstance(data.get('responses'), list) or not isinstance(data.get('result_pages'), list)
            or not data['result_pages'] or len(set(data['result_pages'])) != len(data['result_pages'])):
        raise ValueError('Invalid or unfinished paginated ISED capture')
    pages, exports, names = {}, [], set()
    for entry in data['responses']:
        name = entry.get('path')
        if not isinstance(name, str) or Path(name).name != name or name in names:
            raise ValueError('Invalid ISED capture dependency')
        names.add(name)
        file = (path.parent / name).resolve()
        if file.parent != path.parent.resolve():
            raise ValueError('ISED dependency escapes capture directory')
        for field in ('url', 'final_url'):
            u = urlsplit(entry.get(field, ''))
            if u.scheme != 'https' or u.netloc != 'sms-sgs.ic.gc.ca' or u.path != urlsplit(URL).path:
                raise ValueError('Unexpected ISED source endpoint')
        if entry.get('method') not in ('GET', 'POST'):
            raise ValueError('Unexpected ISED request method')
        body = file.read_bytes()
        if digest(body) != entry.get('sha256'):
            raise ValueError('ISED capture checksum mismatch')
        text = body.decode('utf-8-sig')
        if '<html' in text[:1000].lower():
            if 'id="foundItems"' in text:
                pages[name] = labelled_page(text)
        else:
            rows = list(csv.reader(io.StringIO(text), strict=True))
            if not rows or any(len(r) != 44 for r in rows):
                raise ValueError('ISED headerless CSV layout changed')
            exports.append(rows)
    selected, end, total = [], 0, None
    for name in data['result_pages']:
        if name not in pages:
            raise ValueError('Missing ISED result page')
        page = pages[name]
        if total is None:
            total = page['total']
        if page['total'] != total or page['start'] != (end + 1 if total else 0):
            raise ValueError('ISED page gap, overlap or changed total')
        end = page['end']
        selected.append(page)
    if end != total or (total == 0 and len(selected) != 1):
        raise ValueError('Incomplete ISED pagination')
    ranges = {(p['start'], p['end']): p for p in selected}
    for page in pages.values():
        if page != ranges.get((page['start'], page['end'])):
            raise ValueError('ISED repeated page changed during export')
    if len(exports) != (1 if total else 0):
        raise ValueError('Missing or unexpected ISED export')
    rows = [row for p in selected for row in p['rows']]
    exported = exports[0] if exports else []
    by_identity = {}
    for n, row in enumerate(exported, 1):
        try:
            date = datetime.fromisoformat(row[8]).date().isoformat()
        except ValueError as exc:
            raise ValueError('Invalid ISED CSV approval timestamp') from exc
        key = tuple([row[1], row[2], row[3], row[4], row[5], row[7], date])
        by_identity.setdefault(key, []).append({'row_number': n, 'cells': row})
    matched = set()
    for row in rows:
        matches = by_identity.get(tuple([row['company']] + row['cells']), [])
        row['csv_matches'] = matches
        matched.update(m['row_number'] for m in matches)
    return data['company_query'], rows, exported, {
        'query_records': total, 'csv_rows': len(exported),
        'html_without_exact_csv': sum(not row['csv_matches'] for row in rows),
        'unmatched_csv_rows': len(exported) - len(matched)}


def import_paged_capture(c, path):
    query, rows, exported, stats = validate_paged_capture(path)
    sid = 'ised-rel-query-' + normalize(query)
    c.source(sid, URL, 'regulatory', 'Public factual REL fields; ISED source rights retained',
             'Complete paginated public company-name query: ' + query + '. '
             'Contiguous HTML ranges and stable totals checked; repeated pages must agree. '
             'Company name and hardware identity come from labelled HTML. Group-heading numbers may not identify '
             'every row. CSV expands radio configurations and may differ from HTML; only exact matches are attached, '
             'all 44-column CSV rows remain independently searchable in ised_equipment, including unmatched '
             'rows and non-target company groups, with exact-HTML-match status. Company is a regulatory '
             'association, not physical OEM or deployment proof. '
             'Not all historical hardware or the full REL database. Capture comparison: ' + json.dumps(stats), path)
    store_export(c, sid, exported, {m['row_number'] for row in rows for m in row['csv_matches']})
    count = 0
    for n, row in enumerate(rows, 1):
        company, cells = row['company'], row['cells']
        if target_for(company) != target_for(query):
            continue
        ids = [('ic_id', cells[4])] if cells[4] else []
        facts = [('ised_hvin', cells[0]), ('ised_group_heading_number', row['group_number']),
                 ('ised_approval_date_raw', cells[5])]
        facts.extend((key, cells[i]) for i, key in [(1, 'ised_product_marketing_name'),
                     (2, 'ised_host_marketing_name'), (3, 'ised_firmware_version')] if cells[i])
        for number in sorted({m['cells'][0] for m in row['csv_matches']}):
            facts.append(('ised_company_number', number))
        if not row['csv_matches']:
            facts.append(('source_discrepancy', 'No exact CSV match; identity retained from labelled HTML only'))
        c.add(sid, str(n), URL, target_for(company), cells[0], cells[0], 'radio equipment',
              {'html_row_number': n, 'html_column_order': HEADERS, **row},
              [(company, 'regulatory_company')], ids, facts)
        count += 1
    c.finish(sid, len(rows))
    return {**stats, 'imported': count}
