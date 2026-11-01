"""Ghana NCA's public equipment table; detail pages are separate evidence."""
import json
import re
from pathlib import Path
from urllib.parse import urljoin

from .catalog import target_for
from .documents import dependency
from .fcc_index import Tree

SOURCE = 'ghana-nca-equipment'
URL = 'https://portal.nca.org.gh/search_type_approval.php'
TITLE = 'Online Application Portal Home - National Communication Authority'
HEADER = ['Equipment Type Number', 'Name of Manufacture', 'Brand Name', 'Product Type', 'Model Number', 'Details']
FIELDS = ['approval_number', 'manufacturer', 'brand', 'product_type', 'model', 'details_text']
SCOPE = ('All rows in the public HTML table checked against a second download. No independent database '
         'total or publication date is exposed; current and historical completeness are unproven. '
         'Manufacturer and brand are separate source claims. Blank approval numbers remain blank. '
         'Detail links are references, not proof those pages or certificates were retrieved. '
         'No factory, deployment, current approval validity or model equivalence is inferred.')


def company(value):
    if any(c.isalnum() and not c.isascii() for c in value):
        return None
    return target_for(value)


def parse(body):
    root = Tree(body.decode('utf-8')).root
    tables = [t for t in root.walk('table') if 'ta_datatable' in t.attrs.get('class', '').split()]
    if [n.text() for n in root.walk('title')] != [TITLE] or len(tables) != 1:
        raise ValueError('Unrecognized Ghana NCA table')
    table = tables[0]
    if len(list(table.walk('table'))) != 1:
        raise ValueError('Nested Ghana equipment tables')
    rows = list(table.walk('tr'))
    cells = [[c for c in r.children if getattr(c, 'tag', '') in ('th', 'td')] for r in rows]
    if len(cells) < 2 or [c.text() for c in cells[0]] != HEADER:
        raise ValueError('Ghana table empty or headings changed')
    result = []; ids = set()
    for n, row in enumerate(cells[1:], 1):
        if len(row) != len(HEADER):
            raise ValueError('Ghana table row width changed')
        links = list(row[-1].walk('a'))
        if len(links) != 1 or not re.fullmatch(r'search_type_approval_view_details\.php\?typeApproveDetailID=[1-9][0-9]*', links[0].attrs.get('href', '')):
            raise ValueError('Ghana detail link missing or changed')
        href = links[0].attrs['href']; detail_id = href.rsplit('=', 1)[1]
        if detail_id in ids:
            raise ValueError('Duplicate Ghana detail identity')
        ids.add(detail_id)
        values = [c.text() for c in row]
        result.append(dict(row_number=n, detail_id=detail_id, detail_url=urljoin(URL, href),
                           cells=values, **dict(zip(FIELDS, values))))
    return result


def validate(path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('source_url') != URL
            or type(data.get('expected_rows')) is not int or data['expected_rows'] < 1):
        raise ValueError('Unrecognized Ghana NCA bundle')
    rows = parse(dependency(path.parent, data['before']))
    after = parse(dependency(path.parent, data['after']))
    if rows != after or len(rows) != data['expected_rows']:
        raise ValueError('Ghana table changed or row count differs')
    return rows


def import_bundle(c, path):
    rows = validate(path)
    c.source(SOURCE, URL, 'regulatory', 'Ghana NCA public equipment table; source rights retained', SCOPE, path)
    c.db.execute('DELETE FROM ghana_equipment WHERE source_id=?', (SOURCE,))
    count = 0
    for r in rows:
        c.db.execute('INSERT INTO ghana_equipment VALUES (?,?,?,?,?,?,?,?,?,?,?)',
                     (SOURCE,r['row_number'],r['detail_id'],r['approval_number'],r['manufacturer'],
                      r['brand'],r['product_type'],r['model'],json.dumps(r['cells'],ensure_ascii=False),r['detail_url'],URL))
        target = company(r['manufacturer'])
        # Keep the complete printed label, including an apostrophe in Oracle's
        # product name. Lists, slash groups and parenthetical annotations stay raw.
        if not target or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._ '’-]*", r['model']):
            continue
        namespace = r['brand'] or target
        c.add(SOURCE,r['detail_id'],URL,namespace,r['model'],namespace+' '+r['model'],
              'registered equipment',r,
              [(r['manufacturer'],'registry_manufacturer'),(r['brand'],'brand')],
              [('ghana_type_approval_number',r['approval_number'])] if r['approval_number'] else [],
              [('registry_'+f,r[f]) for f in FIELDS] + [('detail_url',r['detail_url']),
               ('detail_page_scope','Reference link only; detail-page contents not imported'),('evidence_scope',SCOPE)])
        # Non-ASCII text cannot disappear into a spurious target brand alias.
        if r['brand'] and not company(r['brand']):
            c.db.execute('UPDATE affiliations SET target=NULL WHERE company=? AND role=? AND record_id IN '
                         '(SELECT id FROM records WHERE source_id=? AND source_key=?)',
                         (r['brand'],'brand',SOURCE,r['detail_id']))
        count += 1
    c.finish(SOURCE,len(rows))
    return dict(registry_rows=len(rows),model_assertions=count,
                blank_approval_numbers=sum(not r['approval_number'] for r in rows))
