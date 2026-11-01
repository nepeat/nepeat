"""Bounded Edgecore product-sitemap discovery and source-preserving import."""
import hashlib
import datetime
import html
import json
import re
import xml.etree.ElementTree as ET
from concurrent.futures import ThreadPoolExecutor
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urlparse

from .importers import Page
from .specifications import product_statements

ROOT_URL = 'https://www.edge-core.com/'
INDEX_URL = ROOT_URL + 'wp-sitemap.xml'
EXCLUDED_PRODUCTS = {'https://www.edge-core.com/product/ecview-pro/': 'Network management software, not a hardware device'}


def sitemap_urls(body):
    root = ET.fromstring(body)
    return [e.text for e in root.findall('{*}sitemap/{*}loc') + root.findall('{*}url/{*}loc')]


def product_urls(body):
    return sorted({u for u in sitemap_urls(body) if re.fullmatch(r'https://www\.edge-core\.com/product/[a-z0-9-]+/', u)})


class ProductPage(HTMLParser):
    def __init__(self, body):
        super().__init__(convert_charrefs=True)
        self.scripts, self.parts, self.capture = [], [], False
        self.headings, self.heading_parts, self.heading_tag = [], [], None
        self.feed(body)

    def handle_starttag(self, tag, attrs):
        if tag in ('h1', 'h2', 'h3', 'h4', 'h5', 'h6'):
            self.heading_parts, self.heading_tag = [], tag
        if tag == 'script' and dict(attrs).get('type') == 'application/ld+json':
            self.capture, self.parts = True, []

    def handle_data(self, data):
        if self.heading_tag:
            self.heading_parts.append(data)
        if self.capture:
            self.parts.append(data)

    def handle_endtag(self, tag):
        if tag == self.heading_tag:
            self.headings.append(' '.join(''.join(self.heading_parts).split()))
            self.heading_tag = None
        if tag == 'script' and self.capture:
            self.scripts.append(json.loads(''.join(self.parts)))
            self.capture = False

    def products(self):
        result = []
        for script in self.scripts:
            nodes = script if isinstance(script, list) else script.get('@graph', [script])
            for node in nodes:
                types = node.get('@type', [])
                if types == 'Product' or isinstance(types, list) and 'Product' in types:
                    result.append(node)
        return result


def parse_product(body):
    page = ProductPage(body)
    products = page.products()
    if len(products) != 1:
        raise ValueError('Expected one structured Edgecore Product')
    p = products[0]
    name = html.unescape(p['name']).removesuffix(' - Edgecore Networks').strip()
    if not name or len(name) > 150:
        raise ValueError('Unexpected Edgecore product name')
    # Extract aliases only from explicit "MODEL (AS...)" headings/link titles.
    # Other model strings may instead be compatible accessories or replacements.
    aliases = set()
    for title in page.headings + [t for t, _ in Page(body).links]:
        match = re.match(re.escape(name) + r'\s*\((AS[A-Za-z0-9-]+)\)', title, re.I)
        if match:
            aliases.add(match.group(1))
    return name, p, sorted(aliases)


def collect(raw, fetch, refresh=False):
    raw = Path(raw) / 'edgecore'
    raw.mkdir(parents=True, exist_ok=True)
    index = fetch(INDEX_URL, raw/'sitemap-index.xml', refresh, max_bytes=2_000_000)
    maps = sorted({u for u in sitemap_urls(index.read_text())
                   if re.fullmatch(r'https://www\.edge-core\.com/product-sitemap\d*\.xml', u)})
    if not 1 <= len(maps) <= 10:
        raise ValueError('Unexpected Edgecore product sitemap count')
    map_entries, urls = [], set()
    for u in maps:
        path = fetch(u, raw/urlparse(u).path.rsplit('/', 1)[1], refresh, max_bytes=8_000_000)
        urls.update(product_urls(path.read_text()))
        map_entries.append({'url': u, 'path': path.name, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
    if not 1 <= len(urls) <= 1000:
        raise ValueError('Unexpected English Edgecore product count')

    def one(u):
        slug = u.rstrip('/').rsplit('/', 1)[1]
        path = fetch(u, raw/('product-'+slug+'.html'), refresh, max_bytes=5_000_000)
        if u not in EXCLUDED_PRODUCTS:
            parse_product(path.read_text())
        return {'url': u, 'path': path.name, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}

    with ThreadPoolExecutor(max_workers=3) as pool:
        pages = list(pool.map(one, sorted(urls)))
    bundle = {'index': {'path': index.name, 'sha256': hashlib.sha256(index.read_bytes()).hexdigest()},
              'sitemaps': map_entries, 'pages': pages, 'expected_pages': len(urls)}
    path = raw/'products.json'
    tmp = path.with_suffix('.json.part')
    tmp.write_text(json.dumps(bundle, indent=2))
    tmp.replace(path)
    return path


def import_bundle(c, path):
    path = Path(path)
    bundle = json.loads(path.read_text())

    def read(entry):
        if Path(entry['path']).name != entry['path']:
            raise ValueError('Snapshot paths must be filenames within the bundle directory')
        body = (path.parent/entry['path']).read_bytes()
        if hashlib.sha256(body).hexdigest() != entry['sha256']:
            raise ValueError('Edgecore snapshot checksum mismatch')
        return body.decode('utf-8')

    maps = {u for u in sitemap_urls(read(bundle['index']))
            if re.fullmatch(r'https://www\.edge-core\.com/product-sitemap\d*\.xml', u)}
    if not maps or maps != {m['url'] for m in bundle['sitemaps']} or len(maps) != len(bundle['sitemaps']):
        raise ValueError('Incomplete Edgecore sitemap set')
    expected = set()
    for m in bundle['sitemaps']:
        expected.update(product_urls(read(m)))
    pages = bundle['pages']
    if not expected or len(pages) != len(expected) or len(pages) != bundle['expected_pages'] or {p['url'] for p in pages} != expected:
        raise ValueError('Incomplete or duplicate Edgecore product pages')
    parsed = []
    for entry in pages:
        body = read(entry)
        if entry['url'] not in EXCLUDED_PRODUCTS:
            parsed.append((entry, parse_product(body), product_statements(body)))
    sid = 'edgecore-products'
    c.source(sid, INDEX_URL, 'vendor', 'Edgecore factual product metadata; source rights retained',
             'All English /product/ URLs in the cached product sitemaps scanned; ECView Pro management software excluded. Includes inactive models, families and accessories. Scoped feature statements preserve power, cooling, installation and physical qualifiers at product level; not guaranteed configuration of a used unit. No exhaustive historical inventory or current availability claim.', path)
    for entry, (name, p, aliases), specifications in parsed:
        category = html.unescape(p.get('category', ''))
        facts = [('vendor_category', category), ('vendor_description', html.unescape(p.get('description', '')))]
        for prop in p.get('additionalProperty', []):
            facts.append(('vendor_'+prop['name'], str(prop['value'])))
        facts.extend(specifications)
        ids = [('alias', a) for a in aliases]
        if p.get('sku'):
            ids.append(('vendor_sku', str(p['sku'])))
        c.add(sid, entry['url'], entry['url'], 'Edge-Core', name, 'Edge-Core '+name,
              'vendor product', {'product': p, 'snapshot': entry, 'feature_statements': specifications}, [('Edge-Core', 'brand')], ids, facts)
    c.finish(sid, len(pages))
    return {'scanned': len(pages), 'imported': len(parsed), 'excluded': [p['url'] for p in pages if p['url'] in EXCLUDED_PRODUCTS]}


# Reviewed expansions preserve the exact source cell in each assertion.
MODEL_CELLS = {
    'ECS4110-28T/52T/28P': ['ECS4110-28T', 'ECS4110-52T', 'ECS4110-28P'],
    'ECS3528MV2/ECS3528MV2-DC': ['ECS3528MV2', 'ECS3528MV2-DC'],
    'Module: EM4660-10G SFP+': ['EM4660-10G SFP+'],
    'Module:EM4626H-XG10GSFP+ (for ECS4610-26T/50T)': ['EM4626H-XG10GSFP+'],
    'Module: EM4626H-XG-XFP (for ECS4620 series)': ['EM4626H-XG-XFP'],
    'Module:EM4625-XG-XENPAK (for ECS4625)': ['EM4625-XG-XENPAK'],
    'AS4610-54T / AS4610-54Tv1': ['AS4610-54T', 'AS4610-54Tv1'],
    'ECView Pro': [],
}


def resolve_models(cell):
    if cell in MODEL_CELLS:
        return MODEL_CELLS[cell]
    # Numeric-leading ordering codes and explicit regional/power suffixes are
    # retained literally. No AS prefix insertion or suffix stripping.
    if re.fullmatch(r'(?=.*\d)[A-Za-z0-9][A-Za-z0-9_+()-]*(?: (?:EU|UK|EUK|JP|US|AC|DC|2|KIT))?', cell):
        return [cell]
    raise ValueError('Unresolved affected-model cell: '+cell)


def notice_dates(body):
    visible = re.sub(r'<(script|style)\b[^>]*>.*?</\1>', '', body, flags=re.S|re.I)
    visible = ' '.join(html.unescape(re.sub('<[^>]+>', ' ', visible)).split())
    dates = []
    for m in re.finditer(r'\bEffective\s+([^:]{1,35}?\b\d{4})', visible, re.I):
        raw = m.group(1)
        normalized = re.sub(r'(\d)\s*(st|nd|rd|th)\b', r'\1', raw, flags=re.I)
        normalized = normalized.replace(',', ' ').replace('Auguest', 'August')
        normalized = re.sub(r'(\d)([A-Za-z])', r'\1 \2', normalized)
        normalized = ' '.join(normalized.split())
        parsed = None
        for fmt in ('%B %d %Y', '%b %d %Y', '%d %B %Y', '%d %b %Y'):
            try:
                parsed = datetime.datetime.strptime(normalized, fmt).date().isoformat()
                break
            except ValueError:
                pass
        if parsed:
            dates.extend([('notice_effective_date', parsed), ('notice_effective_date_text', raw)])
        else:
            dates.append(('notice_effective_date_unparsed', raw))
    return list(dict.fromkeys(dates))


class NoticePage(Page):
    def __init__(self, body):
        self.tables, self.current_table, self.canonical = [], None, None
        super().__init__(body)

    def handle_starttag(self, tag, attrs):
        values = dict(attrs)
        if tag == 'link' and values.get('rel') == 'canonical':
            self.canonical = values.get('href')
        if tag == 'table':
            self.current_table = len(self.rows)
        super().handle_starttag(tag, attrs)

    def handle_endtag(self, tag):
        super().handle_endtag(tag)
        if tag == 'table' and self.current_table is not None:
            self.tables.append(self.rows[self.current_table:])
            self.current_table = None


def parse_notice(body):
    page = NoticePage(body)
    if not page.canonical or not re.fullmatch(r'https://www\.edge-core\.com/eol-notice/[a-z0-9-]+/', page.canonical):
        raise ValueError('Missing or unexpected Edgecore notice URL')
    entries = []
    for table in page.tables:
        if not table or not table[0] or table[0][0] not in ('EOL Model', 'EOL Equipment', 'EOS Equipment'):
            continue
        for row in table[1:]:
            if not row or not any(row):
                continue
            model = row[0]
            if len(row) != len(table[0]):
                raise ValueError('Incomplete Edgecore notice row')
            source_row = dict(zip(table[0], row))
            for resolved in resolve_models(model):
                entries.append((resolved, source_row))
    if not entries and not page.canonical.endswith('/2021-may-eol-notice-ecview-pro/'):
        raise ValueError('No supported affected-model table; review this notice separately')
    return page, entries, notice_dates(body)


def import_notice(c, path):
    page, entries, dates = parse_notice(Path(path).read_text())
    sid = 'edgecore-eol-'+page.canonical.rstrip('/').rsplit('/', 1)[1]
    c.source(sid, page.canonical, 'vendor', 'Edgecore factual lifecycle table; source rights retained',
             'Affected-model rows from this notice, with reviewed expansion of explicit grouped cells. Software-only rows excluded. Effective dates come from visible notice text and are not support deadlines. Replacements are recommendations, not aliases.', path)
    for n, (model, row) in enumerate(entries):
        replacement = row.get('Replacement Model', row.get('Replacement', row.get('Similar Replacement', row.get('Equivalent substitute products', ''))))
        facts = [('lifecycle_notice', 'Affected model listed in an Edgecore retirement notice')] + dates
        facts.append(('notice_model_column', next(iter(row))))
        facts.append(('affected_model_cell', next(iter(row.values()))))
        if row.get('Description'):
            facts.append(('vendor_description', row['Description']))
        if replacement:
            facts.append(('vendor_replacement_text', replacement))
        if row.get('Reason'):
            facts.append(('lifecycle_reason', row['Reason']))
        ids = [('model_search', model)]
        if row.get('Part Number'):
            ids.append(('part_number', row['Part Number']))
        c.add(sid, str(n)+'|'+model, page.canonical, 'Edge-Core', model, 'Edge-Core '+model,
              'vendor product', row, [('Edge-Core', 'brand')], ids, facts)
    c.finish(sid, len(entries))
    return len(entries)


NOTICE_INDEX_URL = ROOT_URL+'category/eol-notice/'


def notice_urls(body):
    links = Page(body).links
    # The reviewed archive is one page. Fail explicitly if pagination appears,
    # rather than labeling only page one a complete archive snapshot.
    if any(re.search(r'/category/eol-notice/page/\d+', u) for _, u in links):
        raise ValueError('Edgecore notice index is paginated; collector needs updating')
    urls = sorted({u for _, u in links if re.fullmatch(r'https://www\.edge-core\.com/eol-notice/[a-z0-9-]+/', u)})
    if not 1 <= len(urls) <= 300:
        raise ValueError('Unexpected notice index size')
    return urls


def collect_notices(raw, fetch, refresh=False):
    raw = Path(raw)/'edgecore'
    index = fetch(NOTICE_INDEX_URL, raw/'notices-index.html', refresh, max_bytes=5_000_000)
    urls = notice_urls(index.read_text())

    def entry(path, u):
        return {'url': u, 'path': path.name, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}

    def one(u):
        path = fetch(u, raw/u.rstrip('/').rsplit('/', 1)[1], refresh, max_bytes=5_000_000)
        page, _, _ = parse_notice(path.read_text())
        if page.canonical != u:
            raise ValueError('Unexpected notice canonical URL: '+u)
        return entry(path, u)

    with ThreadPoolExecutor(max_workers=3) as pool:
        pages = list(pool.map(one, urls))
    bundle = {'index': entry(index, NOTICE_INDEX_URL), 'expected_pages': len(urls), 'pages': pages}
    path = raw/'notices.json'
    tmp = path.with_suffix('.json.part')
    tmp.write_text(json.dumps(bundle, indent=2))
    tmp.replace(path)
    return path


def import_notices(c, path):
    path = Path(path)
    bundle = json.loads(path.read_text())

    def read(entry):
        if Path(entry['path']).name != entry['path']:
            raise ValueError('Notice snapshot path must be a local filename')
        body = (path.parent/entry['path']).read_bytes()
        if hashlib.sha256(body).hexdigest() != entry['sha256']:
            raise ValueError('Notice snapshot checksum mismatch')
        return body.decode('utf-8')

    expected = notice_urls(read(bundle['index']))
    pages = bundle['pages']
    if len(pages) != bundle['expected_pages'] or sorted(p['url'] for p in pages) != expected:
        raise ValueError('Incomplete or duplicate Edgecore notice set')
    for entry in pages:
        page, _, _ = parse_notice(read(entry))
        if page.canonical != entry['url']:
            raise ValueError('Notice snapshot canonical URL mismatch')
    counts = [import_notice(c, path.parent/entry['path']) for entry in pages]
    # This source is coverage metadata; individual notice sources carry records.
    c.source('edgecore-notice-index', NOTICE_INDEX_URL, 'vendor', 'Edgecore notice index',
             'All notices linked in this cached one-page index validated and imported individually. Software-only notices create no hardware records. Older individually imported notices are retained if later absent from the index.', path)
    c.finish('edgecore-notice-index', len(pages))
    return {'notices': len(pages), 'affected_model_assertions': sum(counts), 'software_only_notices': counts.count(0)}
