"""A bounded, original-HTML PICOS hardware table with row-scoped specifications."""
import datetime as dt
import json
import re
from pathlib import Path

from .documents import dependency
from .fcc_index import Tree

SOURCE = 'picos45-edgecore-accton'
URL = 'https://pica8-fs.atlassian.net/wiki/spaces/PicOS45/pages/258605211/EdgeCore+Accton+Switch'
TITLE = 'EdgeCore/Accton Switch - PICOS-4.5_Configuration_Guide - PICOS Documentation'
HEADING = 'EdgeCore/Accton Switch'
HEADERS = ['Product', 'Model', 'Configuration', 'Switch ASIC', 'CPU']
MODEL = re.compile(r'((?:AS|AIS)\d{3,4}-\d+[A-Z]+)(?: \(((?:DCS|EPS)\d+)\))?')


def parse(body, expected_rows):
    if type(expected_rows) is not int or not 1 <= expected_rows <= 1000:
        raise ValueError('Invalid expected PICOS row count')
    root = Tree(body.decode('utf-8')).root
    if ([n.text() for n in root.walk('title')] != [TITLE]
            or [n.text() for n in root.walk('h1')] != [HEADING]):
        raise ValueError('Unexpected PICOS documentation page')
    tables = list(root.walk('table'))
    if len(tables) != 1 or tables[0].attrs.get('data-testid') != 'renderer-table':
        raise ValueError('Missing or ambiguous PICOS hardware table')
    rows = []
    for row in tables[0].walk('tr'):
        cells = [n for n in row.children if getattr(n, 'tag', '') in ('td', 'th')]
        if (len(cells) != len(HEADERS)
                or any(n.attrs.get(k, '1') != '1' for n in cells for k in ('rowspan', 'colspan'))):
            raise ValueError('PICOS table cell structure changed')
        rows.append([n.text() for n in cells])
    if not rows or rows[0] != HEADERS or len(rows) != expected_rows + 1:
        raise ValueError('PICOS headers or captured row count changed')
    ready, seen = [], set()
    for values in rows[1:]:
        row = dict(zip(HEADERS, values))
        match = MODEL.fullmatch(row['Model'])
        if (not match or match[1] in seen or not all(values)
                or not re.fullmatch(r'(?:\d+G|Multi-Gig) Switch Portfolio', row['Product'])):
            raise ValueError('Invalid, duplicate or unreviewed PICOS model row')
        seen.add(match[1])
        ready.append((match[1], match[2], row))
    return ready


def import_bundle(c, path):
    path = Path(path).resolve()
    data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('url') != URL
            or data.get('guide') != 'PICOS-4.5_Configuration_Guide'
            or data.get('capture_method') != 'original public HTML'):
        raise ValueError('Unrecognized PICOS hardware capture')
    dt.datetime.strptime(data['retrieved_at'], '%Y-%m-%d %H:%M:%S UTC')
    rows = parse(dependency(path.parent, data['html']), data.get('expected_rows'))
    c.source(SOURCE, URL, 'vendor', 'Factual hardware table; Pica8 retains source rights',
             'Complete ' + str(len(rows)) + '-row table in the captured PICOS 4.5 EdgeCore/Accton guide page; '
             'original public HTML is cached and hashed. Guide context is not a per-release '
             'support matrix, current support guarantee, image entitlement or license transfer. '
             'Row-level model names, parenthetical codes, ports, ASIC and CPU text are preserved. '
             'Edge-Core is an identity namespace; the joint EdgeCore/Accton heading supplies '
             'compatibility-partner affiliations only, not brand or manufacturer proof. '
             'No AWS deployment, auction-unit configuration or cross-brand hardware equivalence '
             'is inferred. Models outside this table are not proven unsupported.', path)
    for model, code, row in rows:
        facts = [('compatibility_software', 'PICOS'),
                 ('compatibility_guide', data['guide']),
                 ('compatibility_ports', row['Configuration']),
                 ('compatibility_asic', row['Switch ASIC']),
                 ('compatibility_cpu', row['CPU']),
                 ('source_vendor_group', 'EdgeCore/Accton'),
                 ('source_platform_heading', row['Model']),
                 ('source_portfolio', row['Product']),
                 ('retrieved_at', data['retrieved_at']),
                 ('identity_namespace', 'Edge-Core namespace for joint EdgeCore/Accton guide; manufacturer unestablished'),
                 ('software_installation_description',
                  'Listed in the PICOS 4.5 guide hardware table. Exact release support, '
                  'required firmware, image access and license entitlement are not established by this table.')]
        if code:
            facts.append(('source_parenthetical_model_code', code))
        c.add(SOURCE, model, URL, 'Edge-Core', model, model, 'network platform',
              {'table_row': row, 'guide': data['guide'], 'retrieved_at': data['retrieved_at']},
              [('Edge-Core', 'compatibility_partner'), ('Accton', 'compatibility_partner'),
               ('Pica8', 'software_vendor')], [('alias', code)] if code else [], facts)
    c.finish(SOURCE, len(rows))
    return {'platforms': len(rows)}
