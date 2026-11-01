"""Hash-pinned offline bulk index extraction. This module never uses the network.

Output rows are discovery links, not device, manufacturing or support assertions.
Run with the project Python: offline_parser.py --config inputs.json --output discovery.json
"""
from pathlib import Path
from html.parser import HTMLParser
from urllib.parse import urljoin, urlsplit, urlunsplit, parse_qsl, unquote
from collections import Counter
import argparse
import hashlib
import json
import re
import sqlite3
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
VOID = {'area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'param', 'source', 'track', 'wbr'}


def sha(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def norm(value):
    return ' '.join(value.split())


def endpoint(url):
    """Comparison key only; never replace original URLs or infer URL aliases."""
    p = urlsplit(url)
    return urlunsplit((p.scheme.lower(), p.netloc.lower(), p.path, '', ''))


class Node:
    def __init__(self, tag='', attrs=(), parent=None, line=0):
        self.tag, self.attrs, self.parent, self.line = tag, dict(attrs), parent, line
        self.parts, self.children = [], []

    @property
    def text(self):
        return ''.join(x.text if isinstance(x, Node) else x for x in self.parts)

    def find(self, predicate):
        result = []
        for n in self.children:
            if predicate(n):
                result.append(n)
            result.extend(n.find(predicate))
        return result


class Document(HTMLParser):
    def __init__(self, body):
        super().__init__(convert_charrefs=True)
        self.root = Node()
        self.stack = [self.root]
        self.feed(body)
        self.close()

    def handle_starttag(self, tag, attrs):
        node = Node(tag, attrs, self.stack[-1], self.getpos()[0])
        self.stack[-1].children.append(node)
        self.stack[-1].parts.append(node)
        if tag not in VOID:
            self.stack.append(node)

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        if tag not in VOID:
            self.handle_endtag(tag)

    def handle_endtag(self, tag):
        for i in range(len(self.stack) - 1, 0, -1):
            if self.stack[i].tag == tag:
                del self.stack[i:]
                break

    def handle_data(self, value):
        self.stack[-1].parts.append(value)

    def links(self, base):
        rows = []
        sections, last_heading = {}, ''
        for node in self.root.find(lambda n: n.tag in ('h3', 'table')):
            if node.tag == 'h3' and 'Group' in node.attrs.get('class', '').split():
                last_heading = node.text
            if node.tag == 'table':
                sections[id(node)] = last_heading
        for i, anchor in enumerate(self.root.find(lambda n: n.tag == 'a'), 1):
            href = anchor.attrs.get('href', '')
            url = urljoin(base, href)
            if not href or urlsplit(url).scheme not in ('http', 'https'):
                continue
            row = dict(url=url, href_raw=href, printed_label_raw=anchor.text,
                       printed_label=norm(anchor.text), title_attribute=anchor.attrs.get('title', ''),
                       row_number=i, source_line=anchor.line, row_kind='html_link', anchor_classes=anchor.attrs.get('class', '').split())
            ancestor = anchor.parent
            row['ancestor_classes'] = []
            while ancestor is not None:
                row['ancestor_classes'].extend(ancestor.attrs.get('class', '').split())
                if ancestor.tag == 'table':
                    row['table_id'] = ancestor.attrs.get('id', '')
                    row['section_label_raw'] = sections.get(id(ancestor), '')
                    row['section_label'] = norm(row['section_label_raw'])
                ancestor = ancestor.parent
            row['ancestor_classes'] = list(dict.fromkeys(row['ancestor_classes']))
            card = anchor.parent
            while card is not None:
                if 'well' in card.attrs.get('class', '').split() and 'c2a3' in card.attrs.get('class', '').split():
                    headings = card.find(lambda n: n.tag == 'h3' and 'media-heading' in n.attrs.get('class', '').split())
                    metadata = card.find(lambda n: 'text-muted' in n.attrs.get('class', '').split())
                    row['document_heading_raw'] = headings[0].text if headings else ''
                    row['document_heading'] = norm(row['document_heading_raw'])
                    row['document_metadata_raw'] = metadata[0].text if metadata else ''
                    row['document_metadata'] = norm(row['document_metadata_raw'])
                    break
                card = card.parent
            rows.append(row)
        return rows


def sitemap_rows(body):
    root = ET.fromstring(body)
    kind = root.tag.rsplit('}', 1)[-1]
    if kind not in ('sitemapindex', 'urlset'):
        raise ValueError('Expected a sitemap, not an HTML error page')
    rows = []
    for i, node in enumerate(root, 1):
        loc, lastmod = node.find('{*}loc'), node.find('{*}lastmod')
        if loc is None or not loc.text:
            continue
        if urlsplit(loc.text).scheme not in ('http', 'https'):
            raise ValueError('Non-HTTP sitemap location')
        rows.append(dict(url=loc.text, loc_raw=loc.text, printed_label='', printed_label_raw='',
                         row_number=i, lastmod_raw=lastmod.text if lastmod is not None else None,
                         row_kind='sitemap_pointer' if kind == 'sitemapindex' else 'sitemap_url'))
    return rows


def checked_path(root, relative, expected_hash):
    path = (root / relative).resolve()
    if not path.is_relative_to(root.resolve()) or not path.is_file() or sha(path) != expected_hash:
        raise ValueError('Input path or hash mismatch: ' + relative)
    return path


def classify(row, source):
    if source['id'] == 'edgecore_product_downloads':
        return 'product_document_link'
    if source['id'] == 'oracle_older_sparc_indexes':
        return 'product_documentation_index_link'
    if source['id'] == 'oracle_legacy_x86':
        return 'legacy_product_documentation_index_link'
    if source['id'] == 'celestica_sitemap':
        return ('platform_portfolio_navigation' if '/platform-solutions/' in urlsplit(row['url']).path
                else 'corporate_or_other_navigation')
    return 'unclassified_link'


def extract(config, root=ROOT):
    rows, inputs = [], []
    for dependency in config.get('dependencies', []):
        checked_path(root, dependency['path'], dependency['sha256'])
    for source in config['sources']:
        for capture in source['inputs']:
            if capture['status'] != 200:
                raise ValueError('Only successful HTTP 200 inputs may be parsed')
            path = checked_path(root, capture['path'], capture['sha256'])
            inputs.append(dict(capture, source=source['id']))
            parsed = (sitemap_rows(path.read_bytes()) if capture['format'] == 'xml' else
                      Document(path.read_text(encoding='utf-8', errors='strict')).links(capture['url']))
            for row in parsed:
                p = urlsplit(row['url'])
                if source.get('required_anchor_class') and source['required_anchor_class'] not in row.get('anchor_classes', []):
                    continue
                if source.get('required_ancestor_class') and source['required_ancestor_class'] not in row.get('ancestor_classes', []):
                    continue
                if source.get('same_host') and p.netloc != urlsplit(capture['url']).netloc:
                    continue
                if source.get('path_pattern') and not re.search(source['path_pattern'], p.path):
                    continue
                row.update(source=source['id'], source_role=source['source_role'],
                           capture_path=capture['path'], capture_sha256=capture['sha256'], source_url=capture['url'],
                           endpoint_comparison_key=endpoint(row['url']), query_pairs=parse_qsl(p.query, keep_blank_values=True),
                           evidence_class='discovery_only', identity_assertion=False,
                           manufacturer_assertion=False, deployment_assertion=False)
                row['classification'] = classify(row, source)
                row['wpdmdl_values'] = [v for k, v in row['query_pairs'] if k == 'wpdmdl']
                text = row.get('document_heading', '') + ' ' + row['printed_label'] + ' ' + row.get('title_attribute', '')
                row['revision_strings_as_printed'] = re.findall(r'(?<![\w])R\d+(?:[.\-]\d+)*(?![\w])', text)
                row['url_revision_strings'] = re.findall(r'(?<![\w])r\d+(?:[.]\d+)*(?![\w])', unquote(p.path), re.I)
                row['revision_label_url_disagreement'] = bool(row['revision_strings_as_printed'] and row['url_revision_strings'] and {v.casefold() for v in row['revision_strings_as_printed']} != {v.casefold() for v in row['url_revision_strings']})
                # Tokens are intentionally heuristic leads; no expansion of ranges or families.
                row['identifier_token_leads'] = list(dict.fromkeys(re.findall(
                    r'(?<![A-Za-z0-9])(?:AS\d{4}[A-Za-z0-9+-]*(?:-[A-Za-z0-9+]+)*|[A-Z]{1,5}\d{2,5}(?:-[A-Za-z0-9+]+)*)(?![A-Za-z0-9])', text)))
                row['restrictions'] = [r for r in config['restrictions'] if re.search(r['pattern'], text + ' ' + row['url'], re.I)]
                row['capture_scheduling'] = 'excluded_known_restriction_or_missing' if row['restrictions'] else 'requires_separate_reviewed_capture_plan'
                rows.append(row)
    return inputs, rows


def compare_catalog(rows, dbpath):
    before = sha(dbpath)
    with sqlite3.connect(dbpath.resolve().as_uri() + '?mode=ro', uri=True) as db:
        db.execute('PRAGMA query_only=ON')
        sources = {r[0] for r in db.execute('SELECT url FROM sources')}
        records = {r[0] for r in db.execute('SELECT source_url FROM records')}
        endpoints = {endpoint(u) for u in sources | records}
        models = {}
        for did, brand, model in db.execute('SELECT id, brand, model FROM devices ORDER BY id'):
            models.setdefault(norm(model).casefold(), []).append(dict(device_id=did, brand=brand, model=model))
        identifiers = {}
        for did, kind, value in db.execute('SELECT DISTINCT r.device_id,i.kind,i.value FROM identifiers i JOIN records r ON r.id=i.record_id ORDER BY r.device_id,i.kind,i.value'):
            identifiers.setdefault(norm(value).casefold(), []).append(dict(device_id=did, kind=kind, value=value))
        for row in rows:
            row['raw_url_in_catalog'] = row['url'] in sources | records
            row['endpoint_in_catalog'] = row['endpoint_comparison_key'] in endpoints
            labels = list(dict.fromkeys(filter(None, [row['printed_label'], row.get('document_heading', '')])))
            row['complete_label_model_matches'] = {label: models[norm(label).casefold()] for label in labels if norm(label).casefold() in models}
            row['complete_label_identifier_matches'] = {label: identifiers[norm(label).casefold()] for label in labels if norm(label).casefold() in identifiers}
            row['token_model_matches_leads_only'] = {t: models[t.casefold()] for t in row['identifier_token_leads'] if t.casefold() in models}
    if sha(dbpath) != before:
        raise ValueError('Catalog changed during comparison')
    return before


def summarize(rows, inputs, database_hash):
    return dict(rows=len(rows), input_captures=len(inputs),
                by_source=dict(Counter(r['source'] for r in rows)),
                by_classification=dict(Counter(r['classification'] for r in rows)),
                distinct_raw_urls=len({r['url'] for r in rows}),
                distinct_query_free_endpoints=len({r['endpoint_comparison_key'] for r in rows}),
                raw_urls_absent_from_catalog=len({r['url'] for r in rows if not r['raw_url_in_catalog']}),
                query_free_endpoints_absent_from_catalog=len({r['endpoint_comparison_key'] for r in rows if not r['endpoint_in_catalog']}),
                rows_with_complete_label_model_matches=sum(bool(r['complete_label_model_matches']) for r in rows),
                rows_with_complete_label_identifier_matches=sum(bool(r['complete_label_identifier_matches']) for r in rows),
                restricted_rows=sum(bool(r['restrictions']) for r in rows),
                catalog_sha256=database_hash,
                limits='Catalog absence is not proof a URL is uncaptured. Discovery rows, labels, token matches and product-to-link placement assert no identities, aliases, manufacture, compatibility, deployment, availability or current support.')


def build(config, root=ROOT):
    inputs, rows = extract(config, root)
    dbhash = compare_catalog(rows, root / config['catalog_path'])
    return dict(schema_version=2, config_sha256=hashlib.sha256(json.dumps(config, sort_keys=True, ensure_ascii=False).encode()).hexdigest(),
                inputs=inputs, summary=summarize(rows, inputs, dbhash), rows=rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', default=str(HERE / 'inputs-ready.json'))
    parser.add_argument('--output', default=str(HERE / 'discovery.json'))
    args = parser.parse_args()
    config = json.loads(Path(args.config).read_text())
    result = build(config)
    with Path(args.output).open('x', encoding='utf-8') as f:
        json.dump(result, f, indent=2, ensure_ascii=False)
        f.write('\n')
    print(json.dumps(result['summary'], indent=2))


if __name__ == '__main__':
    main()
