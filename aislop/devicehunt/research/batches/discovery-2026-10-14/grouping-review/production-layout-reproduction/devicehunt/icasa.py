"""Searchable ICASA equipment rows without guessing manufacturers or broken cells."""
import json
import re
from pathlib import Path

from .catalog import digest

SOURCE = 'icasa-2025'
URL = 'https://www.icasa.org.za/uploads/files/List-of-Type-Approved-Equipment-2025.pdf'
PDF_SHA256 = 'c9b0585f1a3d7c61c6238bcb91748401db352c69555a8e2d35cb9e2fa0099be0'
HEADER = ['Name Model Category Type Approval Number', None, None, None]
PAGE_COUNT = 906


def extract(pdf_path, tables_path):
    pdf_path = Path(pdf_path)
    if digest(pdf_path.read_bytes()) != PDF_SHA256:
        raise ValueError('ICASA PDF differs from the reviewed snapshot')
    try:
        import pdfplumber
    except ImportError as exc:
        raise ValueError('ICASA PDF extraction requires optional pdfplumber') from exc
    pages = []
    with pdfplumber.open(pdf_path) as pdf:
        if len(pdf.pages) != PAGE_COUNT:
            raise ValueError('Unexpected ICASA page count')
        for number, page in enumerate(pdf.pages, 1):
            tables = page.extract_tables()
            if len(tables) != 1 or not tables[0] or tables[0][0] != HEADER:
                raise ValueError('ICASA table layout changed on page ' + str(number))
            pages.append({'page': number, 'rows': tables[0][1:]})
            page.close()
    Path(tables_path).write_text(json.dumps({'pages': pages}, ensure_ascii=False))


def parse_pages(data, expected_pages=PAGE_COUNT):
    pages = data.get('pages')
    if not isinstance(pages, list) or len(pages) != expected_pages:
        raise ValueError('Incomplete ICASA page set')
    parsed = []
    for number, page in enumerate(pages, 1):
        if page.get('page') != number or not isinstance(page.get('rows'), list) or not page['rows']:
            raise ValueError('Missing, duplicate or unordered ICASA page')
        for row_number, cells in enumerate(page['rows'], 1):
            if not isinstance(cells, list) or len(cells) != 4 or any(c is not None and not isinstance(c, str) for c in cells):
                raise ValueError('Unexpected ICASA row shape')
            clean = [' '.join((c or '').split()) for c in cells]
            valid = all(clean) and re.fullmatch(r'(?:TA|TE|PA)-\d{4}/\d+', clean[3])
            # A shifted/empty row is preserved but no named columns are asserted.
            fields = clean if valid else [None] * 4
            parsed.append((number, row_number, *fields,
                           'identified' if valid else 'review_required',
                           json.dumps(cells, ensure_ascii=False)))
    return parsed


def import_bundle(c, path):
    path = Path(path)
    data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('source_url') != URL
            or data.get('capture_format') != 'pdfplumber_positional_tables'):
        raise ValueError('Unrecognized ICASA extraction bundle')
    content = {}
    for key in ['pdf', 'tables']:
        entry = data[key]
        name = entry['path']
        if not isinstance(name, str) or Path(name).name != name or name in ('.', '..'):
            raise ValueError('ICASA input must be a filename within the bundle')
        p = (path.parent / name).resolve()
        if p.parent != path.parent.resolve():
            raise ValueError('ICASA file escapes bundle directory')
        content[key] = p.read_bytes()
        if digest(content[key]) != entry['sha256']:
            raise ValueError('ICASA input checksum mismatch: ' + key)
    if data['pdf']['sha256'] != PDF_SHA256:
        raise ValueError('ICASA PDF differs from reviewed snapshot')
    rows = parse_pages(json.loads(content['tables']))
    counts = {'rows': len(rows), 'identified': sum(r[6] == 'identified' for r in rows),
              'review_required': sum(r[6] == 'review_required' for r in rows)}
    if counts != data.get('expected'):
        raise ValueError('ICASA extraction count mismatch')
    c.source(SOURCE, URL, 'regulatory', 'ICASA factual public equipment register; source rights retained',
             'All 906 pages retained as positional table rows from the cached 2025 PDF. '
             'Clearly aligned name/model/category/TA/TE/PA-number rows are indexed; '
             'irregular rows retain raw cells only and require review. No manufacturer/applicant '
             'column is present, so no vendor or OEM relationship is inferred. Grouped models '
             'remain verbatim; listed approval numbers do not establish current validity.', path)
    c.db.execute('DELETE FROM equipment_register WHERE source_id=?', (SOURCE,))
    c.db.executemany('INSERT INTO equipment_register VALUES (?,?,?,?,?,?,?,?,?,?)',
                    [(SOURCE, *row, URL + '#page=' + str(row[0])) for row in rows])
    c.finish(SOURCE, len(rows))
    return {'pages': PAGE_COUNT, **counts}


def search(db, query='', limit=100):
    if limit < -1:
        raise ValueError('Limit must be -1 or nonnegative')
    conditions, args = [], []
    for token in query.split():
        conditions.append("cells_json LIKE ? ESCAPE '\\'")
        args.append('%' + token.replace('\\', '\\\\').replace('%', '\\%').replace('_', '\\_') + '%')
    clause = ' WHERE ' + ' AND '.join(conditions) if conditions else ''
    return [dict(r) for r in db.execute(
        'SELECT source_id,page,row_number,name,model,category,approval_number,parse_status,cells_json,source_url '
        'FROM equipment_register' + clause + ' ORDER BY source_id,page,row_number LIMIT ?', args + [limit])]
