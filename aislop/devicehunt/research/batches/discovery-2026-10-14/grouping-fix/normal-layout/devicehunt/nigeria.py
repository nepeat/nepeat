"""Nigeria NCC's published equipment CSV and its independently rendered table."""
import base64
import csv
import io
import json
import re
from pathlib import Path
from urllib.parse import urljoin

from .catalog import target_for
from .documents import dependency
from .fcc_index import Tree

SOURCE = 'nigeria-ncc-equipment'
URL = 'https://www.ncc.gov.ng/page/approved-equipment'
HEADER = ['SN', 'APPLICANT', 'CERTIFICATE HOLDER', 'EQUIPMENT NAME', 'MODELS', 'MANUFACTURER']
FIELDS = ['serial_number', 'applicant', 'certificate_holder', 'equipment_name', 'models', 'manufacturer']
SCOPE = ('Complete published CSV checked against two original HTML tables, not proof of the full '
         'historical/live regulator database. Row serial numbers are not certificate IDs. Manufacturer, '
         'certificate holder and applicant remain distinct source claims; no factory, deployment or '
         'current approval validity inferred. UTF-8 decoding errors are flagged; affected original '
         'cell bytes are retained as base64 without guessing an encoding or repairing source text.')


def validate_rows(rows):
    if not rows or rows[0] != HEADER or len(rows) < 2:
        raise ValueError('Nigeria NCC equipment headings changed or register empty')
    for n, row in enumerate(rows[1:], 1):
        if len(row) != 6 or row[0] != str(n):
            raise ValueError('Nigeria NCC row width or serial sequence changed')


def html_table(body):
    # This Drupal page has repeated unrelated element IDs. Parse the static
    # table directly; do not apply the FCC reader's Next.js fragment moves.
    root = Tree(body.decode('utf-8')).root
    tables = list(root.walk('table'))
    if ([n.text() for n in root.walk('title')] != ['Approved Equipment | NCC'] or len(tables) != 1):
        raise ValueError('Unrecognized Nigeria NCC equipment page')
    rows = [[c.text() for c in row.children if getattr(c, 'tag', '') in ('th', 'td')]
            for row in tables[0].walk('tr')]
    validate_rows(rows)
    links = {urljoin(URL, n.attrs.get('href', '')) for n in root.walk('a')
             if re.fullmatch(r'/sites/default/files/\d{4}-\d{2}/[A-Za-z0-9_-]+\.csv', n.attrs.get('href', ''))}
    if len(links) != 1:
        raise ValueError('Nigeria NCC CSV link missing or ambiguous')
    return rows, links.pop()


def csv_table(body):
    # Latin-1 is only a reversible bridge through the CSV quoting parser.
    # Actual cells are decoded as UTF-8, matching the official HTML display.
    byte_rows = list(csv.reader(io.StringIO(body.decode('latin1'), newline=''), strict=True))
    rows, errors = [], []
    for byte_row in byte_rows:
        values, bad = [], {}
        for i, value in enumerate(byte_row):
            raw = value.encode('latin1')
            try:
                values.append(raw.decode('utf-8'))
            except UnicodeDecodeError:
                values.append(raw.decode('utf-8', errors='replace'))
                bad[str(i)] = base64.b64encode(raw).decode('ascii')
        rows.append(values); errors.append(bad)
    validate_rows(rows)
    return rows, errors


def validate(path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('source_url') != URL
            or data.get('csv_decoding') != 'UTF-8 with flagged replacement and original invalid cell bytes'
            or type(data.get('expected_rows')) is not int or data['expected_rows'] < 1):
        raise ValueError('Unrecognized Nigeria NCC bundle')
    first, first_url = html_table(dependency(path.parent, data['page_before']))
    second, second_url = html_table(dependency(path.parent, data['page_after']))
    rows, errors = csv_table(dependency(path.parent, data['csv']))
    displayed = [[' '.join(c.split()) for c in row] for row in rows]
    if (first != second or displayed != first or first_url != second_url
            or data['csv'].get('url') != first_url or len(rows) - 1 != data['expected_rows']):
        raise ValueError('Nigeria NCC CSV/table content, count or linked URL mismatch')
    return data, rows[1:], errors[1:]


def company(name):
    if any(c.isalnum() and not c.isascii() for c in name):
        return None
    return target_for(name)


def import_bundle(c, path):
    data, rows, errors = validate(path)
    c.source(SOURCE, URL, 'regulatory', 'Nigeria NCC public register; source rights retained', SCOPE, path)
    c.db.execute('DELETE FROM nigeria_equipment WHERE source_id=?', (SOURCE,))
    count = 0
    for n, (row, bad) in enumerate(zip(rows, errors), 1):
        r = dict(zip(FIELDS, row))
        c.db.execute('INSERT INTO nigeria_equipment VALUES (?,?,?,?,?,?,?,?,?,?,?)',
                     (SOURCE, n, *row, json.dumps(row, ensure_ascii=False), json.dumps(bad), data['csv']['url']))
        target = company(r['manufacturer']); model = r['models'].strip()
        # Grouped/annotated labels stay raw; they are never split or shortened.
        # Text with decoding errors never supplies a target identity assertion.
        if not target or bad or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._ -]*', model):
            continue
        facts = [('registry_' + key, value) for key, value in r.items()]
        facts += [('evidence_scope', SCOPE), ('csv_source_url', data['csv']['url'])]
        c.add(SOURCE, str(n), URL, target, model, target + ' ' + model,
              'registered equipment', r,
              [(r['manufacturer'], 'registry_manufacturer'),
               (r['certificate_holder'], 'certificate_holder'), (r['applicant'], 'regulatory_applicant')],
              [], facts)
        # Catalog aliases normalize ASCII; non-ASCII applicant/holder words
        # must not disappear and create a target affiliation around a name.
        for name in (r['manufacturer'], r['certificate_holder'], r['applicant']):
            if not company(name):
                c.db.execute('UPDATE affiliations SET target=NULL WHERE company=? AND record_id IN '
                             '(SELECT id FROM records WHERE source_id=? AND source_key=?)',
                             (name, SOURCE, str(n)))
        count += 1
    c.finish(SOURCE, len(rows))
    return dict(registry_rows=len(rows), model_assertions=count,
                rows_with_decoding_errors=sum(bool(e) for e in errors))
