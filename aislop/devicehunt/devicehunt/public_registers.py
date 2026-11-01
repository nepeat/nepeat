"""Raw public approval tables with no automatic manufacturer or model inference."""
import json
import re
from pathlib import Path

from .documents import dependency
from .fcc_index import Tree

SOURCES = {
    'ntrc-vc': {
        'url': 'https://www.ntrc.vc/consumer/type-approval-registry-2/',
        'title': 'Type Approval Registry – National Telecommunications Regulatory Commission (NTRC)',
        'table': 'tablepress-2',
        'header': ['Brand', 'Device', 'Model', 'Foreign Certification #'],
        'fields': ['brand', 'device_description', 'model', 'foreign_certification'],
        'scope': 'Saint Vincent and the Grenadines NTRC: all rows in one published HTML table. '
                 'No independent total or publication date; complete historical coverage is unproven. '
                 'Brand and foreign certification text remain source claims, not manufacturer evidence.',
    },
    'mauritius-icta': {
        'url': 'https://www.icta.mu/type-approved-list/',
        'title': 'Type Approved List – ICTA Website',
        'table': 'table_1',
        'header': ['TA number', 'Type', 'Make', 'Model'],
        'fields': ['approval_number', 'device_description', 'make', 'model'],
        'scope': 'Mauritius ICTA: public All-rows table query, unfiltered count checked and rows '
                 'compared across two downloads. This covers the returned table, not proven historical '
                 'completeness. Make is preserved separately from brand and manufacturer.',
    },
}
FIELDS = ['approval_number', 'brand', 'make', 'device_description', 'model', 'foreign_certification']
SCOPE = ('Raw approval-table rows; grouped labels, blank cells and duplicate rows are retained. '
         'No automatic catalog identity, manufacturer, factory, operator, current validity or '
         'model-equivalence claim is created. Foreign certification strings are not verified grants.')


def table_rows(body, config):
    text = body.decode('utf-8')
    root = Tree(text).root
    tables = [t for t in root.walk('table') if t.attrs.get('id') == config['table']]
    if config['title'] not in [t.text() for t in root.walk('title')] or len(tables) != 1:
        raise ValueError('Public register title or table changed')
    # Reject an unclosed table even though the HTML tree reader tolerates it.
    if not re.search(r'<table\b[^>]*\bid=["\']' + re.escape(config['table']) +
                     r'["\'][^>]*>.*?</table\s*>', text, re.S | re.I):
        raise ValueError('Unclosed public register table')
    if len(list(tables[0].walk('table'))) != 1:
        raise ValueError('Nested public register table')
    rows = [[c.text() for c in row.children if getattr(c, 'tag', '') in ('th', 'td')]
            for row in tables[0].walk('tr')]
    if not rows or rows[0] != config['header']:
        raise ValueError('Public register headings changed')
    rows = rows[1:]
    if config is SOURCES['mauritius-icta']:
        if not rows or rows[-1] != config['header']:
            raise ValueError('ICTA footer headings changed')
        rows = rows[:-1]
    if not rows or any(len(r) != 4 for r in rows):
        raise ValueError('Empty or malformed public register table')
    return rows


def api_rows(body):
    data = json.loads(body)
    if not isinstance(data, dict) or type(data.get('draw')) is not int:
        raise ValueError('Unrecognized public table response')
    counts = []
    for key in ('recordsTotal', 'recordsFiltered'):
        value = data.get(key)
        if type(value) is not int and not (isinstance(value, str) and re.fullmatch(r'[0-9]+', value)):
            raise ValueError('Invalid public table count')
        counts.append(int(value))
    rows = data.get('data')
    if (not isinstance(rows, list) or not rows or counts != [len(rows), len(rows)]
            or any(not isinstance(r, list) or len(r) != 4 or
                   any(not isinstance(v, str) for v in r) for r in rows)):
        raise ValueError('Partial, filtered or malformed public table response')
    return rows


def validate(path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    kind = data.get('kind'); config = SOURCES.get(kind)
    if (data.get('schema_version') != 1 or not config or data.get('source_url') != config['url']
            or type(data.get('expected_rows')) is not int or data['expected_rows'] < 1):
        raise ValueError('Unrecognized public register bundle')
    page_rows = table_rows(dependency(path.parent, data['page']), config)
    if kind == 'mauritius-icta':
        rows = api_rows(dependency(path.parent, data['before']))
        if rows != api_rows(dependency(path.parent, data['after'])):
            raise ValueError('Public table changed between downloads')
        # HTML text extraction collapses whitespace; compare like for like,
        # while preserving the unmodified API strings in the actual raw table.
        normalized = {tuple(' '.join(v.split()) for v in row) for row in rows}
        if any(tuple(row) not in normalized for row in page_rows):
            raise ValueError('HTML seed rows absent from full table')
    else:
        rows = page_rows
    if len(rows) != data['expected_rows']:
        raise ValueError('Public register row count differs')
    return kind, config, rows


def import_bundle(c, path):
    kind, config, rows = validate(path)
    sid = 'public-register-' + kind
    c.source(sid, config['url'], 'regulatory', 'Public regulator table; source rights retained',
             config['scope'] + ' ' + SCOPE, path)
    c.db.execute('DELETE FROM public_equipment_rows WHERE source_id=?', (sid,))
    for number, cells in enumerate(rows, 1):
        values = dict(zip(config['fields'], cells))
        c.db.execute('INSERT INTO public_equipment_rows VALUES (?,?,?,?,?,?,?,?,?,?)',
                     [sid, number] + [values.get(field, '') for field in FIELDS] +
                     [json.dumps(cells, ensure_ascii=False), config['url']])
    c.finish(sid, len(rows))
    return {'source': sid, 'registry_rows': len(rows), 'model_assertions': 0}
