"""Original public sitemap snapshots: discovery URLs, never filing/model proof."""
import json
import re
import xml.etree.ElementTree as ET
from pathlib import Path
from urllib.parse import urlsplit

from .documents import dependency

SOURCE = 'fcc-sitemaps'
URL = 'https://fccid.co/sitemap.xml'
NS = '{http://www.sitemaps.org/schemas/sitemap/0.9}'
SCOPE = ('Original public FCCID.co sitemap index and advertised child captures. '
         'Coverage is limited to the advertised sitemap snapshot, not complete FCC history. '
         'Absent years and unavailable children are not zero filings. Sitemap lastmod is '
         'site metadata, not a filing, grant or product release date. URL-derived FCC IDs '
         'are discovery strings; no grantee, model, manufacturer, application, PDF or '
         'hyperscaler assertion is inferred. Duplicate URLs and cross-year repetitions '
         'are retained. Navigation, company and classification URLs remain separate kinds.')


def tree(body, expected):
    if len(body) > 12_000_000 or b'<!DOCTYPE' in body.upper() or b'<!ENTITY' in body.upper():
        raise ValueError('Oversized sitemap or forbidden XML declaration')
    try:
        root = ET.fromstring(body)
    except ET.ParseError as exc:
        raise ValueError('Malformed sitemap XML') from exc
    if root.tag != NS + expected:
        raise ValueError('Unexpected sitemap root')
    return root


def fields(node, allowed):
    result = {}
    for child in node:
        key = child.tag.removeprefix(NS)
        if child.tag != NS + key or key not in allowed or key in result or len(child):
            raise ValueError('Unknown, nested or repeated sitemap field')
        result[key] = child.text or ''
    value = result.get('loc', '')
    parsed = urlsplit(value)
    if (parsed.scheme != 'https' or parsed.netloc != 'fccid.co'
            or parsed.fragment or not value or value != value.strip()
            or any(c.isspace() for c in value)):
        raise ValueError('Missing or noncanonical sitemap location')
    return result


def parse_index(body):
    result = []
    for node in tree(body, 'sitemapindex'):
        if node.tag != NS + 'sitemap':
            raise ValueError('Unexpected sitemap-index entry')
        row = fields(node, {'loc', 'lastmod'})
        if not re.fullmatch(r'https://fccid\.co/sitemaps/(main|applications-\d{4}|companies|equipment-classes)\.xml', row['loc']):
            raise ValueError('Unreviewed child sitemap URL')
        result.append(row)
    if not result or len({r['loc'] for r in result}) != len(result):
        raise ValueError('Empty or repeated sitemap index')
    return result


def parse_urls(body):
    rows = []
    for node in tree(body, 'urlset'):
        if node.tag != NS + 'url':
            raise ValueError('Unexpected URL-set entry')
        row = fields(node, {'loc', 'lastmod', 'changefreq', 'priority'})
        path = urlsplit(row['loc']).path
        match = re.fullmatch(r'/fcc/([A-Za-z0-9-]+)', path)
        row['fcc_id'] = match.group(1) if match and not urlsplit(row['loc']).query else ''
        row['page_kind'] = ('fcc' if row['fcc_id'] else 'company' if path.startswith('/company/')
                            else 'equipment_class' if path.startswith('/equipment-class/') else 'other')
        rows.append(row)
    return rows


def validate(path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('source_url') != URL
            or not isinstance(data.get('children'), list)):
        raise ValueError('Invalid sitemap capture bundle')
    index = parse_index(dependency(path.parent, data['index']))
    if [r.get('url') for r in data['children']] != [r['loc'] for r in index]:
        raise ValueError('Capture must account for every advertised child in index order')
    for row in data['children']:
        if row.get('status') == 'captured':
            count = len(parse_urls(dependency(path.parent, row['page'])))
            if type(row.get('row_count')) is not int or row['row_count'] != count:
                raise ValueError('Sitemap row count changed')
        elif row.get('status') == 'unavailable':
            if not isinstance(row.get('issue'), str) or not row['issue'].strip() or 'page' in row or 'row_count' in row:
                raise ValueError('Unavailable sitemap needs a reason, not an empty capture')
        else:
            raise ValueError('Unrecognized sitemap capture status')
    return path, data, index


def import_bundle(c, path):
    path, data, index = validate(path)  # Validate all files before source replacement.
    count = 0; unavailable = 0
    with c.db:
        c.source(SOURCE, URL, 'regulatory_mirror', 'Public sitemap metadata; mirror rights retained', SCOPE, path)
        c.db.execute('DELETE FROM fcc_sitemap_pages WHERE source_id=?', (SOURCE,))
        for capture, listed in zip(data['children'], index):
            captured = capture['status'] == 'captured'
            # Recheck dependencies during insertion; a concurrent file change rolls back.
            rows = parse_urls(dependency(path.parent, capture['page'])) if captured else []
            c.db.execute('INSERT INTO fcc_sitemap_pages VALUES (?,?,?,?,?,?,?)',
                         (SOURCE, capture['url'], listed.get('lastmod', ''), capture['status'],
                          capture.get('issue', ''), len(rows) if captured else None,
                          capture['page']['sha256'] if captured else ''))
            c.db.executemany('INSERT INTO fcc_sitemap_urls VALUES (?,?,?,?,?,?,?,?,?)',
                ((SOURCE, capture['url'], n, r['loc'], r['fcc_id'], r['page_kind'],
                  r.get('lastmod', ''), r.get('changefreq', ''), r.get('priority', ''))
                 for n, r in enumerate(rows, 1)))
            count += len(rows); unavailable += not captured
        c.db.execute('UPDATE sources SET records_seen=? WHERE id=?', (count, SOURCE))
    return dict(sitemaps=len(index), unavailable=unavailable, url_rows=count, catalog_assertions=0)
