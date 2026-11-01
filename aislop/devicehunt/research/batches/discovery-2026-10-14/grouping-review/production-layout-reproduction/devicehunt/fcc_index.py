"""Cached public FCC company indexes, separate from grants and model identities."""
import datetime as dt
import json
import re
from html.parser import HTMLParser
from pathlib import Path

from .catalog import digest

BASE = 'https://fccid.co'
GRANTEES = {'HED': 'Accton Technology Corp', 'YZK': 'Edgecore Networks Corporation',
            'A4H': 'Oracle America, Inc', '2AVPL': 'Amazon Data Services, Inc.',
            'MRF': 'Celestica Incorporated', 'NVN': 'Celestica Limited'}
VOID = {'area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'param', 'source', 'track', 'wbr'}


class Node:
    def __init__(self, tag='', attrs=(), parent=None):
        self.tag, self.attrs, self.parent, self.children = tag, dict(attrs), parent, []

    def walk(self, tag=None):
        if tag is None or self.tag == tag:
            yield self
        for child in self.children:
            if isinstance(child, Node):
                yield from child.walk(tag)

    def text(self):
        return ' '.join(' '.join(c.text() if isinstance(c, Node) and c.tag not in ('script', 'style')
                                else c if isinstance(c, str) else '' for c in self.children).split())


class Tree(HTMLParser):
    def __init__(self, text):
        super().__init__(convert_charrefs=True)
        self.root = self.current = Node()
        self.feed(text)

    def handle_starttag(self, tag, attrs):
        node = Node(tag, attrs, self.current)
        self.current.children.append(node)
        if tag not in VOID:
            self.current = node

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        if tag not in VOID:
            self.handle_endtag(tag)

    def handle_endtag(self, tag):
        node = self.current
        while node.parent is not None:
            if node.tag == tag:
                self.current = node.parent
                break
            node = node.parent

    def handle_data(self, data):
        self.current.children.append(data)


def document(text):
    root = Tree(text).root
    # Next.js sends some HTML fragments after placeholders. Reconnect only the
    # explicit static $RS(source,target) moves; never evaluate JavaScript.
    nodes = [n for n in root.walk() if n.attrs.get('id')]
    ids = {n.attrs['id']: n for n in nodes}
    if len(ids) != len(nodes):
        raise ValueError('Duplicate HTML fragment ID')
    moves = []
    for script in root.walk('script'):
        body = ''.join(c for c in script.children if isinstance(c, str))
        if body.startswith('$RS=') or body.startswith('$RS('):
            moves.extend(re.findall(r'\$RS\("([^"]+)","([^"]+)"\)', body))
    for source, target in moves:
        if source not in ids or target not in ids or ids[target].tag != 'template':
            raise ValueError('Unresolved streamed HTML fragment')
        src, dst = ids[source], ids[target]
        ancestor = dst
        while ancestor:
            if ancestor is src:
                raise ValueError('Cyclic HTML fragment')
            ancestor = ancestor.parent
        parent = dst.parent
        if dst not in parent.children or src not in src.parent.children:
            raise ValueError('Repeated HTML fragment move')
        index = parent.children.index(dst)
        parent.children[index:index + 1] = src.children
        for child in src.children:
            if isinstance(child, Node): child.parent = parent
        src.parent.children.remove(src)
    return root


def page_url(code, page=1):
    return BASE + '/company/' + code + ('' if page == 1 else '?page=' + str(page))


def detail_index_rows(db):
    """Only this mirror's original company captures are inputs to its detail parser."""
    sources = ['fcc-index-' + code.lower() for code in GRANTEES]
    return [dict(r) for r in db.execute(
        'SELECT fcc_id,grantee_code,grantee_name,source_url FROM fcc_id_index '
        'WHERE source_id IN (' + ','.join('?' for _ in sources) + ') ORDER BY fcc_id', sources)]


def parse_page(body, code, page):
    root = document(body.decode('utf-8'))
    titles = [n.text() for n in root.walk('title')]
    if any(t.startswith('Company Not Found') for t in titles):
        if list(root.walk('h1')):
            raise ValueError('Conflicting FCC company not-found page')
        return {'state': 'not_found_in_mirror', 'total': None, 'pages': None, 'rows': []}
    headings = [n.text() for n in root.walk('h1')]
    if headings != [GRANTEES[code]]:
        raise ValueError('FCC index company heading differs: ' + code)
    labels = {}
    for dl in root.walk('dl'):
        for item in dl.walk('div'):
            keys, values = list(item.walk('dt')), list(item.walk('dd'))
            if len(keys) == len(values) == 1:
                labels[keys[0].text()] = values[0].text()
    if labels.get('Grantee Code') != code or not re.fullmatch(r'[0-9,]+', labels.get('Total Applications', '')):
        raise ValueError('Missing FCC company count or code')
    total = int(labels['Total Applications'].replace(',', ''))
    pages = max(1, (total + 24) // 25)
    if not 1 <= page <= pages:
        raise ValueError('FCC index page outside reported scope')
    rows = []
    for anchor in root.walk('a'):
        fid = anchor.text()
        if not fid or anchor.attrs.get('href') != '/fcc/' + fid:
            continue
        if not re.fullmatch(re.escape(code) + r'[A-Z0-9-]+', fid):
            raise ValueError('Unexpected FCC ID in company index')
        card = anchor.parent
        while card and not any(n.text() == 'Equipment Class' for n in card.walk('p')):
            card = card.parent
        if card is None or len([a for a in card.walk('a') if a.text() and a.attrs.get('href', '').startswith('/fcc/')]) != 1:
            raise ValueError('Ambiguous FCC card boundary')
        if list(card.walk('template')):
            raise ValueError('Unresolved FCC card content')
        description = [n.text() for n in anchor.parent.walk('p')]
        companies = [a.text() for a in card.walk('a') if a.attrs.get('href') == '/company/' + code]
        dates = [n.text() for n in card.walk('span') if re.fullmatch(r'[A-Z][a-z]{2} \d{2}, \d{4}', n.text())]
        if len(description) != 1 or companies != [GRANTEES[code]] or len(dates) != 1:
            raise ValueError('Incomplete FCC card identity/date fields')
        dt.datetime.strptime(dates[0], '%b %d, %Y')
        fields = {}
        for label in card.walk('p'):
            if label.text() in ('Equipment Class', 'Application Purpose'):
                siblings = [n for n in label.parent.children if isinstance(n, Node) and n is not label]
                if len(siblings) != 1 or not siblings[0].text():
                    raise ValueError('Missing FCC card field')
                fields[label.text()] = siblings[0].text()
        if set(fields) != {'Equipment Class', 'Application Purpose'}:
            raise ValueError('FCC card fields changed')
        rows.append(dict(fcc_id=fid, grantee_code=code, grantee_name=GRANTEES[code],
                         listed_date=dates[0], product_description=description[0],
                         equipment_class=fields['Equipment Class'], application_purpose=fields['Application Purpose'],
                         source_url=BASE + '/fcc/' + fid, page_url=page_url(code, page)))
    expected = min(25, total - (page - 1) * 25)
    if len(rows) != expected or len({r['fcc_id'] for r in rows}) != len(rows):
        raise ValueError('FCC card count or uniqueness differs from pagination')
    if pages > 1:
        if not re.search(r'\bPage ' + str(page) + r' of ' + str(pages) + r'\b', root.text()):
            raise ValueError('FCC page number differs')
    return {'state': 'listed', 'total': total, 'pages': pages, 'rows': rows}


def import_bundle(c, path):
    path = Path(path)
    data = json.loads(path.read_text())
    if data.get('schema_version') != 1 or data.get('mirror') != BASE or set(data['companies']) != set(GRANTEES):
        raise ValueError('Unrecognized FCC company index bundle')
    collected = {}
    for code, entries in data['companies'].items():
        if not entries:
            raise ValueError('Missing FCC company capture')
        pages = []
        for number, entry in enumerate(entries, 1):
            name = entry['path']
            if (not isinstance(name, str) or Path(name).name != name or name in ('.', '..')
                    or entry['url'] != page_url(code, number)):
                raise ValueError('Invalid FCC index page reference')
            file = (path.parent / name).resolve()
            if file.parent != path.parent.resolve():
                raise ValueError('FCC page escapes bundle directory')
            body = file.read_bytes()
            if digest(body) != entry['sha256']:
                raise ValueError('FCC page checksum mismatch')
            pages.append(parse_page(body, code, number))
        first = pages[0]
        rows = [r for p in pages for r in p['rows']]
        if (len(entries) != (first['pages'] or 1)
                or any((p['state'], p['total'], p['pages']) != (first['state'], first['total'], first['pages']) for p in pages)
                or len(rows) != (first['total'] or 0) or len({r['fcc_id'] for r in rows}) != len(rows)):
            raise ValueError('FCC company page set is incomplete or inconsistent')
        collected[code] = (first, rows)
    # Validate every dependency before replacing any source, including explicit
    # mirror-not-found captures which are not evidence of no FCC registrations.
    for code, (summary, rows) in collected.items():
        sid = 'fcc-index-' + code.lower()
        scope = ('All pages returned by this mirror for grantee ' + code + '. Capture state: ' + summary['state'] + '. '
                 'Index entries only, not complete applications, exhibits, model names or manufacturer claims. '
                 'The mirror is known to omit entries present in other sources; listed dates are verbatim and may '
                 'differ from FCC grant/application dates. Not-found pages do not establish zero FCC filings.')
        c.source(sid, page_url(code), 'regulatory_mirror', 'Factual public index metadata; source rights retained', scope, path)
        c.db.execute('DELETE FROM fcc_id_index WHERE source_id=?', (sid,))
        for r in rows:
            c.db.execute('INSERT INTO fcc_id_index VALUES (?,?,?,?,?,?,?,?,?,?)',
                         [sid] + [r[k] for k in ('fcc_id', 'grantee_code', 'grantee_name', 'listed_date',
                          'product_description', 'equipment_class', 'application_purpose', 'source_url', 'page_url')])
        c.db.execute('UPDATE sources SET records_seen=? WHERE id=?', (len(rows), sid))
    c.db.commit()
    return {code: {'state': s['state'], 'rows': len(rows), 'pages': s['pages']} for code, (s, rows) in collected.items()}
